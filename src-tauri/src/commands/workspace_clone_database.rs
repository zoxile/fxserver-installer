use super::*;
use crate::{
    commands::backup_manager::{
        owned_restore::{self, OwnedDatabase, OwnedDatabasePurpose},
        storage,
    },
    models::mariadb::MariaDBCredentials,
};

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatabaseSelection {
    pub dump_path: String,
    pub source_database: String,
    pub host: String,
    pub port: u16,
    pub username: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatabasePackage {
    pub source_database: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabasePreview {
    pub source_path: String,
    pub source_database: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub table_count: usize,
    #[serde(serialize_with = "serialize_public_target")]
    pub target: Option<OwnedDatabase>,
}

fn serialize_public_target<S: serde::Serializer>(
    target: &Option<OwnedDatabase>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct PublicTarget<'a> {
        database: &'a str,
        host: &'a str,
        port: u16,
    }
    target
        .as_ref()
        .map(|target| PublicTarget {
            database: &target.database,
            host: &target.host,
            port: target.port,
        })
        .serialize(serializer)
}

#[derive(Clone, PartialEq, Eq)]
pub struct DatabasePlan {
    pub source: PathBuf,
    pub package: DatabasePackage,
    pub table_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseDefaults {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub database: String,
}

pub fn prepare(
    request: &CloneRequest,
    package: Option<&DatabasePackage>,
) -> Result<Option<DatabasePlan>, String> {
    let Some(selection) = &request.database else {
        return Ok(None);
    };
    let (path, source_database) = if request.mode == CloneMode::Import {
        let package = package.ok_or("This package does not include a database dump.")?;
        (
            Path::new(&request.source_path).join("database.sql"),
            package.source_database.clone(),
        )
    } else {
        (
            PathBuf::from(&selection.dump_path),
            selection.source_database.clone(),
        )
    };
    storage::validate_database(&source_database)?;
    if request.mode != CloneMode::Export
        && (selection.port == 0
            || selection.host.is_empty()
            || selection.host.len() > 253
            || selection
                .host
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || ".:-_".contains(c)))
            || selection.username.is_empty()
            || selection.username.len() > 80)
    {
        return Err("Enter a valid target database host, port, and username.".into());
    }
    validate_absolute(&path)?;
    let _handles = pin_directories(path.parent().ok_or("Missing dump parent.")?)?;
    let bytes = read_bounded(&path, 32 * 1024 * 1024)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| "Database clone requires a UTF-8 SQL dump.")?;
    if sensitive_text(text) {
        return Err("The selected SQL dump contains potential credentials or secret fields. Database copy refused; create a reviewed secret-free dump. SQL is never rewritten.".into());
    }
    let mut file = storage::open_snapshot(&path)?;
    check_metadata(&file.metadata().map_err(io_error)?)?;
    let sha256 = storage::sha256(&mut file)?;
    if sha256 != digest(&bytes) {
        return Err("Database dump changed during preview.".into());
    }
    let guard = storage::constrained_dump(&mut file)?;
    let metadata = DatabasePackage {
        source_database,
        size_bytes: bytes.len() as u64,
        sha256,
    };
    if package.is_some_and(|expected| expected != &metadata) {
        return Err("Database dump does not match its package manifest.".into());
    }
    Ok(Some(DatabasePlan {
        source: path.canonicalize().map_err(io_error)?,
        package: metadata,
        table_count: guard.tables.len(),
    }))
}

pub fn preview(plan: &DatabasePlan, request: &CloneRequest) -> Result<DatabasePreview, String> {
    let selection = request
        .database
        .as_ref()
        .ok_or("Missing database selection.")?;
    let target = if request.mode == CloneMode::Export {
        None
    } else {
        let target = OwnedDatabase::new(
            &MariaDBCredentials {
                host: selection.host.clone(),
                port: selection.port,
                username: selection.username.clone(),
                password: String::new(),
                database: None,
            },
            OwnedDatabasePurpose::Clone,
        )?;
        if target
            .database
            .eq_ignore_ascii_case(&plan.package.source_database)
        {
            return Err("The clone database must differ from the source database.".into());
        }
        Some(target)
    };
    Ok(DatabasePreview {
        source_path: display(&plan.source),
        source_database: plan.package.source_database.clone(),
        size_bytes: plan.package.size_bytes,
        sha256: plan.package.sha256.clone(),
        table_count: plan.table_count,
        target,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Evidence<'a> {
    target: &'a OwnedDatabase,
    source_database: &'a str,
    sha256: &'a str,
    status: &'a str,
    created: bool,
}

pub struct DatabaseRun {
    target: OwnedDatabase,
    plan: DatabasePlan,
    credentials: MariaDBCredentials,
    evidence: PathBuf,
    created: bool,
}

impl DatabaseRun {
    pub fn new(
        plan: &DatabasePlan,
        preview: &DatabasePreview,
        credentials: MariaDBCredentials,
        confirmation: &str,
        selection: &DatabaseSelection,
        evidence_root: &Path,
    ) -> Result<Self, String> {
        let target = preview
            .target
            .clone()
            .ok_or("Missing owned database target.")?;
        target.validate(&credentials)?;
        if confirmation != target.database
            || credentials.username != selection.username
            || credentials
                .database
                .as_deref()
                .is_some_and(|name| !name.is_empty() && name != target.database)
        {
            return Err("Confirm the exact new clone database and target credentials. Source database credentials must not be used as the target database selection.".into());
        }
        check_no_links(evidence_root)?;
        let evidence = evidence_root.join(format!("{}-planned.json", target.id));
        let run = Self {
            target,
            plan: plan.clone(),
            credentials,
            evidence,
            created: false,
        };
        run.record("planned")?;
        Ok(run)
    }

    fn record(&self, status: &str) -> Result<(), String> {
        let path = self
            .evidence
            .parent()
            .ok_or("Missing evidence parent.")?
            .join(format!("{}-{status}.json", self.target.id));
        write_new(
            &path,
            &serde_json::to_vec_pretty(&Evidence {
                target: &self.target,
                source_database: &self.plan.package.source_database,
                sha256: &self.plan.package.sha256,
                status,
                created: self.created,
            })
            .map_err(io_error)?,
        )
    }

    pub fn import(&mut self) -> Result<(), String> {
        let _handles = pin_directories(
            self.plan
                .source
                .parent()
                .ok_or("Missing database dump parent.")?,
        )?;
        check_no_links(&self.plan.source)?;
        let mut file = storage::open_snapshot(&self.plan.source)?;
        check_metadata(&file.metadata().map_err(io_error)?)?;
        let outcome = owned_restore::import_dump(
            &self.credentials,
            &mut file,
            &self.plan.package.sha256,
            &self.target,
        );
        self.created = outcome.created;
        self.record(if outcome.error.is_some() {
            "import-failed"
        } else {
            "imported"
        })?;
        if outcome.error.is_some() {
            return Err(format!("Guarded database import failed for {}. No filesystem destination was promoted. Evidence: {}", self.target.database, display(&self.evidence)));
        }
        Ok(())
    }

    pub fn finish(&self, success: bool) -> Result<(), String> {
        if success {
            // The imported record already survives a crash; final evidence is best effort after promotion.
            let _ = self.record("complete");
            return Ok(());
        }
        if !self.created {
            return Ok(());
        }
        if owned_restore::cleanup_owned_database(
            &self.credentials,
            &self.target,
            &self.target.database,
        )
        .is_err()
        {
            let _ = self.record("cleanup-required");
            return Err(format!("Clone failed. Cleanup could not verify or remove the newly owned database {} on {}:{}. No existing database was dropped. Keep the ownership evidence for manual review: {}", self.target.database, self.target.host, self.target.port, display(&self.evidence)));
        }
        self.record("cleaned")
    }

    pub fn defaults(&self) -> DatabaseDefaults {
        DatabaseDefaults {
            host: self.target.host.clone(),
            port: self.target.port,
            username: self.credentials.username.clone(),
            database: self.target.database.clone(),
        }
    }
}
