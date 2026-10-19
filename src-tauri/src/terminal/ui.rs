use super::{actions as operations, safe_text, settings::Settings};
use crate::{
    commands::fxserver::FxserverManager,
    models::{
        artifact::ArtifactBuild,
        fxserver::{
            FxserverCommandRequest, FxserverRconConfig, FxserverResourceInfo, FxserverStatus,
            FxserverTerminalEntry, SaveServerConfigRequest, ServerConfigFile, TxDataProfilesResult,
        },
        mariadb::MariaDBCredentials,
    },
};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, ListItem, Paragraph, Tabs, Wrap},
    Frame,
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::runtime::Runtime;

mod actions;
mod config;
mod console;
mod lists;
mod performance;
mod render;
mod resources;
mod worker;
use render::{ConfirmationPreview, Input};
use worker::{monitor, worker};

const TABS: [&str; 9] = [
    "Server",
    "Console",
    "Resources",
    "Database",
    "Artifacts",
    "Checks",
    "Settings",
    "Actions",
    "Config",
];
const FIELDS: [&str; 11] = [
    "Artifact folder",
    "txData folder",
    "Profile",
    "RCON host",
    "RCON port",
    "Database host",
    "Database port",
    "Database user",
    "Database",
    "RCON password",
    "Database password",
];
const MAX_LINES: usize = 2000;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

enum Job {
    LoadCredentials(Box<operations::Context>),
    Profiles(Settings),
    LoadConfig(Settings),
    SaveConfig(Settings, SaveServerConfigRequest, String),
    SaveSettings(PathBuf, Settings),
    Action(Box<(operations::Context, operations::Submission)>),
    Start(Settings, bool),
    Stop,
    Restart(Settings),
    Rcon(FxserverCommandRequest),
    Resources(Settings),
    Catalog,
    Install(ArtifactBuild, String),
    Query(MariaDBCredentials, String),
    Databases(MariaDBCredentials),
    Check(Settings),
    ReadSql(PathBuf),
    Quit,
}

enum Update {
    Credentials(
        Box<operations::Context>,
        Result<Option<MariaDBCredentials>, String>,
        Result<Option<String>, String>,
    ),
    Databases(Result<Vec<String>, String>),
    Profiles(Result<TxDataProfilesResult, String>),
    Config(Settings, Result<Vec<ServerConfigFile>, String>),
    SavedConfig(Result<ServerConfigFile, String>),
    Action(Result<operations::Outcome, String>),
    Complete(Result<String, String>),
    Rcon(Result<String, String>),
    Resources(Settings, Result<Vec<FxserverResourceInfo>, String>),
    Catalog(Result<Vec<ArtifactBuild>, String>),
    Sample(Option<FxserverStatus>, Vec<FxserverTerminalEntry>),
    SqlFile(Result<String, String>),
    Stopped(Result<(), String>),
}

enum Edit {
    Console,
    Query,
    Setting(usize),
    SqlFile,
}

enum Selection {
    Database,
    Profile(String),
}

struct Picker {
    kind: Selection,
    values: Vec<String>,
    selected: usize,
}

#[derive(Clone, Copy, Default)]
struct CredentialRevision {
    database: u64,
    rcon: u64,
}

struct App {
    manager: FxserverManager,
    progress: Option<SyncSender<String>>,
    actions: actions::Panel,
    config: config::Panel,
    performance: performance::History,
    picker: Option<Picker>,
    settings: Settings,
    config_path: PathBuf,
    rcon_password: String,
    database_password: String,
    credential_revision: CredentialRevision,
    pending_credential_revision: Option<CredentialRevision>,
    tab: usize,
    selected: usize,
    input: Input,
    edit: Option<Edit>,
    confirm: Option<(ConfirmationPreview, Job)>,
    confirmation_visible: bool,
    busy: bool,
    server_transition: bool,
    quitting: bool,
    notice: String,
    error: bool,
    console: console::Panel,
    output: Vec<String>,
    resources: resources::Panel,
    builds: Vec<ArtifactBuild>,
    status: Option<FxserverStatus>,
    scroll: usize,
    started: Instant,
}

impl App {
    fn new(settings: Settings, config_path: PathBuf) -> Self {
        let mut app = Self {
            manager: FxserverManager::default(),
            progress: None,
            actions: actions::Panel::new(Vec::new()),
            config: config::Panel::default(),
            performance: performance::History::default(),
            picker: None,
            settings,
            config_path,
            rcon_password: String::new(),
            database_password: String::new(),
            credential_revision: CredentialRevision::default(),
            pending_credential_revision: None,
            tab: 0,
            selected: 0,
            input: Input::default(),
            edit: None,
            confirm: None,
            confirmation_visible: false,
            busy: false,
            server_transition: false,
            quitting: false,
            notice: "Ready".into(),
            error: false,
            console: console::Panel::default(),
            output: Vec::new(),
            resources: resources::Panel::default(),
            builds: Vec::new(),
            status: None,
            scroll: 0,
            started: Instant::now(),
        };
        app.actions.refresh(operations::list(&app.context()));
        app
    }
    fn paste(&mut self, value: &str) {
        if self.picker.is_some() {
            return;
        }
        if self.tab == 1 && self.edit.is_none() && self.confirm.is_none() && !self.quitting {
            self.console.paste(value);
            return;
        }
        if self.tab == 2 && self.edit.is_none() && self.confirm.is_none() && !self.quitting {
            self.resources.paste(value);
            return;
        }
        if self.tab == 8 && self.confirm.is_none() && !self.quitting {
            self.config.paste(value);
            return;
        }
        if self.tab == 7 && self.confirm.is_none() && !self.quitting {
            self.actions.paste(value);
            return;
        }
        if self.edit.is_none() || self.confirm.is_some() || self.quitting {
            return;
        }
        if let Err(error) = self.input.paste(value) {
            self.notice = error.into();
            self.error = true;
        }
    }

    fn credentials(&self) -> MariaDBCredentials {
        MariaDBCredentials {
            host: self.settings.database_host.clone(),
            port: self.settings.database_port,
            username: self.settings.database_user.clone(),
            password: self.database_password.clone(),
            database: (!self.settings.database.is_empty()).then(|| self.settings.database.clone()),
        }
    }
    fn context(&self) -> operations::Context {
        operations::Context {
            settings: self.settings.clone(),
            credentials: self.credentials(),
            rcon: self.rcon(String::new()).rcon,
            manager: self.manager.clone(),
            progress: self.progress.clone(),
        }
    }

    fn open_actions(&mut self) {
        let mut actions = operations::list(&self.context());
        if self.tab == 2 {
            if let Some(resource) = self.resources.selected() {
                for action in &mut actions {
                    for field in &mut action.fields {
                        if field.key == "resource_path"
                            || (action.group == "Resources" && field.key == "resource")
                        {
                            field.value.clone_from(&resource.path);
                        }
                    }
                }
            }
        }
        self.actions.refresh(actions);
        self.tab = 7;
        self.edit = None;
        self.input = Input::default();
    }
    fn rcon(&self, command: String) -> FxserverCommandRequest {
        FxserverCommandRequest {
            command,
            rcon: FxserverRconConfig {
                host: self.settings.rcon_host.clone(),
                port: self.settings.rcon_port,
                password: self.rcon_password.clone(),
            },
        }
    }
    fn submit(&mut self, sender: &SyncSender<Job>, job: Job) {
        if self.busy {
            self.notice = "An operation is already running. You can continue browsing.".into();
            return;
        }
        let quitting = matches!(job, Job::Quit);
        let server_transition = matches!(job, Job::Start(..) | Job::Stop | Job::Restart(..));
        // The worker is serial: this stamp belongs to its sole in-flight job.
        let credential_revision = matches!(job, Job::LoadCredentials(_) | Job::Action(_))
            .then_some(self.credential_revision);
        match sender.try_send(job) {
            Ok(()) => {
                self.pending_credential_revision = credential_revision;
                self.quitting = quitting;
                self.server_transition = server_transition;
                self.busy = true;
                self.error = false;
                self.notice = "Working".into();
                self.started = Instant::now();
            }
            Err(_) => {
                self.error = true;
                self.notice = "The worker is busy or unavailable.".into();
            }
        }
    }
    fn require_confirmation(&mut self, title: String, job: Job) {
        if self.busy {
            self.notice = "Wait for the current operation before starting another.".into();
            return;
        }
        self.confirm = Some((ConfirmationPreview::new(title), job));
        self.confirmation_visible = false;
    }
    fn set_result(&mut self, result: Result<String, String>) {
        self.busy = false;
        self.server_transition = false;
        self.error = result.is_err();
        match result {
            Ok(text) => {
                self.notice = "Complete".into();
                self.output = safe_text(&text)
                    .lines()
                    .take(MAX_LINES)
                    .map(str::to_owned)
                    .collect();
                self.scroll = 0;
            }
            Err(error) => {
                self.notice = safe_text(&error);
            }
        }
    }
    fn update(&mut self, update: Update) -> Option<Result<(), String>> {
        match update {
            Update::Credentials(context, database, rcon) => {
                self.busy = false;
                let pending = self.pending_credential_revision.take();
                let mut failures = Vec::new();
                if pending
                    .is_some_and(|revision| revision.database == self.credential_revision.database)
                    && operations::database::saved_login_id(&context.settings)
                        == operations::database::saved_login_id(&self.settings)
                    && self.database_password.is_empty()
                {
                    match database {
                        Ok(Some(credentials)) => {
                            self.database_password = credentials.password;
                            self.credential_revision.database =
                                self.credential_revision.database.wrapping_add(1);
                        }
                        Err(error) => {
                            failures.push(format!("Database login: {}", safe_text(&error)))
                        }
                        _ => {}
                    }
                }
                if pending.is_some_and(|revision| revision.rcon == self.credential_revision.rcon)
                    && context.settings.tx_data_path == self.settings.tx_data_path
                    && context.settings.profile == self.settings.profile
                    && context.rcon.host == self.settings.rcon_host
                    && context.rcon.port == self.settings.rcon_port
                    && self.rcon_password.is_empty()
                {
                    match rcon {
                        Ok(Some(password)) => {
                            self.rcon_password = password;
                            self.credential_revision.rcon =
                                self.credential_revision.rcon.wrapping_add(1);
                        }
                        Err(error) => {
                            failures.push(format!("RCON password: {}", safe_text(&error)))
                        }
                        _ => {}
                    }
                }
                self.error = !failures.is_empty();
                self.notice = if self.error {
                    failures.join("; ")
                } else {
                    "Ready. Saved credentials loaded where available.".into()
                };
            }
            Update::Databases(result) => {
                self.busy = false;
                match result {
                    Ok(values) => {
                        self.notice =
                            format!("{} databases. Choose one for this session.", values.len());
                        self.picker = Some(Picker {
                            kind: Selection::Database,
                            values,
                            selected: 0,
                        });
                    }
                    Err(error) => self.set_result(Err(error)),
                }
            }
            Update::Profiles(result) => {
                self.busy = false;
                match result {
                    Ok(result) => {
                        self.notice = format!("{} profiles found.", result.profiles.len());
                        self.picker = Some(Picker {
                            kind: Selection::Profile(result.data_path),
                            values: result.profiles,
                            selected: 0,
                        });
                    }
                    Err(error) => self.set_result(Err(error)),
                }
            }
            Update::Config(settings, result) => {
                self.busy = false;
                if settings.artifact_path != self.settings.artifact_path
                    || settings.tx_data_path != self.settings.tx_data_path
                    || settings.profile != self.settings.profile
                {
                    self.error = false;
                    self.notice = "Configuration scope changed; stale reload ignored.".into();
                    return None;
                }
                match result {
                    Ok(files) => {
                        self.notice = if self.config.loaded(settings, files) {
                            "Configuration files loaded."
                        } else {
                            "Reload skipped: close the open editor first."
                        }
                        .into();
                        self.error = false;
                    }
                    Err(error) => self.set_result(Err(error)),
                }
            }
            Update::SavedConfig(result) => {
                self.busy = false;
                match result {
                    Ok(file) => {
                        self.config.saved(file);
                        self.notice = "Config saved.".into();
                        self.error = false;
                    }
                    Err(error) => self.set_result(Err(error)),
                }
            }
            Update::Action(result) => {
                self.busy = false;
                self.error = result.is_err();
                self.notice = if self.error {
                    "Action failed. Actions > Ctrl+O shows the result."
                } else {
                    "Complete. Actions > Ctrl+O shows the result."
                }
                .into();
                let pending = self.pending_credential_revision.take();
                let text = match result {
                    Ok(operations::Outcome::Login(_))
                        if !pending.is_some_and(|revision| {
                            revision.database == self.credential_revision.database
                        }) =>
                    {
                        self.notice = "Session changed; database login ignored. Actions > Ctrl+O shows the result.".into();
                        "The operation completed, but its returned database login was ignored because connection settings or credentials were edited while it ran.".into()
                    }
                    Ok(operations::Outcome::Login(credentials)) => {
                        self.settings.database_host = credentials.host;
                        self.settings.database_port = credentials.port;
                        self.settings.database_user = credentials.username;
                        self.settings.database = credentials.database.unwrap_or_default();
                        self.database_password = credentials.password;
                        self.credential_revision.database =
                            self.credential_revision.database.wrapping_add(1);
                        "Protected database login loaded. Password is not displayed.".into()
                    }
                    Ok(operations::Outcome::RconPassword(_))
                        if !pending.is_some_and(|revision| {
                            revision.rcon == self.credential_revision.rcon
                        }) =>
                    {
                        self.notice = "Session changed; RCON password ignored. Actions > Ctrl+O shows the result.".into();
                        "The operation completed, but its returned RCON password was ignored because the endpoint, profile or password was edited while it ran.".into()
                    }
                    Ok(operations::Outcome::RconPassword(password)) => {
                        self.rcon_password = password;
                        self.credential_revision.rcon =
                            self.credential_revision.rcon.wrapping_add(1);
                        "Protected RCON password loaded. Password is not displayed.".into()
                    }
                    Ok(operations::Outcome::Text(text)) => text,
                    Err(error) => error,
                };
                self.actions.show_result(safe_text(&text));
            }
            Update::Complete(result) => self.set_result(result),
            Update::Rcon(result) => {
                self.busy = false;
                self.error = result.is_err();
                let text = match result {
                    Ok(text) | Err(text) => text,
                };
                self.notice = if self.error {
                    "RCON failed. See Console."
                } else {
                    "RCON complete. See Console."
                }
                .into();
                self.console.rcon(&text, self.error);
            }
            Update::Resources(settings, result) => {
                if settings.artifact_path != self.settings.artifact_path
                    || settings.tx_data_path != self.settings.tx_data_path
                    || settings.profile != self.settings.profile
                {
                    self.set_result(Ok("Resource scope changed; stale scan ignored.".into()));
                } else {
                    match result {
                        Ok(resources) => {
                            self.resources.replace(resources);
                            self.set_result(Ok(format!("{} resources", self.resources.len())));
                        }
                        Err(error) => self.set_result(Err(error)),
                    }
                }
            }
            Update::Catalog(result) => match result {
                Ok(builds) => {
                    self.builds = builds;
                    self.selected = 0;
                    self.set_result(Ok(format!("{} available builds", self.builds.len())));
                }
                Err(error) => self.set_result(Err(error)),
            },
            Update::Sample(status, entries) => {
                if let Some(status) = &status {
                    self.performance.sample(status);
                }
                if status.is_some() {
                    self.status = status;
                }
                for entry in entries {
                    self.console.append(entry);
                }
            }
            Update::SqlFile(result) => {
                self.busy = false;
                match result {
                    Ok(sql) => self.require_confirmation(
                        format!(
                            "Execute this SQL file against {} / {}? Back up before changes.\n\n{}",
                            self.settings.database_host,
                            if self.settings.database.is_empty() {
                                "global scope"
                            } else {
                                &self.settings.database
                            },
                            sql
                        ),
                        Job::Query(self.credentials(), sql),
                    ),
                    Err(error) => self.set_result(Err(error)),
                }
            }
            Update::Stopped(result) => return Some(result),
        }
        None
    }

    fn update_affects_view(&self, update: &Update) -> bool {
        let Update::Sample(status, entries) = update else {
            return true;
        };
        if status.as_ref().is_some_and(|status| {
            self.status.as_ref().is_some_and(|old| old.running) != status.running
        }) {
            return true;
        }
        if self.picker.is_some() || self.confirm.is_some() {
            return false;
        }
        match self.tab {
            0 => status
                .as_ref()
                .is_some_and(|status| status.running || self.status.is_none()),
            // Paused history still shows incoming counts and retention changes.
            1 => !entries.is_empty(),
            _ => false,
        }
    }
    fn field(&self, index: usize) -> String {
        match index {
            0 => self.settings.artifact_path.clone(),
            1 => self.settings.tx_data_path.clone(),
            2 => self.settings.profile.clone(),
            3 => self.settings.rcon_host.clone(),
            4 => self.settings.rcon_port.to_string(),
            5 => self.settings.database_host.clone(),
            6 => self.settings.database_port.to_string(),
            7 => self.settings.database_user.clone(),
            8 => self.settings.database.clone(),
            9 => self.rcon_password.clone(),
            10 => self.database_password.clone(),
            _ => String::new(),
        }
    }
    fn set_field(&mut self, index: usize, value: String) -> Result<(), String> {
        let previous = self.field(index);
        let scope_changed = index <= 2 && value != previous;
        if scope_changed && self.server_transition {
            return Err(
                "Wait for server start, stop or restart before changing its paths or profile."
                    .into(),
            );
        }
        if scope_changed && self.config.dirty() {
            return Err("Save or discard unsaved config changes before changing the artifact folder or profile.".into());
        }
        if scope_changed && self.status.as_ref().is_some_and(|status| status.running) {
            return Err(
                "Stop the managed server before changing its artifact folder or profile.".into(),
            );
        }
        match index {
            0 => self.settings.artifact_path = value,
            1 => self.settings.tx_data_path = value,
            2 => self.settings.profile = value,
            3 => self.settings.rcon_host = value,
            4 => self.settings.rcon_port = parse_port(&value)?,
            5 => self.settings.database_host = value,
            6 => self.settings.database_port = parse_port(&value)?,
            7 => self.settings.database_user = value,
            8 => self.settings.database = value,
            9 => self.rcon_password = value,
            10 => self.database_password = value,
            _ => {}
        }
        self.invalidate_credential_result(index);
        if self.field(index) != previous {
            match index {
                0..=2 => {
                    self.database_password.clear();
                    self.rcon_password.clear();
                }
                3 | 4 => self.rcon_password.clear(),
                5..=7 => self.database_password.clear(),
                _ => {}
            }
        }
        if scope_changed {
            self.config = config::Panel::default();
            self.resources = resources::Panel::default();
        }
        self.notice = if index >= 9 {
            "Credential updated for this session only."
        } else {
            "Setting updated. Press S in Settings to save paths and connection details."
        }
        .into();
        Ok(())
    }

    fn invalidate_credential_result(&mut self, field: usize) {
        if matches!(field, 0..=2 | 5..=8 | 10) {
            self.credential_revision.database = self.credential_revision.database.wrapping_add(1);
        }
        if matches!(field, 0..=4 | 9) {
            self.credential_revision.rcon = self.credential_revision.rcon.wrapping_add(1);
        }
    }

    fn handle_event(&mut self, event: Event, sender: &SyncSender<Job>) -> bool {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key, sender),
            Event::Paste(value) => self.paste(&value),
            Event::Resize(_, _) => self.confirmation_visible = false,
            _ => return false,
        }
        true
    }

    fn key(&mut self, key: KeyEvent, sender: &SyncSender<Job>) {
        #[cfg(windows)]
        let key = normalize_windows_altgr(key);
        if self.quitting {
            return;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.request_quit(sender);
            return;
        }
        if self.confirm.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') if self.confirmation_visible => {
                    if let Some((_, job)) = self.confirm.take() {
                        self.submit(sender, job);
                    }
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => self.confirm = None,
                _ => {
                    if let Some((preview, _)) = &mut self.confirm {
                        preview.key(key.code);
                    }
                }
            }
            return;
        }
        if key.code == KeyCode::Char('k') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.picker = None;
            self.open_actions();
            return;
        }
        if let Some(picker) = &mut self.picker {
            match key.code {
                KeyCode::Esc => self.picker = None,
                KeyCode::Down => {
                    picker.selected =
                        (picker.selected + 1).min(picker.values.len().saturating_sub(1))
                }
                KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
                KeyCode::Enter => {
                    let picker = self.picker.take().expect("picker is open");
                    if let Some(value) = picker.values.get(picker.selected) {
                        match picker.kind {
                            Selection::Database => {
                                self.settings.database.clone_from(value);
                                self.invalidate_credential_result(8);
                                self.notice =
                                    "Database selected. Save Settings to remember the scope."
                                        .into();
                            }
                            Selection::Profile(path) => match self
                                .set_field(2, value.clone())
                                .and_then(|()| self.set_field(1, path))
                            {
                                Ok(()) => {}
                                Err(error) => {
                                    self.error = true;
                                    self.notice = error;
                                }
                            },
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if self.tab == 8 && !matches!(key.code, KeyCode::F(1..=9)) {
            match self.config.key(key) {
                config::Event::Load => self.submit(sender, Job::LoadConfig(self.settings.clone())),
                config::Event::Save {
                    settings,
                    path,
                    original,
                    content,
                } => self.submit(
                    sender,
                    Job::SaveConfig(
                        *settings,
                        SaveServerConfigRequest { path, content },
                        original,
                    ),
                ),
                config::Event::None => {}
            }
            return;
        }
        if self.tab == 7 && !matches!(key.code, KeyCode::F(1..=9)) {
            if let Some((action, submission)) = self.actions.key(key) {
                if self.busy {
                    self.actions.submission_rejected();
                    self.notice = "Wait for the current operation before starting another.".into();
                    return;
                }
                let title = actions::confirmation(&action, &submission);
                let job = Job::Action(Box::new((self.context(), submission)));
                if action.confirmation.is_some() {
                    self.require_confirmation(title, job);
                } else {
                    self.submit(sender, job);
                }
            }
            return;
        }
        if self.edit.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.edit = None;
                    self.input = Input::default();
                }
                KeyCode::Enter => {
                    let value = std::mem::take(&mut self.input.value);
                    self.input.cursor = 0;
                    match self.edit.take() {
                        Some(Edit::Console) if !value.trim().is_empty() => {
                            self.submit(sender, Job::Rcon(self.rcon(value)))
                        }
                        Some(Edit::Query) if !value.trim().is_empty() => self.require_confirmation(
                            format!(
                                "Execute this SQL against {} / {}?\n\n{}",
                                self.settings.database_host,
                                if self.settings.database.is_empty() {
                                    "global scope"
                                } else {
                                    &self.settings.database
                                },
                                value
                            ),
                            Job::Query(self.credentials(), value),
                        ),
                        Some(Edit::Setting(index)) => {
                            if let Err(error) = self.set_field(index, value) {
                                self.error = true;
                                self.notice = error;
                            }
                        }
                        Some(Edit::SqlFile) => {
                            self.submit(sender, Job::ReadSql(PathBuf::from(value)))
                        }
                        _ => {}
                    }
                }
                _ => self.input.key(key),
            }
            return;
        }
        if self.tab == 2 && !matches!(key.code, KeyCode::F(1..=9)) && self.resources.key(key) {
            return;
        }
        if self.tab == 1
            && !matches!(
                key.code,
                KeyCode::F(1..=9) | KeyCode::Tab | KeyCode::BackTab
            )
            && self.console.key(key)
        {
            return;
        }
        match key.code {
            KeyCode::Char('p') if self.tab == 6 => {
                self.submit(sender, Job::Profiles(self.settings.clone()))
            }
            KeyCode::Char(value @ '1'..='5') if self.tab == 0 => {
                self.performance.range = (value as u8 - b'1') as usize
            }
            KeyCode::Char('q') => self.request_quit(sender),
            KeyCode::Tab => {
                self.tab = (self.tab + 1) % TABS.len();
                self.selected = 0;
                self.scroll = 0;
                if self.tab == 7 {
                    self.open_actions();
                }
            }
            KeyCode::BackTab => {
                self.tab = (self.tab + TABS.len() - 1) % TABS.len();
                self.selected = 0;
                self.scroll = 0;
                if self.tab == 7 {
                    self.open_actions();
                }
            }
            KeyCode::F(8) => self.open_actions(),
            KeyCode::F(9) => {
                self.tab = 8;
                self.edit = None;
                self.input = Input::default();
            }
            KeyCode::F(n) if (1..=7).contains(&n) => {
                self.tab = n as usize - 1;
                self.selected = 0;
                self.scroll = 0;
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.row_count().saturating_sub(1))
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::PageUp => self.scroll = (self.scroll + 10).min(MAX_LINES),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::End => self.scroll = 0,
            KeyCode::Char('s') if self.tab == 0 => {
                self.submit(sender, Job::Start(self.settings.clone(), false))
            }
            KeyCode::Char('f') if self.tab == 0 => self.require_confirmation(
                "Force start despite preflight errors? Review Checks first.".into(),
                Job::Start(self.settings.clone(), true),
            ),
            KeyCode::Char('x') if self.tab == 0 => self.require_confirmation(
                "Stop the managed server? Connected players will disconnect.".into(),
                Job::Stop,
            ),
            KeyCode::Char('r') if self.tab == 0 => self.require_confirmation(
                "Restart the managed server? Connected players will disconnect.".into(),
                Job::Restart(self.settings.clone()),
            ),
            KeyCode::Char('r') if self.tab == 2 => {
                self.submit(sender, Job::Resources(self.settings.clone()))
            }
            KeyCode::Char('r') if self.tab == 4 => self.submit(sender, Job::Catalog),
            KeyCode::Char('r') if self.tab == 5 => {
                self.submit(sender, Job::Check(self.settings.clone()))
            }
            KeyCode::Char('d') if self.tab == 3 => {
                self.submit(sender, Job::Databases(self.credentials()))
            }
            KeyCode::Char('f') if self.tab == 3 => {
                self.edit = Some(Edit::SqlFile);
                self.input = Input::default();
            }
            KeyCode::Char('s') | KeyCode::Char('S') if self.tab == 6 => {
                self.submit(
                    sender,
                    Job::SaveSettings(self.config_path.clone(), self.settings.clone()),
                );
            }
            KeyCode::Enter if self.tab == 1 => {
                self.edit = Some(Edit::Console);
                self.input = Input::default();
            }
            KeyCode::Enter if self.tab == 3 => {
                self.edit = Some(Edit::Query);
                self.input = Input::default();
            }
            KeyCode::Enter if self.tab == 6 => {
                // Starting an edit expresses newer intent, even if it is later cleared or cancelled.
                self.invalidate_credential_result(self.selected);
                self.input = Input::new(self.field(self.selected));
                self.edit = Some(Edit::Setting(self.selected));
            }
            KeyCode::Enter if self.tab == 4 => {
                if let Some(build) = self.builds.get(self.selected).cloned() {
                    self.require_confirmation(format!("Install artifact {} ({}) into {}? Back up first. This acknowledges any health warnings:\n{}", build.version, build.health, self.settings.artifact_path, build.issues.iter().map(|i| i.reason.as_str()).collect::<Vec<_>>().join("\n")), Job::Install(build, self.settings.artifact_path.clone()));
                }
            }
            KeyCode::Char(key @ ('s' | 'x' | 'e' | 't')) if self.tab == 2 => {
                if let Some(resource) = self.resources.selected() {
                    let action = match key {
                        's' => "start",
                        'x' => "stop",
                        'e' => "ensure",
                        _ => "restart",
                    };
                    if !resource
                        .name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                    {
                        self.notice = "Resource name contains unsupported RCON characters.".into();
                        return;
                    }
                    self.require_confirmation(
                        format!("{action} resource {}?", resource.name),
                        Job::Rcon(self.rcon(format!("{action} {}", resource.name))),
                    );
                }
            }
            _ => {}
        }
    }
    fn row_count(&self) -> usize {
        match self.tab {
            4 => self.builds.len(),
            6 => FIELDS.len(),
            _ => 1,
        }
    }
    fn request_quit(&mut self, sender: &SyncSender<Job>) {
        if self.busy {
            self.notice = "Wait for the current operation to finish before quitting safely.".into();
            return;
        }
        if self.config.dirty() {
            self.require_confirmation(
                "Quit and discard unsaved config changes? The managed server will stop.".into(),
                Job::Quit,
            );
            return;
        }
        self.submit(sender, Job::Quit);
        if self.quitting {
            self.notice = "Stopping the managed server and closing".into();
        }
    }
}

#[cfg(windows)]
fn normalize_windows_altgr(mut key: KeyEvent) -> KeyEvent {
    // Windows reports AltGr text as Ctrl+Alt; normalize before any shortcut handler.
    if matches!(key.code, KeyCode::Char(character) if !character.is_control())
        && key
            .modifiers
            .contains(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        key.modifiers
            .remove(KeyModifiers::CONTROL | KeyModifiers::ALT);
    }
    key
}

fn parse_port(value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| "Port must be between 1 and 65535.".into())
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

fn redact_rcon_password(result: Result<String, String>, password: &str) -> Result<String, String> {
    let redact = |text: String| {
        if password.is_empty() {
            text
        } else {
            text.replace(password, "[redacted]")
        }
    };
    result.map(redact).map_err(redact)
}

fn finished_worker_result(app: &mut App, receive: &Receiver<Update>) -> Result<(), String> {
    // Called only after is_finished(): all final sends have completed. A missing
    // Stopped event remains a failure, including during an explicit quit.
    for update in receive.try_iter() {
        if let Some(result) = app.update(update) {
            return result;
        }
    }
    Err("The terminal worker stopped unexpectedly.".into())
}

pub(super) fn run(config: Settings, path: PathBuf, runtime: Runtime) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "The TUI requires an interactive terminal. Use --help for non-interactive commands."
                .into(),
        );
    }
    if std::env::var("TERM").as_deref() == Ok("dumb") {
        return Err("Use a terminal with cursor support, or choose a CLI subcommand.".into());
    }
    let shutdown = super::shutdown::Shutdown::listen(runtime.handle())?;
    let manager = FxserverManager::default();
    let mut terminal = ratatui::try_init().map_err(|e| e.to_string())?;
    let guard = TerminalGuard;
    crossterm::execute!(io::stdout(), EnableBracketedPaste).map_err(|e| e.to_string())?;
    let (send, jobs) = mpsc::sync_channel(1);
    let (updates, receive) = mpsc::sync_channel(16);
    let (progress, progress_events) = mpsc::sync_channel(32);
    let monitor_stop = Arc::new(AtomicBool::new(false));
    let monitor_manager = manager.clone();
    let monitor_updates = updates.clone();
    let monitor_stopping = monitor_stop.clone();
    let monitoring =
        thread::spawn(move || monitor(monitor_manager, monitor_updates, monitor_stopping));
    let worker_manager = manager.clone();
    let worker = thread::spawn(move || worker(worker_manager, runtime, jobs, updates));
    let mut app = App::new(config, path);
    app.manager = manager.clone();
    app.progress = Some(progress);
    app.submit(&send, Job::LoadCredentials(Box::new(app.context())));
    let mut dirty = true;
    let mut last_draw = Instant::now() - FRAME_INTERVAL;
    let result = (|| loop {
        if shutdown.requested() {
            return Ok(());
        }
        for message in progress_events.try_iter().take(32) {
            if app.busy {
                app.notice = message;
                dirty = true;
            }
        }
        for update in receive.try_iter().take(4) {
            dirty |= app.update_affects_view(&update);
            if let Some(result) = app.update(update) {
                return result;
            }
        }
        if (dirty && last_draw.elapsed() >= FRAME_INTERVAL)
            || (app.busy && last_draw.elapsed() >= Duration::from_millis(200))
        {
            terminal
                .draw(|frame| draw(frame, &mut app))
                .map_err(|e| e.to_string())?;
            dirty = false;
            last_draw = Instant::now();
        }
        let wait = if dirty {
            FRAME_INTERVAL.saturating_sub(last_draw.elapsed())
        } else {
            Duration::from_millis(50)
        };
        if event::poll(wait).map_err(|e| e.to_string())? {
            for _ in 0..64 {
                dirty |= app.handle_event(event::read().map_err(|e| e.to_string())?, &send);
                if !event::poll(Duration::ZERO).map_err(|e| e.to_string())? {
                    break;
                }
            }
        }
        if worker.is_finished() {
            return finished_worker_result(&mut app, &receive);
        }
    })();
    manager.begin_shutdown();
    monitor_stop.store(true, Ordering::Relaxed);
    drop(send);
    drop(receive);
    drop(guard);
    let stopped = worker
        .join()
        .map_err(|_| "The terminal worker failed during shutdown.")?;
    monitoring
        .join()
        .map_err(|_| "The terminal monitor failed during shutdown.")?;
    result.and(stopped)
}

fn tab_titles(width: u16, selected: usize) -> Vec<&'static str> {
    const SHORT: [&str; 9] = [
        "Srv", "Log", "Res", "DB", "Build", "Check", "Set", "Act", "Cfg",
    ];
    const TINY: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    let full_width = TABS.iter().map(|title| title.len()).sum::<usize>() + 3 * (TABS.len() - 1);
    let short_width = SHORT.iter().map(|title| title.len()).sum::<usize>() + 3 * (TABS.len() - 1)
        - SHORT[selected].len()
        + TABS[selected].len();
    TABS.iter()
        .enumerate()
        .map(|(index, title)| {
            if width as usize >= full_width || index == selected {
                *title
            } else if width as usize >= short_width {
                SHORT[index]
            } else {
                TINY[index]
            }
        })
        .collect()
}

fn footer_lines(tab: usize, width: u16) -> Vec<Line<'static>> {
    let actions = match tab {
        1 => "Enter: RCON  Space: pause/follow  End: follow",
        2 => "/: find R: scan S/X/T/E: start/stop/restart/ensure",
        3 => "Enter: SQL  D: databases  F: SQL file",
        4 => "R: refresh  Enter: install",
        5 => "R: run preflight",
        6 => "Enter: edit  S: save  P: choose profile",
        7 => "Type: search  Arrows: select  Enter: open",
        8 => "R: load  Enter: edit  Ctrl+S: save  Ctrl+Z: undo",
        _ => "S: start  X: stop  R: restart  1-5: time range",
    };
    const GLOBAL: &str = "F1-F9: view  Ctrl+K: actions  Ctrl+C: quit";
    let combined = format!("{GLOBAL}  {actions}");
    if combined.len() <= width as usize {
        vec![Line::raw(combined)]
    } else {
        vec![Line::raw(GLOBAL), Line::raw(actions)]
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    app.confirmation_visible = false;
    let area = frame.area();
    if area.width < 50 || area.height < 14 {
        frame.render_widget(
            Paragraph::new(
                "FXServer Installer\nIncrease the terminal to at least 50 columns and 14 rows.",
            ),
            area,
        );
        return;
    }
    let footer = footer_lines(app.tab, area.width);
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(if app.tab >= 7 { 1 } else { 3 }),
        Constraint::Length(footer.len() as u16),
    ])
    .split(area);
    let running = app.status.as_ref().is_some_and(|s| s.running);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" FXServer Installer ", Style::new().bold()),
            Span::styled(
                if running { " RUNNING " } else { " STOPPED " },
                if running {
                    Style::new().fg(Color::Green)
                } else {
                    Style::new().fg(Color::Red)
                },
            ),
            Span::raw(format!("  {}", safe_text(&app.settings.profile))),
        ])),
        layout[0],
    );
    frame.render_widget(
        Tabs::new(tab_titles(area.width, app.tab))
            .select(app.tab)
            .highlight_style(Style::new().fg(Color::Cyan).bold())
            .padding("", "")
            .divider(" | "),
        layout[1],
    );
    let mut block = Block::default().borders(Borders::ALL).title(TABS[app.tab]);
    if app.tab == 1 {
        block = block.title_bottom(app.console.footer());
    }
    let body = block.inner(layout[2]);
    frame.render_widget(block, layout[2]);
    match app.tab {
        0 => app
            .performance
            .draw(frame, body, app.status.as_ref(), &app.settings),
        1 => app.console.draw(frame, body),
        2 => app.resources.draw(frame, body),
        4 => {
            lists::draw(
                frame,
                body,
                app.builds.len(),
                app.selected,
                |index| {
                    let b = &app.builds[index];
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<8}", b.version), Style::new().bold()),
                        Span::styled(
                            safe_text(&b.health),
                            if !b.issues.is_empty() {
                                Style::new().fg(Color::Red)
                            } else if b.recommended {
                                Style::new().fg(Color::Green)
                            } else {
                                Style::new().fg(Color::Yellow)
                            },
                        ),
                    ]))
                },
                "No artifacts loaded. Refresh [R]",
            );
        }
        6 => {
            lists::draw(
                frame,
                body,
                FIELDS.len(),
                app.selected,
                |index| {
                    let name = FIELDS[index];
                    let value = if index >= 9 {
                        if app.field(index).is_empty() {
                            "not set".into()
                        } else {
                            "********".into()
                        }
                    } else {
                        safe_text(&app.field(index))
                    };
                    ListItem::new(format!("{name:<20} {value}"))
                },
                "",
            );
        }
        7 => app.actions.draw(frame, body),
        8 => app.config.draw(frame, body),
        _ => render_output(frame, body, app.output.iter(), app.scroll),
    }
    if let Some(edit) = &app.edit {
        let label = match edit {
            Edit::Console => "RCON command",
            Edit::Query => "SQL (review before execution)",
            Edit::Setting(i) => FIELDS[*i],
            Edit::SqlFile => "SQL file path",
        };
        let mut input = Block::default()
            .borders(Borders::ALL)
            .title(label)
            .border_style(Style::new().fg(Color::Cyan));
        if app.error {
            input = input
                .border_style(Style::new().fg(Color::Red))
                .title_bottom(app.notice.as_str());
        }
        let inner = input.inner(layout[3]);
        frame.render_widget(input, layout[3]);
        let (value, cursor) = app
            .input
            .visible(inner.width as usize, matches!(edit, Edit::Setting(9 | 10)));
        frame.render_widget(Paragraph::new(value), inner);
        frame.set_cursor_position((inner.x + cursor, inner.y));
    } else {
        let spinner = if app.busy {
            ["|", "/", "-", "\\"][(app.started.elapsed().as_millis() / 200) as usize % 4]
        } else {
            ""
        };
        frame.render_widget(
            Paragraph::new(format!("{spinner} {}", app.notice))
                .style(if app.error {
                    Style::new().fg(Color::Red)
                } else {
                    Style::new().fg(Color::Gray)
                })
                .wrap(Wrap { trim: false }),
            layout[3],
        );
    }
    frame.render_widget(Paragraph::new(footer).fg(Color::DarkGray), layout[4]);
    if let Some(picker) = &app.picker {
        let modal = Rect::new(
            3,
            3,
            area.width.saturating_sub(6),
            area.height.saturating_sub(6),
        );
        let title = match picker.kind {
            Selection::Database => "Choose database",
            Selection::Profile(_) => "Choose profile",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_bottom("Arrows: select  Enter: choose  Esc: cancel");
        let inner = block.inner(modal);
        frame.render_widget(ratatui::widgets::Clear, modal);
        frame.render_widget(block, modal);
        lists::draw(
            frame,
            inner,
            picker.values.len(),
            picker.selected,
            |index| ListItem::new(safe_text(&picker.values[index])),
            "No matches found.",
        );
    }
    if let Some((preview, _)) = &mut app.confirm {
        let width = area.width.saturating_sub(6);
        let height = area.height.saturating_sub(6);
        let modal = Rect::new(3, 3, width, height);
        preview.draw(frame, modal);
        app.confirmation_visible = true;
    }
}

fn render_output<'a>(
    frame: &mut Frame,
    area: Rect,
    lines: impl ExactSizeIterator<Item = &'a String>,
    scroll: usize,
) {
    let end = lines
        .len()
        .saturating_sub(scroll.min(lines.len().saturating_sub(1)));
    let start = end.saturating_sub(area.height as usize);
    let text = lines
        .skip(start)
        .take(end - start)
        .map(|line| line.chars().take(area.width as usize).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(
        Paragraph::new(if text.is_empty() {
            "No output yet.".into()
        } else {
            text
        }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn resource(name: &str) -> FxserverResourceInfo {
        FxserverResourceInfo {
            name: name.into(),
            path: format!("/resources/{name}"),
            manifest_path: String::new(),
            manifest_name: "fxmanifest.lua".into(),
            version: None,
            repository: None,
        }
    }

    fn sample(running: bool, with_log: bool) -> Update {
        Update::Sample(
            Some(FxserverStatus {
                running,
                pid: running.then_some(42),
                artifact_path: None,
                started_at: None,
                uptime_seconds: None,
                resources: None,
            }),
            if with_log {
                vec![FxserverTerminalEntry {
                    id: 1,
                    stream: "stdout".into(),
                    line: "test log".into(),
                    plain_line: "test log".into(),
                    segments: vec![],
                    timestamp: String::new(),
                }]
            } else {
                vec![]
            },
        )
    }

    fn console_screen(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    fn console_sample(text: &str) -> Update {
        let Update::Sample(_, mut entries) = sample(false, true) else {
            unreachable!()
        };
        entries[0].plain_line = text.into();
        Update::Sample(None, entries)
    }

    #[test]
    fn console_filter_and_rcon_inputs_do_not_share_text_or_shortcuts() {
        let (send, jobs) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 1;
        app.update(console_sample("keep row"));
        app.update(console_sample("other row"));
        app.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), &send);
        app.paste("keep");
        app.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE), &send);
        app.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE), &send);
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        assert!(app.edit.is_none());
        assert!(app.input.value.is_empty());
        assert!(jobs.try_recv().is_err());
        let screen = console_screen(&mut app, 80, 24).join("\n");
        assert!(screen.contains("1 matches"));
        assert!(screen.contains("keep row") && !screen.contains("other row"));

        app.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE), &send);
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        app.paste("status / command-secret");
        app.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE), &send);
        app.key(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
            &send,
        );
        app.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE), &send);
        assert_eq!(app.input.value, "status / command-secret ");
        assert!(matches!(app.edit, Some(Edit::Console)));
        assert!(console_screen(&mut app, 80, 24)
            .join("\n")
            .contains("Paused"));
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &send);
        assert!(jobs.try_recv().is_err());
        assert!(app.input.value.is_empty());
        let screen = console_screen(&mut app, 80, 24).join("\n");
        assert!(screen.contains("1 matches") && !screen.contains("command-secret"));
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &send);
        assert!(console_screen(&mut app, 80, 24)
            .join("\n")
            .contains("2 matches"));

        app.key(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
            &send,
        );
        app.paste("keep");
        app.key(
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            &send,
        );
        assert_eq!(app.tab, 7);
        app.key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE), &send);
        assert!(console_screen(&mut app, 80, 24)
            .join("\n")
            .contains("Filter (Enter/Esc: done)"));
        app.key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &send,
        );
        assert!(matches!(jobs.try_recv(), Ok(Job::Quit)));
    }

    #[test]
    fn console_anchor_survives_samples_rcon_tab_switches_and_other_view_scrolling() {
        let (send, jobs) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 1;
        for index in 0..60 {
            app.update(console_sample(&format!("history-{index:03}")));
        }
        console_screen(&mut app, 80, 24);
        app.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE), &send);
        for _ in 0..5 {
            app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &send);
        }
        let history = |app: &mut App| -> Vec<String> {
            console_screen(app, 80, 24)
                .into_iter()
                .filter(|line| line.contains("history-"))
                .collect()
        };
        let original = history(&mut app);
        assert!(original[0].contains("history-005"));
        for (away, back) in [
            (KeyCode::F(1), KeyCode::F(2)),
            (KeyCode::Tab, KeyCode::BackTab),
            (KeyCode::BackTab, KeyCode::Tab),
            (KeyCode::F(8), KeyCode::F(2)),
        ] {
            app.key(KeyEvent::new(away, KeyModifiers::NONE), &send);
            app.update(console_sample("arrived offscreen"));
            app.key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE), &send);
            app.key(KeyEvent::new(back, KeyModifiers::NONE), &send);
            assert_eq!(history(&mut app), original);
        }
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        app.paste("private-command");
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        assert!(matches!(jobs.try_recv(), Ok(Job::Rcon(_))));
        app.update(Update::Rcon(Ok("separate response".into())));
        app.update(console_sample("arrived after RCON"));
        assert_eq!(history(&mut app), original);
        assert!(!app
            .console
            .lines()
            .any(|line| line.to_string().contains("private-command")));
        for (width, height) in [(50, 14), (140, 40)] {
            let resized = console_screen(&mut app, width, height);
            assert!(resized
                .iter()
                .find(|line| line.contains("history-"))
                .unwrap()
                .contains("history-005"));
        }
    }

    #[test]
    fn paused_samples_update_counts_but_hidden_samples_and_modal_content_do_not_redraw() {
        let (send, _) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 1;
        app.update(sample(false, false));
        app.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE), &send);
        assert!(app.update_affects_view(&sample(false, true)));
        assert!(!app.update_affects_view(&sample(false, false)));
        app.update(sample(false, true));
        assert!(console_screen(&mut app, 50, 14)
            .join("\n")
            .contains("1 new"));
        app.tab = 2;
        assert!(!app.update_affects_view(&sample(false, true)));
        app.tab = 1;
        app.require_confirmation("Guarded operation".into(), Job::Stop);
        assert!(!app.update_affects_view(&sample(false, true)));
        assert!(app.update_affects_view(&sample(true, true)));
        assert!(app.update_affects_view(&Update::Rcon(Ok("reply".into()))));
    }

    #[test]
    fn console_retention_notice_and_log_row_fit_the_smallest_supported_view() {
        let (send, _) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 1;
        app.update(console_sample("old anchor"));
        console_screen(&mut app, 50, 14);
        app.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE), &send);
        for _ in 0..MAX_LINES + 5 {
            app.update(console_sample("retained row"));
        }
        let screen = console_screen(&mut app, 50, 14).join("\n");
        assert!(screen.contains("Paused rows expired; oldest retained shown."));
        assert!(screen.contains("retained row"));
        assert!(screen.contains("999+ new"));
        assert_eq!(app.console.lines().len(), MAX_LINES);
    }

    #[test]
    fn hidden_samples_are_collected_without_redrawing_unrelated_views() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.update(sample(true, false));
        for tab in 2..9 {
            app.tab = tab;
            for _ in 0..100 {
                let update = sample(true, true);
                assert!(!app.update_affects_view(&update));
                app.update(update);
            }
        }
        assert_eq!(app.console.lines().len(), 700);
        app.tab = 1;
        assert!(app.update_affects_view(&sample(true, true)));
        assert!(!app.update_affects_view(&sample(true, false)));
        app.tab = 0;
        assert!(app.update_affects_view(&sample(true, false)));
        app.tab = 8;
        assert!(
            app.update_affects_view(&sample(false, false)),
            "Running badge must update in every view"
        );
    }

    #[test]
    fn unchanged_stopped_status_does_not_repaint() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        assert!(app.update_affects_view(&sample(false, false)));
        app.update(sample(false, false));
        assert!(!app.update_affects_view(&sample(false, false)));
    }

    #[test]
    fn resource_scans_are_scoped_and_profile_changes_clear_the_list() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        let previous = app.settings.clone();
        app.update(Update::Resources(
            previous.clone(),
            Ok(vec![resource("alpha")]),
        ));
        assert_eq!(app.resources.len(), 1);
        app.set_field(2, "another-profile".into()).unwrap();
        assert_eq!(app.resources.len(), 0);
        app.busy = true;
        app.update(Update::Resources(previous, Ok(vec![resource("stale")])));
        assert_eq!(app.resources.len(), 0);
        assert!(!app.busy);
    }

    #[test]
    fn filtered_resource_controls_use_the_selected_match() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 2;
        app.resources
            .replace(vec![resource("alpha"), resource("second")]);
        let (send, receive) = mpsc::sync_channel(1);
        app.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), &send);
        app.paste("second");
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        app.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), &send);
        assert!(receive.try_recv().is_err());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        app.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), &send);
        let Job::Rcon(request) = receive.try_recv().unwrap() else {
            panic!("Expected RCON job")
        };
        assert_eq!(request.command, "stop second");
    }

    #[test]
    fn queued_approval_cannot_skip_rendering_the_confirmation() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        let (send, receive) = mpsc::sync_channel(1);
        for key in ['x', 'y'] {
            app.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE), &send);
        }
        assert!(app.confirm.is_some());
        assert!(receive.try_recv().is_err());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        app.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), &send);
        assert!(matches!(receive.try_recv(), Ok(Job::Stop)));
    }

    #[test]
    fn hidden_confirmation_cannot_be_approved_in_an_undersized_terminal() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        let (send, receive) = mpsc::sync_channel(1);
        app.require_confirmation("Stop?".into(), Job::Stop);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(app.confirmation_visible);
        app.handle_event(Event::Resize(20, 8), &send);
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
            &send,
        );
        assert!(app.confirm.is_some());
        assert!(receive.try_recv().is_err());
        let mut small = Terminal::new(TestBackend::new(20, 8)).unwrap();
        small.draw(|frame| draw(frame, &mut app)).unwrap();
        app.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), &send);
        assert!(app.confirm.is_some());
        assert!(receive.try_recv().is_err());
    }

    #[test]
    fn paste_under_a_picker_cannot_change_the_resource_filter() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 2;
        app.resources
            .replace(vec![resource("alpha"), resource("second")]);
        app.resources
            .key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.paste("alpha");
        app.picker = Some(Picker {
            kind: Selection::Database,
            values: vec!["database".into()],
            selected: 0,
        });
        app.paste("hidden");
        assert_eq!(app.resources.selected().unwrap().name, "alpha");
    }

    #[test]
    #[ignore = "manual rendering benchmark"]
    fn benchmark_large_resource_list() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 2;
        app.resources.replace(
            (0..20_000)
                .map(|index| FxserverResourceInfo {
                    name: format!("resource-{index:05}"),
                    path: format!("/server/resources/resource-{index:05}"),
                    manifest_path: String::new(),
                    manifest_name: "fxmanifest.lua".into(),
                    version: Some("1.0.0".into()),
                    repository: None,
                })
                .collect(),
        );
        app.resources
            .key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let start = Instant::now();
        for _ in 0..200 {
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        }
        eprintln!("20,000 resources, 200 frames: {:?}", start.elapsed());
    }

    fn queue_job(app: &mut App, job: Job) {
        let (send, jobs) = mpsc::sync_channel(1);
        app.submit(&send, job);
        assert!(jobs.try_recv().is_ok());
    }

    fn queue_restore(app: &mut App) -> Box<operations::Context> {
        let context = Box::new(app.context());
        queue_job(app, Job::LoadCredentials(context.clone()));
        context
    }

    fn queue_login(app: &mut App) {
        let context = app.context();
        queue_job(
            app,
            Job::Action(Box::new((
                context,
                operations::Submission {
                    id: "database.load-login".into(),
                    values: Default::default(),
                },
            ))),
        );
    }

    #[test]
    fn endpoint_and_profile_edits_clear_only_affected_session_secrets() {
        for (field, value, clear_database, clear_rcon) in [
            (0, "other-artifacts", true, true),
            (1, "other-txdata", true, true),
            (2, "other-profile", true, true),
            (3, "other-rcon", false, true),
            (4, "30121", false, true),
            (5, "other-db", true, false),
            (6, "3307", true, false),
            (7, "other-user", true, false),
            (8, "other-schema", false, false),
        ] {
            let mut app = App::new(Settings::default(), PathBuf::new());
            app.database_password = "database-secret".into();
            app.rcon_password = "rcon-secret".into();
            app.set_field(field, app.field(field)).unwrap();
            assert_eq!(app.database_password, "database-secret");
            assert_eq!(app.rcon_password, "rcon-secret");
            app.set_field(field, value.into()).unwrap();
            assert_eq!(
                app.database_password.is_empty(),
                clear_database,
                "field {field}"
            );
            assert_eq!(app.rcon_password.is_empty(), clear_rcon, "field {field}");
        }
    }

    #[test]
    fn invalid_ports_do_not_change_session_credentials() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.database_password = "database-secret".into();
        app.rcon_password = "rcon-secret".into();
        for field in [4, 6] {
            let original = app.field(field);
            for value in ["0", "65536", "not-a-port"] {
                assert!(app.set_field(field, value.into()).is_err());
                assert_eq!(app.field(field), original);
                assert_eq!(app.database_password, "database-secret");
                assert_eq!(app.rcon_password, "rcon-secret");
            }
        }
    }

    #[test]
    fn queued_start_keeps_its_profile_until_the_operation_finishes() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        let (send, receive) = mpsc::sync_channel(1);
        app.submit(&send, Job::Start(app.settings.clone(), false));
        assert!(matches!(receive.try_recv(), Ok(Job::Start(..))));
        assert!(app.set_field(2, "different-profile".into()).is_err());
        assert_ne!(app.settings.profile, "different-profile");
        app.update(Update::Complete(Err("fixture start failed".into())));
        assert!(app.set_field(2, "different-profile".into()).is_ok());
    }

    #[test]
    fn profile_picker_clears_credentials_when_only_txdata_path_changes() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.database_password = "database-secret".into();
        app.rcon_password = "rcon-secret".into();
        app.picker = Some(Picker {
            kind: Selection::Profile("other-txdata".into()),
            values: vec![app.settings.profile.clone()],
            selected: 0,
        });
        let (send, jobs) = mpsc::sync_channel(1);
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        assert_eq!(app.settings.tx_data_path, "other-txdata");
        assert!(app.database_password.is_empty() && app.rcon_password.is_empty());
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn startup_restore_rejects_change_and_revert_and_explicit_password_clear() {
        for (field, value) in [
            (0, "other-artifacts"),
            (1, "other-txdata"),
            (2, "other-profile"),
            (3, "other-rcon"),
            (4, "30121"),
            (5, "other-db"),
            (6, "3307"),
            (7, "other-user"),
            (8, "other-schema"),
            (9, ""),
            (10, ""),
        ] {
            let mut app = App::new(Settings::default(), PathBuf::new());
            let context = queue_restore(&mut app);
            let original = app.field(field);
            app.set_field(field, value.into()).unwrap();
            app.set_field(field, original).unwrap();
            let mut credentials = context.credentials.clone();
            credentials.password = "stored-database".into();
            app.update(Update::Credentials(
                context,
                Ok(Some(credentials)),
                Ok(Some("stored-rcon".into())),
            ));
            assert_eq!(
                app.database_password.is_empty(),
                matches!(field, 0..=2 | 5..=8 | 10),
                "field {field}"
            );
            assert_eq!(
                app.rcon_password.is_empty(),
                matches!(field, 0..=4 | 9),
                "field {field}"
            );
        }
    }

    #[test]
    fn starting_password_edit_prevents_startup_restore_even_before_commit() {
        for field in [9, 10] {
            let mut app = App::new(Settings::default(), PathBuf::new());
            let context = queue_restore(&mut app);
            app.tab = 6;
            app.selected = field;
            let (send, _) = mpsc::sync_channel(1);
            app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
            let mut credentials = context.credentials.clone();
            credentials.password = "stored-database".into();
            app.update(Update::Credentials(
                context,
                Ok(Some(credentials)),
                Ok(Some("stored-rcon".into())),
            ));
            assert_eq!(app.database_password.is_empty(), field == 10);
            assert_eq!(app.rcon_password.is_empty(), field == 9);
            assert!(app.input.value.is_empty());
        }
    }

    #[test]
    fn late_login_actions_do_not_overwrite_newer_connection_or_password_edits() {
        for (field, value) in [
            (0, "other-artifacts"),
            (1, "other-txdata"),
            (2, "other-profile"),
            (5, "other-db"),
            (6, "3307"),
            (7, "other-user"),
            (8, "other-schema"),
            (10, "new-password"),
        ] {
            let mut app = App::new(Settings::default(), PathBuf::new());
            let mut credentials = app.credentials();
            credentials.password = "stale-password".into();
            queue_login(&mut app);
            app.set_field(field, value.into()).unwrap();
            app.update(Update::Action(Ok(operations::Outcome::Login(credentials))));
            assert_eq!(app.field(field), value);
            assert_ne!(app.database_password, "stale-password");
            assert!(app.notice.contains("ignored"));
        }
        for (field, value) in [
            (0, "other-artifacts"),
            (1, "other-txdata"),
            (2, "other-profile"),
            (3, "other-rcon"),
            (4, "30121"),
            (9, "new-password"),
        ] {
            let mut app = App::new(Settings::default(), PathBuf::new());
            queue_login(&mut app);
            app.set_field(field, value.into()).unwrap();
            app.update(Update::Action(Ok(operations::Outcome::RconPassword(
                "stale-password".into(),
            ))));
            assert_eq!(app.field(field), value);
            assert_ne!(app.rcon_password, "stale-password");
            assert!(app.notice.contains("ignored"));
        }
    }

    #[test]
    fn credential_results_remain_masked_and_do_not_replace_later_user_input() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        let context = queue_restore(&mut app);
        app.set_field(10, "entered-during-load".into()).unwrap();
        let mut credentials = app.credentials();
        credentials.password = "older-stored-secret".into();
        app.update(Update::Credentials(
            context,
            Ok(Some(credentials.clone())),
            Ok(Some("stored-rcon".into())),
        ));
        assert_eq!(app.database_password, "entered-during-load");
        assert_eq!(app.rcon_password, "stored-rcon");
        queue_login(&mut app);
        app.update(Update::Action(Ok(operations::Outcome::Login(credentials))));
        assert_eq!(app.database_password, "older-stored-secret");
        assert_eq!(app.notice, "Complete. Actions > Ctrl+O shows the result.");
        queue_login(&mut app);
        app.set_field(8, "other-schema".into()).unwrap();
        app.update(Update::Action(Ok(operations::Outcome::RconPassword(
            "new-rcon-secret".into(),
        ))));
        assert_eq!(app.rcon_password, "new-rcon-secret");
        app.tab = 7;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(!text.contains("older-stored-secret") && !text.contains("new-rcon-secret"));
        app.update(Update::Action(Err("fixture failure".into())));
        assert_eq!(
            app.notice,
            "Action failed. Actions > Ctrl+O shows the result."
        );
    }

    fn open_config_fixture(app: &mut App) {
        let file = ServerConfigFile {
            name: "server.cfg".into(),
            path: "fixture.cfg".into(),
            content: "ensure fixture\n".into(),
            size: 15,
            modified: None,
            has_rcon_password: false,
            has_rconlog: false,
        };
        assert!(app.config.loaded(app.settings.clone(), vec![file]));
        app.config
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }

    #[test]
    fn config_scope_changes_reject_dirty_editor_and_preserve_unchanged_scope() {
        for field in 0..=2 {
            let mut app = App::new(Settings::default(), PathBuf::new());
            open_config_fixture(&mut app);
            app.config.paste("# unsaved\n");
            app.database_password = "database-secret".into();
            app.rcon_password = "rcon-secret".into();
            let original = app.field(field);
            assert!(app.set_field(field, "other-scope".into()).is_err());
            assert_eq!(app.field(field), original);
            assert!(app.config.dirty());
            assert_eq!(app.database_password, "database-secret");
            assert_eq!(app.rcon_password, "rcon-secret");
            app.set_field(field, original).unwrap();
            assert!(app.config.dirty());
        }
    }

    #[test]
    fn config_scope_changes_clear_cached_files_and_reject_old_load_responses() {
        for field in 0..=2 {
            let mut app = App::new(Settings::default(), PathBuf::new());
            open_config_fixture(&mut app);
            let old_settings = app.settings.clone();
            app.set_field(field, "other-scope".into()).unwrap();
            app.config
                .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            app.config.paste("would dirty an old editor");
            assert!(!app.config.dirty());
            app.update(Update::Config(old_settings, Ok(Vec::new())));
            assert!(app.notice.contains("stale reload ignored"));
            assert!(!app.busy);
        }
    }

    #[test]
    fn config_reload_reports_skipped_when_editor_is_already_open() {
        for dirty in [false, true] {
            let mut app = App::new(Settings::default(), PathBuf::new());
            open_config_fixture(&mut app);
            if dirty {
                app.config.paste("# unsaved\n");
            }
            app.update(Update::Config(app.settings.clone(), Ok(Vec::new())));
            assert_eq!(app.notice, "Reload skipped: close the open editor first.");
            assert_eq!(app.config.dirty(), dirty);
            let config::Event::Save { content, .. } = app
                .config
                .key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
            else {
                panic!("Reload removed the existing editor");
            };
            assert!(content.contains("ensure fixture"));
            assert_eq!(content.contains("# unsaved"), dirty);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_altgr_inserts_text_without_triggering_control_shortcuts() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 6;
        app.selected = 10;
        let (send, jobs) = mpsc::sync_channel(1);
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        for character in "@{}cuk".chars() {
            app.key(
                KeyEvent::new(
                    KeyCode::Char(character),
                    KeyModifiers::CONTROL | KeyModifiers::ALT,
                ),
                &send,
            );
        }
        assert_eq!(app.input.value, "@{}cuk");
        assert!(!app.quitting);
        assert_eq!(app.tab, 6);
        assert!(jobs.try_recv().is_err());
        let shifted = normalize_windows_altgr(KeyEvent::new(
            KeyCode::Char('{'),
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT,
        ));
        assert_eq!(shifted.modifiers, KeyModifiers::SHIFT);

        app.key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            &send,
        );
        assert!(app.input.value.is_empty());
        app.key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &send,
        );
        assert!(matches!(jobs.try_recv(), Ok(Job::Quit)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_altgr_preserves_config_editor_text_and_ctrl_save() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        open_config_fixture(&mut app);
        app.tab = 8;
        let (send, jobs) = mpsc::sync_channel(1);
        for character in "@{}".chars() {
            app.key(
                KeyEvent::new(
                    KeyCode::Char(character),
                    KeyModifiers::CONTROL | KeyModifiers::ALT,
                ),
                &send,
            );
        }
        assert!(app.config.dirty());
        app.key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &send,
        );
        let Ok(Job::SaveConfig(_, request, _)) = jobs.try_recv() else {
            panic!("Ctrl+S did not request config save");
        };
        assert!(request.content.contains("@{}"));
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn database_picker_selects_scope_without_another_connection_or_query() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        let (send, jobs) = mpsc::sync_channel(1);
        app.update(Update::Databases(Ok(vec!["first".into(), "second".into()])));
        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &send);
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        assert_eq!(app.settings.database, "second");
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn action_navigation_stays_available_while_a_job_is_running() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        let (send, jobs) = mpsc::sync_channel(1);
        app.submit(&send, Job::Catalog);
        app.key(
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            &send,
        );
        assert_eq!(app.tab, 7);
        app.paste("profile");
        app.key(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE), &send);
        assert_eq!(app.tab, 8);
        assert!(app.busy);
        assert!(matches!(jobs.try_recv(), Ok(Job::Catalog)));
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn paste_never_submits_commands_or_accepts_a_confirmation() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.paste("sxq");
        assert!(app.input.value.is_empty());
        assert!(!app.quitting);
        app.edit = Some(Edit::Query);
        app.paste("SELECT 'hello';");
        assert_eq!(app.input.value, "SELECT 'hello';");
        app.paste("\ny\n");
        assert_eq!(app.input.value, "SELECT 'hello';");
        assert!(app.confirm.is_none());
        assert!(!app.busy);
        app.edit = None;
        app.require_confirmation("Stop?".into(), Job::Stop);
        app.paste("y");
        assert!(app.confirm.is_some());
        assert!(!app.busy);
    }
    #[test]
    fn text_editing_uses_characters_and_visible_cell_width() {
        let mut input = Input::new("a界b".into());
        input.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        input.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(input.value, "ab");
        assert_eq!(input.cursor, 1);
        input.key(KeyEvent::new(KeyCode::Char('界'), KeyModifiers::NONE));
        assert_eq!(input.value, "a界b");
        assert!(input.visible(3, false).1 < 3);
        assert!(!input.visible(20, true).0.contains('界'));
    }
    #[test]
    fn all_views_render_at_supported_sizes_without_exposing_passwords() {
        for (width, height) in [(50, 14), (80, 24), (140, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(Settings::default(), PathBuf::from("unused.json"));
            app.rcon_password = "rcon-secret-fixture".into();
            app.database_password = "db-secret-fixture".into();
            for (tab, title) in TABS.iter().enumerate() {
                app.tab = tab;
                terminal.draw(|f| draw(f, &mut app)).unwrap();
                let content: String = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                assert!(!content.contains("secret-fixture"));
                assert!(content.contains(title));
            }
        }
    }
    #[test]
    fn busy_operations_do_not_block_navigation_or_queue_duplicates() {
        let (send, jobs) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.submit(&send, Job::Catalog);
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &send);
        assert_eq!(app.tab, 1);
        assert!(app.busy);
        app.submit(&send, Job::Catalog);
        assert!(jobs.try_recv().is_ok());
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn selected_tabs_and_footer_actions_fit_supported_terminal_sizes() {
        for (width, height) in [(50, 14), (80, 24), (140, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(Settings::default(), PathBuf::new());
            for tab in 0..TABS.len() {
                app.tab = tab;
                let titles = tab_titles(width, tab);
                assert_eq!(titles[tab], TABS[tab]);
                assert!(
                    titles.iter().map(|title| title.len()).sum::<usize>() + 3 * (titles.len() - 1)
                        <= width as usize
                );
                let footer = footer_lines(tab, width);
                assert!(
                    footer.iter().all(|line| line.width() <= width as usize),
                    "Footer {tab} exceeds {width} columns"
                );
                terminal.draw(|frame| draw(frame, &mut app)).unwrap();
                let buffer = terminal.backend().buffer();
                let tab_row: String = (0..width).map(|x| buffer[(x, 1)].symbol()).collect();
                assert!(tab_row.contains(TABS[tab]));
                for (index, line) in footer.iter().enumerate() {
                    let row = height - footer.len() as u16 + index as u16;
                    let actual: String = (0..width).map(|x| buffer[(x, row)].symbol()).collect();
                    assert!(
                        actual.contains(&line.to_string()),
                        "Missing footer at {width}x{height}: {line}"
                    );
                }
            }
        }
    }

    #[test]
    fn finished_worker_drains_final_stopped_after_the_initial_empty_poll() {
        let (send, receive) = mpsc::sync_channel(16);
        let (release, ready) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            ready.recv().unwrap();
            send.send(Update::Complete(Ok("final update".into())))
                .unwrap();
            send.send(Update::Stopped(Ok(()))).unwrap();
        });
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.quitting = true;
        assert!(receive.try_iter().next().is_none());
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(finished_worker_result(&mut app, &receive), Ok(()));
        assert_eq!(app.output, ["final update"]);
    }

    #[test]
    fn missing_stopped_and_failed_shutdown_remain_errors_while_quitting() {
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.quitting = true;
        let (send, receive) = mpsc::sync_channel(1);
        drop(send);
        assert!(finished_worker_result(&mut app, &receive)
            .unwrap_err()
            .contains("unexpectedly"));
        let (send, receive) = mpsc::sync_channel(1);
        send.send(Update::Stopped(Err("stop failed".into())))
            .unwrap();
        drop(send);
        assert_eq!(
            finished_worker_result(&mut app, &receive),
            Err("stop failed".into())
        );
    }

    #[test]
    fn sql_confirmation_navigation_never_changes_or_sends_the_job_before_approval() {
        let (send, receive) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        let sql = format!("{}SELECT '\t\\literal';\r\n", "SELECT 1;\n".repeat(100_000));
        app.update(Update::SqlFile(Ok(sql.clone())));
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        for key in [
            KeyCode::End,
            KeyCode::Home,
            KeyCode::PageDown,
            KeyCode::Down,
            KeyCode::PageUp,
        ] {
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            app.key(KeyEvent::new(key, KeyModifiers::NONE), &send);
            assert!(receive.try_recv().is_err());
        }
        app.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), &send);
        let Job::Query(_, submitted) = receive.try_recv().unwrap() else {
            panic!("Expected SQL job")
        };
        assert_eq!(submitted, sql);
        assert!(app.confirm.is_none());
    }

    #[test]
    fn rcon_responses_are_visible_bounded_and_do_not_echo_sent_commands() {
        let (send, receive) = mpsc::sync_channel(1);
        let mut app = App::new(Settings::default(), PathBuf::new());
        app.tab = 1;
        app.edit = Some(Edit::Console);
        app.rcon_password = "session-secret".into();
        app.input = Input::new("setr private_password command-secret".into());
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &send);
        let Job::Rcon(request) = receive.try_recv().unwrap() else {
            panic!("Expected RCON job")
        };
        assert!(request.command.contains("command-secret"));
        assert!(
            app.console.lines().len() == 0,
            "Do not echo submitted commands into console history"
        );
        app.output = vec!["previous SQL result".into()];
        app.scroll = 10;
        app.update(Update::Rcon(redact_rcon_password(
            Ok("online: 3\nreply session-secret\x1b".into()),
            &request.rcon.password,
        )));
        assert!(!app.busy && !app.error);
        assert_eq!(app.scroll, 10);
        assert_eq!(app.output, ["previous SQL result"]);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("[RCON] online: 3"));
        assert!(!content.contains("session-secret") && !content.contains("command-secret"));
        assert!(!app
            .console
            .lines()
            .any(|line| line.to_string().contains('\x1b')));
        app.update(Update::Rcon(Ok("line\n".repeat(MAX_LINES + 10))));
        assert_eq!(app.console.lines().len(), MAX_LINES);
        app.update(Update::Rcon(redact_rcon_password(
            Err("denied session-secret".into()),
            &request.rcon.password,
        )));
        assert!(app.error);
        assert_eq!(
            app.console.lines().next_back().unwrap().to_string(),
            "[RCON error] denied [redacted]"
        );
        assert_eq!(app.console.lines().len(), MAX_LINES);
    }
}
