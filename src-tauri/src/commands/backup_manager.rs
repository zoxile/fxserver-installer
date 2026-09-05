use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    models::mariadb::{MariaDBBackupOptions, MariaDBCredentials},
    process::CommandNoWindowExt,
    services::mariadb::{
        backup::{create_backup, run_backup_client},
        query::{
            apply_credentials_args, execute_query, find_mariadb_client, list_databases, list_tables,
        },
    },
};

#[path = "../services/mariadb/backup_storage.rs"]
mod storage;
use storage::{now_ms, unique_id, validate_database, validate_id};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSchedule {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub database: String,
    pub output_dir: String,
    pub interval_minutes: u32,
    pub retain_count: usize,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleStatus {
    pub config: BackupSchedule,
    #[serde(skip_deserializing)]
    pub enabled: bool,
    #[serde(skip_deserializing)]
    pub running: bool,
    #[serde(skip_deserializing)]
    pub next_run: Option<u64>,
    pub last_run: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSnapshot {
    pub id: String,
    pub workspace_id: String,
    pub schedule_id: String,
    pub database: String,
    pub directory: String,
    pub created_at: u64,
    pub size_bytes: u64,
    pub sha256: String,
    pub kind: String,
    pub source_host: String,
    pub source_port: u16,
}

#[derive(Default, Clone, Deserialize, Serialize)]
struct Registry {
    schedules: Vec<ScheduleStatus>,
    snapshots: Vec<BackupSnapshot>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOverview {
    schedules: Vec<ScheduleStatus>,
    snapshots: Vec<BackupSnapshot>,
    busy: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEvent {
    pub workspace_id: String,
    pub schedule_id: String,
    pub stage: String,
    pub message: String,
    pub timestamp: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    token: String,
    snapshot: BackupSnapshot,
    target_host: String,
    target_port: u16,
    target_database: String,
    existing_tables: usize,
    expires_at: u64,
    warnings: Vec<String>,
}

struct RestorePermit {
    preview: RestorePreview,
    credentials: MariaDBCredentials,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    recovery_snapshot: BackupSnapshot,
    message: String,
}

#[derive(Default)]
struct Inner {
    path: Option<PathBuf>,
    registry: Registry,
    credentials: HashMap<String, MariaDBCredentials>,
    previews: HashMap<String, RestorePermit>,
}

#[derive(Clone, Default)]
pub struct BackupManager {
    inner: Arc<Mutex<Inner>>,
    operation: Arc<Mutex<()>>,
    started: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

impl BackupManager {
    pub fn start(&self, app: AppHandle) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if manager.stopped.load(Ordering::Relaxed) {
                    break;
                }
                let worker = manager.clone();
                let app = app.clone();
                if let Err(error) = super::run_blocking(move || worker.tick(&app)).await {
                    log::error!("Backup scheduler: {error}");
                }
            }
        });
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub fn stop_and_wait(&self) -> Result<(), String> {
        self.stop();
        let _operation = self
            .operation
            .lock()
            .map_err(|_| "Backup worker lock is unavailable.")?;
        Ok(())
    }

    pub fn is_busy(&self) -> bool {
        self.operation.try_lock().is_err()
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, String> {
        self.inner
            .lock()
            .map_err(|_| "Backup settings lock is unavailable.".into())
    }

    fn initialize(&self, app: &AppHandle) -> Result<(), String> {
        let mut inner = self.lock()?;
        if inner.path.is_some() {
            return Ok(());
        }
        let directory = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("backup-manager");
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let path = directory.join("registry.json");
        if path.exists() {
            let content = fs::read(&path).map_err(|e| e.to_string())?;
            inner.registry = serde_json::from_slice(&content).map_err(|e| {
                format!("Backup registry could not be read; it has not been overwritten: {e}")
            })?;
        }
        inner.path = Some(path);
        Ok(())
    }

    fn persist(inner: &Inner) -> Result<(), String> {
        storage::save_json(
            inner
                .path
                .as_ref()
                .ok_or("Backup manager is not initialized.")?,
            &inner.registry,
        )
    }

    fn tick(&self, app: &AppHandle) -> Result<(), String> {
        if self.stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.initialize(app)?;
        let Ok(_operation) = self.operation.try_lock() else {
            return Ok(());
        };
        if self.stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Ok(_database) = super::mariadb::database_access() else {
            return Ok(());
        };
        let due = {
            let inner = self.lock()?;
            inner
                .registry
                .schedules
                .iter()
                .find(|schedule| is_due(schedule, now_ms()))
                .and_then(|schedule| {
                    inner
                        .credentials
                        .get(&schedule.config.id)
                        .map(|credentials| (schedule.config.clone(), credentials.clone()))
                })
        };
        if let Some((config, credentials)) = due {
            self.execute_backup(app, config, credentials, "scheduled")?;
        }
        Ok(())
    }

    fn execute_backup(
        &self,
        app: &AppHandle,
        config: BackupSchedule,
        credentials: MariaDBCredentials,
        kind: &str,
    ) -> Result<BackupSnapshot, String> {
        {
            let mut inner = self.lock()?;
            if let Some(schedule) = inner
                .registry
                .schedules
                .iter_mut()
                .find(|s| s.config.id == config.id)
            {
                schedule.running = true;
                schedule.next_run = schedule
                    .enabled
                    .then(|| now_ms() + u64::from(config.interval_minutes) * 60_000);
            }
        }
        publish(
            app,
            &config,
            "running",
            "Checking disk space and creating a database backup.",
        );
        let result = self
            .capture(&config, &credentials, kind)
            .and_then(|snapshot| {
                let mut inner = self.lock()?;
                inner.registry.snapshots.push(snapshot.clone());
                Self::persist(&inner)?;
                apply_retention(&mut inner.registry, &config)?;
                Self::persist(&inner)?;
                Ok(snapshot)
            })
            .map_err(|error| redact_error(error, &credentials));
        {
            let mut inner = self.lock()?;
            if let Some(schedule) = inner
                .registry
                .schedules
                .iter_mut()
                .find(|s| s.config.id == config.id)
            {
                schedule.running = false;
                schedule.last_run = Some(now_ms());
                schedule.last_error = result.as_ref().err().cloned();
                schedule.next_run = schedule
                    .enabled
                    .then(|| now_ms() + u64::from(config.interval_minutes) * 60_000);
            }
            if let Err(error) = Self::persist(&inner) {
                publish(
                    app,
                    &config,
                    "error",
                    &format!("Backup state could not be saved: {error}"),
                );
                return Err(error);
            }
        }
        match &result {
            Ok(_) => publish(
                app,
                &config,
                "completed",
                "Database backup completed and checksum verified.",
            ),
            Err(error) => publish(app, &config, "error", error),
        }
        result
    }

    fn capture(
        &self,
        config: &BackupSchedule,
        credentials: &MariaDBCredentials,
        kind: &str,
    ) -> Result<BackupSnapshot, String> {
        validate_config(config)?;
        assert_database_exists(credentials, &config.database)?;
        let directory =
            storage::owned_directory(&config.output_dir, &config.workspace_id, &config.id)?;
        let previous_size = self
            .lock()?
            .registry
            .snapshots
            .iter()
            .filter(|s| s.workspace_id == config.workspace_id && s.schedule_id == config.id)
            .map(|s| s.size_bytes)
            .max()
            .unwrap_or(0);
        check_disk(credentials, &config.database, &directory, previous_size)?;
        let id = unique_id();
        let options = MariaDBBackupOptions {
            output_dir: directory.to_string_lossy().into_owned(),
            file_name: Some(format!("{id}.sql")),
            database: Some(config.database.clone()),
            tables: vec![],
            all_databases: false,
            schema_only: false,
            data_only: false,
            include_routines: true,
            include_triggers: true,
            include_events: true,
            single_transaction: true,
            add_drop_statements: false,
            where_clause: None,
        };
        let result = create_backup(credentials.clone(), options)?;
        if result.size_bytes == 0 {
            return Err("MariaDB produced an empty backup; retention was not applied.".into());
        }
        let mut file = storage::open_snapshot(&PathBuf::from(&result.path))?;
        let sha256 = storage::sha256(&mut file)?;
        Ok(BackupSnapshot {
            id,
            workspace_id: config.workspace_id.clone(),
            schedule_id: config.id.clone(),
            database: config.database.clone(),
            directory: directory.to_string_lossy().into_owned(),
            created_at: now_ms(),
            size_bytes: result.size_bytes,
            sha256,
            kind: kind.into(),
            source_host: credentials.host.clone(),
            source_port: credentials.port,
        })
    }
}

fn is_due(schedule: &ScheduleStatus, now: u64) -> bool {
    schedule.enabled && !schedule.running && schedule.next_run.is_some_and(|next| next <= now)
}

fn validate_config(config: &BackupSchedule) -> Result<(), String> {
    validate_id(&config.id)?;
    validate_id(&config.workspace_id)?;
    validate_database(&config.database)?;
    if config.name.trim().is_empty() || config.name.len() > 100 {
        return Err("Enter a schedule name of up to 100 characters.".into());
    }
    if !(5..=10080).contains(&config.interval_minutes) {
        return Err("Backup intervals must be between 5 minutes and 7 days.".into());
    }
    if !(1..=100).contains(&config.retain_count) {
        return Err("Retain between 1 and 100 backups per schedule.".into());
    }
    if !PathBuf::from(&config.output_dir).is_dir() {
        return Err("Choose an existing backup output folder.".into());
    }
    Ok(())
}

fn redact_error(error: String, credentials: &MariaDBCredentials) -> String {
    if credentials.password.is_empty() {
        error
    } else {
        error.replace(&credentials.password, "[redacted]")
    }
}

fn publish(app: &AppHandle, config: &BackupSchedule, stage: &str, message: &str) {
    let event = BackupEvent {
        workspace_id: config.workspace_id.clone(),
        schedule_id: config.id.clone(),
        stage: stage.into(),
        message: message.into(),
        timestamp: now_ms(),
    };
    let level = match stage {
        "error" => "error",
        "completed" => "success",
        _ => "info",
    };
    super::logs::append_background_log(app, level, "mariadb.backup", message);
    let _ = app.emit("backup-manager-progress", event);
}

fn assert_database_exists(credentials: &MariaDBCredentials, database: &str) -> Result<(), String> {
    validate_database(database)?;
    let mut credentials = credentials.clone();
    credentials.database = None;
    if !list_databases(credentials)?
        .iter()
        .any(|name| name == database)
    {
        return Err(
            "The selected database no longer exists or is not accessible. No SQL was executed."
                .into(),
        );
    }
    Ok(())
}

fn check_disk(
    credentials: &MariaDBCredentials,
    database: &str,
    directory: &std::path::Path,
    previous_size: u64,
) -> Result<(), String> {
    let database_hex: String = database.bytes().map(|byte| format!("{byte:02x}")).collect();
    let query = format!("SELECT COALESCE(SUM(DATA_LENGTH + INDEX_LENGTH), 0) AS bytes FROM information_schema.TABLES WHERE TABLE_SCHEMA = CONVERT(0x{database_hex} USING utf8mb4);");
    let mut credentials = credentials.clone();
    credentials.database = None;
    let result = execute_query(credentials, query)?;
    if !result.success {
        return Err(format!("Could not estimate backup size: {}", result.stderr));
    }
    let estimate = result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or("MariaDB did not return a database size estimate.")?;
    storage::require_headroom(
        storage::available_bytes(directory)?,
        estimate.max(previous_size),
    )
}

fn apply_retention(registry: &mut Registry, config: &BackupSchedule) -> Result<(), String> {
    let mut owned: Vec<_> = registry
        .snapshots
        .iter()
        .filter(|s| {
            s.workspace_id == config.workspace_id
                && s.schedule_id == config.id
                && s.kind != "recovery"
        })
        .cloned()
        .collect();
    owned.sort_by_key(|s| std::cmp::Reverse(s.created_at));
    for snapshot in owned.into_iter().skip(config.retain_count) {
        recovery_config(&snapshot)?;
        let expected = PathBuf::from(&snapshot.directory).join(format!("{}.sql", snapshot.id));
        if fs::symlink_metadata(&expected)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            registry
                .snapshots
                .retain(|s| s.id != snapshot.id || s.workspace_id != snapshot.workspace_id);
            continue;
        }
        let path = storage::snapshot_path(&snapshot.directory, &snapshot.id)?;
        let mut file = storage::open_snapshot(&path)?;
        if storage::sha256(&mut file)? != snapshot.sha256 {
            return Err(
                "Retention skipped a backup whose contents changed outside the app.".into(),
            );
        }
        drop(file);
        fs::remove_file(path).map_err(|e| {
            format!("Backup was created, but an older owned file could not be removed: {e}")
        })?;
        registry
            .snapshots
            .retain(|s| s.id != snapshot.id || s.workspace_id != snapshot.workspace_id);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_backup_manager(
    app: AppHandle,
    manager: State<'_, BackupManager>,
    workspace_id: String,
) -> Result<BackupOverview, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        validate_id(&workspace_id)?;
        manager.initialize(&app)?;
        let inner = manager.lock()?;
        Ok(BackupOverview {
            schedules: inner
                .registry
                .schedules
                .iter()
                .filter(|s| s.config.workspace_id == workspace_id)
                .cloned()
                .collect(),
            snapshots: inner
                .registry
                .snapshots
                .iter()
                .filter(|s| s.workspace_id == workspace_id)
                .rev()
                .cloned()
                .collect(),
            busy: manager.is_busy(),
        })
    })
    .await
}

#[tauri::command]
pub async fn save_backup_schedule(
    app: AppHandle,
    manager: State<'_, BackupManager>,
    config: BackupSchedule,
    enabled: bool,
    credentials: Option<MariaDBCredentials>,
) -> Result<(), String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = manager
            .operation
            .try_lock()
            .map_err(|_| "A backup or restore is in progress.")?;
        manager.initialize(&app)?;
        validate_config(&config)?;
        let _database = super::mariadb::database_access()?;
        if enabled {
            let credentials = credentials.as_ref().ok_or(
                "Validate credentials to enable this schedule for the current app session.",
            )?;
            assert_database_exists(credentials, &config.database)
                .map_err(|e| redact_error(e, credentials))?;
        }
        let mut inner = manager.lock()?;
        let previous = inner
            .registry
            .schedules
            .iter()
            .find(|s| s.config.id == config.id)
            .cloned();
        if previous
            .as_ref()
            .is_some_and(|s| s.config.workspace_id != config.workspace_id)
        {
            return Err("Schedule belongs to a different workspace.".into());
        }
        if previous.is_none() && inner.registry.schedules.len() >= 100 {
            return Err("The app supports up to 100 saved backup schedules.".into());
        }
        let status = ScheduleStatus {
            config: config.clone(),
            enabled,
            running: false,
            next_run: enabled.then(|| now_ms() + u64::from(config.interval_minutes) * 60_000),
            last_run: previous.as_ref().and_then(|s| s.last_run),
            last_error: None,
        };
        let mut registry = inner.registry.clone();
        registry.schedules.retain(|s| s.config.id != config.id);
        registry.schedules.push(status);
        storage::save_json(
            inner
                .path
                .as_ref()
                .ok_or("Backup manager is not initialized.")?,
            &registry,
        )?;
        inner.registry = registry;
        if enabled {
            inner
                .credentials
                .insert(config.id, credentials.ok_or("Credentials are required.")?);
        } else {
            inner.credentials.remove(&config.id);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_backup_schedule(
    app: AppHandle,
    manager: State<'_, BackupManager>,
    workspace_id: String,
    schedule_id: String,
) -> Result<(), String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = manager
            .operation
            .try_lock()
            .map_err(|_| "A backup or restore is in progress.")?;
        manager.initialize(&app)?;
        let mut inner = manager.lock()?;
        if !inner
            .registry
            .schedules
            .iter()
            .any(|s| s.config.workspace_id == workspace_id && s.config.id == schedule_id)
        {
            return Err("Backup schedule not found in this workspace.".into());
        }
        let mut registry = inner.registry.clone();
        registry
            .schedules
            .retain(|s| s.config.workspace_id != workspace_id || s.config.id != schedule_id);
        storage::save_json(
            inner
                .path
                .as_ref()
                .ok_or("Backup manager is not initialized.")?,
            &registry,
        )?;
        inner.registry = registry;
        inner.credentials.remove(&schedule_id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn run_scheduled_backup_now(
    app: AppHandle,
    manager: State<'_, BackupManager>,
    workspace_id: String,
    schedule_id: String,
    credentials: MariaDBCredentials,
) -> Result<BackupSnapshot, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = manager
            .operation
            .try_lock()
            .map_err(|_| "A backup or restore is in progress.")?;
        manager.initialize(&app)?;
        let config = manager
            .lock()?
            .registry
            .schedules
            .iter()
            .find(|s| s.config.workspace_id == workspace_id && s.config.id == schedule_id)
            .map(|s| s.config.clone())
            .ok_or("Backup schedule not found in this workspace.")?;
        let _database = super::mariadb::database_access()?;
        manager.execute_backup(&app, config, credentials, "manual")
    })
    .await
}

#[tauri::command]
pub async fn preview_backup_restore(
    app: AppHandle,
    manager: State<'_, BackupManager>,
    workspace_id: String,
    snapshot_id: String,
    credentials: MariaDBCredentials,
) -> Result<RestorePreview, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = manager.operation.try_lock().map_err(|_| "A backup or restore is in progress.")?;
        let _database = super::mariadb::database_access()?;
        manager.initialize(&app)?;
        let snapshot = manager.lock()?.registry.snapshots.iter()
            .find(|s| s.workspace_id == workspace_id && s.id == snapshot_id).cloned()
            .ok_or("Backup snapshot not found in this workspace.")?;
        let result = (|| {
            assert_database_exists(&credentials, &snapshot.database)?;
            let path = storage::snapshot_path(&snapshot.directory, &snapshot.id)?;
            let mut file = storage::open_snapshot(&path)?;
            if storage::sha256(&mut file)? != snapshot.sha256 { return Err("Backup checksum changed. Restore has been blocked.".into()); }
            let mut scoped = credentials.clone();
            scoped.database = Some(snapshot.database.clone());
            let existing_tables = list_tables(scoped.clone(), snapshot.database.clone())?.len();
            check_disk(&credentials, &snapshot.database, &PathBuf::from(&snapshot.directory), snapshot.size_bytes)?;
            let preview = RestorePreview {
                token: unique_id(), target_host: credentials.host.clone(), target_port: credentials.port,
                target_database: snapshot.database.clone(), snapshot, existing_tables, expires_at: now_ms() + 300_000,
                warnings: vec![
                    "Existing tables and data can be replaced. Stop FXServer and other database writers first.".into(),
                    "A recovery backup must succeed before the restore starts. Recovery backups are never deleted by schedule retention.".into(),
                    "SQL restores are not atomic. A failure can leave partial changes. Review the recovery backup before retrying.".into(),
                    "Stored routines, triggers and events may reference other schemas; use a database-scoped account to limit access.".into(),
                ],
            };
            let mut inner = manager.lock()?;
            inner.previews.retain(|_, permit| permit.preview.expires_at > now_ms());
            if inner.previews.len() >= 20 { inner.previews.clear(); }
            inner.previews.insert(preview.token.clone(), RestorePermit { preview: preview.clone(), credentials: scoped });
            Ok(preview)
        })();
        result.map_err(|e| redact_error(e, &credentials))
    }).await
}

#[tauri::command]
pub async fn restore_backup_snapshot(
    app: AppHandle,
    manager: State<'_, BackupManager>,
    workspace_id: String,
    token: String,
    confirmation_database: String,
) -> Result<RestoreResult, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = manager.operation.try_lock().map_err(|_| "A backup or restore is in progress.")?;
        let _database = super::mariadb::maintenance_access()?;
        manager.initialize(&app)?;
        let permit = manager.lock()?.previews.remove(&token).ok_or("Preview expired. Review this restore again.")?;
        validate_restore_permit(&permit.preview, &workspace_id, &confirmation_database, now_ms())?;
        let snapshot = &permit.preview.snapshot;
        let config = recovery_config(snapshot)?;
        let result = (|| {
            assert_database_exists(&permit.credentials, &snapshot.database)?;
            let path = storage::snapshot_path(&snapshot.directory, &snapshot.id)?;
            let mut file = storage::open_snapshot(&path)?;
            if storage::sha256(&mut file)? != snapshot.sha256 { return Err("Backup changed after preview. Restore has been blocked.".into()); }
            publish(&app, &config, "running", "Creating a recovery backup before restoring the selected snapshot.");
            let recovery = manager.capture(&config, &permit.credentials, "recovery")?;
            {
                let mut inner = manager.lock()?;
                inner.registry.snapshots.push(recovery.clone());
                BackupManager::persist(&inner)?;
            }
            publish(&app, &config, "running", "Recovery backup verified. Streaming SQL into the selected database.");
            let client = find_mariadb_client().ok_or("MariaDB client is unavailable.")?;
            let mut command = Command::new(client);
            command.no_window();
            apply_credentials_args(&mut command, &permit.credentials);
            command.arg("--binary-mode").arg("--connect-timeout=10").arg("--default-character-set=utf8mb4")
                .arg("--one-database").arg(format!("--database={}", snapshot.database)).stdin(Stdio::from(file));
            if let Err(error) = run_backup_client(&mut command, "MariaDB restore") {
                return Err(format!("Restore failed and may have applied partial changes. Recovery backup: {}/{}.sql. {error}", recovery.directory, recovery.id));
            }
            Ok(RestoreResult { recovery_snapshot: recovery, message: "Database restore completed. Review the server before resuming writes.".into() })
        })().map_err(|error| redact_error(error, &permit.credentials));
        match &result {
            Ok(_) => publish(&app, &config, "completed", "Database restore completed."),
            Err(error) => publish(&app, &config, "error", error),
        }
        result
    }).await
}

fn validate_restore_permit(
    preview: &RestorePreview,
    workspace: &str,
    confirmation: &str,
    now: u64,
) -> Result<(), String> {
    if preview.snapshot.workspace_id != workspace {
        return Err("Restore belongs to another workspace.".into());
    }
    if preview.expires_at <= now {
        return Err("Preview expired. Review this restore again.".into());
    }
    if confirmation != preview.target_database {
        return Err("Type the exact database name to confirm the restore.".into());
    }
    Ok(())
}

fn recovery_config(snapshot: &BackupSnapshot) -> Result<BackupSchedule, String> {
    let directory = std::path::Path::new(&snapshot.directory);
    let workspace = directory
        .parent()
        .ok_or("Backup directory has no workspace parent.")?;
    let managed = workspace
        .parent()
        .ok_or("Backup directory has no managed root.")?;
    if directory.file_name() != Some(std::ffi::OsStr::new(&snapshot.schedule_id))
        || workspace.file_name() != Some(std::ffi::OsStr::new(&snapshot.workspace_id))
        || managed.file_name() != Some(std::ffi::OsStr::new("fxserver-managed-backups"))
    {
        return Err("Backup directory does not match its recorded workspace and schedule.".into());
    }
    let root = managed
        .parent()
        .ok_or("Backup directory has no output root.")?;
    Ok(BackupSchedule {
        id: "restore-recovery".into(),
        workspace_id: snapshot.workspace_id.clone(),
        name: "Before restore".into(),
        database: snapshot.database.clone(),
        output_dir: root.to_string_lossy().into_owned(),
        interval_minutes: 60,
        retain_count: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_config(output: &str) -> BackupSchedule {
        BackupSchedule {
            id: "daily".into(),
            workspace_id: "development".into(),
            name: "Daily".into(),
            database: "qbx".into(),
            output_dir: output.into(),
            interval_minutes: 60,
            retain_count: 1,
        }
    }

    #[test]
    fn schedules_reload_paused_without_credentials_or_deadlines() {
        let status = ScheduleStatus {
            config: fixture_config("C:/backups"),
            enabled: true,
            running: true,
            next_run: Some(100),
            last_run: Some(50),
            last_error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("password"));
        let restored: ScheduleStatus = serde_json::from_str(&json).unwrap();
        assert!(!restored.enabled && !restored.running && restored.next_run.is_none());
        assert!(!is_due(&restored, 1000));
    }

    #[test]
    fn due_time_does_not_overlap_a_running_or_paused_backup() {
        let mut status = ScheduleStatus {
            config: fixture_config("C:/backups"),
            enabled: true,
            running: false,
            next_run: Some(100),
            last_run: None,
            last_error: None,
        };
        assert!(!is_due(&status, 99));
        assert!(is_due(&status, 100));
        status.running = true;
        assert!(!is_due(&status, 101));
        status.running = false;
        status.enabled = false;
        assert!(!is_due(&status, 101));
    }

    #[test]
    fn shutdown_waits_for_active_work_without_holding_the_settings_lock() {
        let manager = BackupManager::default();
        let operation = manager.operation.lock().unwrap();
        let worker = manager.clone();
        let (done, receiver) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            worker.stop_and_wait().unwrap();
            done.send(()).unwrap();
        });
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(manager.lock().is_ok());
        drop(operation);
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.join().unwrap();
        assert!(manager.stopped.load(Ordering::Relaxed));
    }

    #[cfg(windows)]
    #[test]
    fn retention_never_deletes_untracked_other_workspace_or_recovery_files() {
        let root = std::env::temp_dir().join(format!("fx-retention-{}", unique_id()));
        fs::create_dir(&root).unwrap();
        let config = fixture_config(root.to_str().unwrap());
        let directory =
            storage::owned_directory(&config.output_dir, &config.workspace_id, &config.id).unwrap();
        let mut registry = Registry::default();
        for (id, workspace, kind, created_at) in [
            ("old", "development", "scheduled", 1),
            ("new", "development", "scheduled", 2),
            ("safe", "development", "recovery", 0),
            ("other", "production", "scheduled", 0),
        ] {
            let path = directory.join(format!("{id}.sql"));
            fs::write(&path, b"SELECT 1;").unwrap();
            let sha256 = storage::sha256(&mut storage::open_snapshot(&path).unwrap()).unwrap();
            registry.snapshots.push(BackupSnapshot {
                id: id.into(),
                workspace_id: workspace.into(),
                schedule_id: config.id.clone(),
                database: "qbx".into(),
                directory: directory.to_string_lossy().into_owned(),
                created_at,
                size_bytes: 9,
                sha256,
                kind: kind.into(),
                source_host: "localhost".into(),
                source_port: 3306,
            });
        }
        fs::write(directory.join("untracked.sql"), b"do not delete").unwrap();
        apply_retention(&mut registry, &config).unwrap();
        assert!(!directory.join("old.sql").exists());
        for name in ["new.sql", "safe.sql", "other.sql", "untracked.sql"] {
            assert!(directory.join(name).exists());
        }
        let recovery = recovery_config(&registry.snapshots[0]).unwrap();
        assert_eq!(
            PathBuf::from(recovery.output_dir),
            root.canonicalize().unwrap()
        );
        let mut preview = RestorePreview {
            token: "test".into(),
            snapshot: registry.snapshots[0].clone(),
            target_host: "localhost".into(),
            target_port: 3306,
            target_database: "qbx".into(),
            existing_tables: 0,
            expires_at: 100,
            warnings: vec![],
        };
        assert!(validate_restore_permit(&preview, "development", "qbx", 99).is_ok());
        assert!(validate_restore_permit(&preview, "production", "qbx", 99).is_err());
        assert!(validate_restore_permit(&preview, "development", "qb", 99).is_err());
        preview.expires_at = 99;
        assert!(validate_restore_permit(&preview, "development", "qbx", 99).is_err());
        for name in ["new.sql", "safe.sql", "other.sql", "untracked.sql"] {
            fs::remove_file(directory.join(name)).unwrap();
        }
        fs::remove_dir(&directory).unwrap();
        fs::remove_dir(directory.parent().unwrap()).unwrap();
        fs::remove_dir(root.join("fxserver-managed-backups")).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
