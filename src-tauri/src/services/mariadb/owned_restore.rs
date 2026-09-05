use crate::{
    commands::{
        backup_manager::storage,
        database_browser::{query_json, quote_identifier, sql_text},
    },
    models::mariadb::MariaDBCredentials,
    process::CommandNoWindowExt,
    services::mariadb::{
        backup::run_backup_client,
        query::{apply_credentials_args, find_mariadb_client},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    process::{Command, Stdio},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OwnedDatabasePurpose {
    RestoreTest,
    Clone,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedDatabase {
    pub id: String,
    pub database: String,
    pub host: String,
    pub port: u16,
    pub purpose: OwnedDatabasePurpose,
    pub marker_token: String,
}

impl OwnedDatabase {
    pub fn new(
        credentials: &MariaDBCredentials,
        purpose: OwnedDatabasePurpose,
    ) -> Result<Self, String> {
        let id = storage::secure_token()?;
        let database = format!(
            "{}{id}",
            match purpose {
                OwnedDatabasePurpose::RestoreTest => "fxsi_restore_test_",
                OwnedDatabasePurpose::Clone => "fxsi_clone_",
            }
        );
        Ok(Self {
            id,
            database,
            host: credentials.host.clone(),
            port: credentials.port,
            purpose,
            marker_token: storage::secure_token()?,
        })
    }

    pub fn validate(&self, credentials: &MariaDBCredentials) -> Result<(), String> {
        let prefix = match self.purpose {
            OwnedDatabasePurpose::RestoreTest => "fxsi_restore_test_",
            OwnedDatabasePurpose::Clone => "fxsi_clone_",
        };
        if self.id.len() != 32
            || !self.id.bytes().all(|b| b.is_ascii_hexdigit())
            || self.database != format!("{prefix}{}", self.id)
            || self.host != credentials.host
            || self.port != credentials.port
            || self.marker_token.len() != 32
            || !self
                .marker_token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.marker_token == self.id
        {
            return Err("Restore target must be an app-owned unique database on the confirmed host and port.".into());
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedImportResult {
    pub created: bool,
    pub tables_verified: Vec<String>,
    pub error: Option<String>,
}

// Callers hold maintenance_access inside run_blocking and persist the target before importing.
pub fn import_dump(
    credentials: &MariaDBCredentials,
    file: &mut File,
    expected_sha256: &str,
    target: &OwnedDatabase,
) -> OwnedImportResult {
    let mut result = OwnedImportResult {
        created: false,
        tables_verified: vec![],
        error: None,
    };
    let work = (|| {
        target.validate(credentials)?;
        if storage::sha256(file)? != expected_sha256 {
            return Err("Dump checksum changed. Import refused before SQL execution.".into());
        }
        let plan = storage::constrained_dump(file)?;
        let database = quote_identifier(&target.database)?;
        let _: Vec<serde_json::Value> = query_json(
            credentials,
            &format!("CREATE DATABASE {database} CHARACTER SET utf8mb4;"),
        )?;
        result.created = true;
        let _: Vec<serde_json::Value> = query_json(credentials, &format!("CREATE TABLE {database}.`__fx_restore_owner` (`owner_id` varchar(32) NOT NULL PRIMARY KEY) ENGINE=InnoDB; INSERT INTO {database}.`__fx_restore_owner` VALUES ({});", sql_text(&target.marker_token)))?;
        let client = find_mariadb_client().ok_or("MariaDB client is unavailable.")?;
        let mut command = Command::new(client);
        command.no_window().arg("--no-defaults");
        apply_credentials_args(&mut command, credentials);
        command.args(["--binary-mode", "--skip-reconnect", "--local-infile=0", "--connect-timeout=10", "--default-character-set=utf8mb4", "--init-command=SET SESSION sql_mode='NO_AUTO_VALUE_ON_ZERO', max_statement_time=60"])
            .arg(format!("--database={}", target.database)).stdin(Stdio::from(file.try_clone().map_err(|e| e.to_string())?));
        run_backup_client(&mut command, "Owned database restore")?;
        let restored: Vec<String> = query_json(credentials, &format!("SELECT JSON_QUOTE(TABLE_NAME) FROM information_schema.TABLES WHERE TABLE_SCHEMA={} AND TABLE_TYPE='BASE TABLE' AND TABLE_NAME <> '__fx_restore_owner';", sql_text(&target.database)))?;
        let mut inventory: Vec<_> = restored
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        inventory.sort();
        if inventory != plan.tables {
            return Err("Restored table inventory does not match the preflight plan.".into());
        }
        for table in restored {
            let _: Vec<u8> = query_json(credentials, &format!("SET SESSION max_statement_time=10; SELECT JSON_EXTRACT('1','$') FROM {database}.{} LIMIT 1;", quote_identifier(&table)?))?;
            result.tables_verified.push(table);
        }
        Ok(())
    })();
    result.error = work.err().map(|error: String| {
        if credentials.password.is_empty() {
            error
        } else {
            error.replace(&credentials.password, "[redacted]")
        }
    });
    result
}

pub fn cleanup_owned_database(
    credentials: &MariaDBCredentials,
    target: &OwnedDatabase,
    confirmation: &str,
) -> Result<(), String> {
    target.validate(credentials)?;
    if confirmation != target.database {
        return Err("Confirm the exact owned database name before cleanup.".into());
    }
    let database = quote_identifier(&target.database)?;
    let owner: Vec<String> = query_json(
        credentials,
        &format!("SELECT JSON_QUOTE(owner_id) FROM {database}.`__fx_restore_owner` LIMIT 2;"),
    )?;
    if owner != [target.marker_token.clone()] {
        return Err("Ownership marker mismatch. No database was dropped.".into());
    }
    let _: Vec<serde_json::Value> = query_json(credentials, &format!("DROP DATABASE {database};"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn credentials() -> MariaDBCredentials {
        MariaDBCredentials {
            host: "fixture.invalid".into(),
            port: 3306,
            username: "fixture".into(),
            password: String::new(),
            database: None,
        }
    }
    #[test]
    fn only_exact_owned_prefixes_and_endpoints_are_accepted() {
        let credentials = credentials();
        let id = "0123456789abcdef0123456789abcdef";
        let mut target = OwnedDatabase {
            id: id.into(),
            database: format!("fxsi_clone_{id}"),
            host: credentials.host.clone(),
            port: 3306,
            purpose: OwnedDatabasePurpose::Clone,
            marker_token: "fedcba9876543210fedcba9876543210".into(),
        };
        assert!(target.validate(&credentials).is_ok());
        target.database = "production".into();
        assert!(target.validate(&credentials).is_err());
        target.database = format!("fxsi_restore_test_{id}");
        assert!(target.validate(&credentials).is_err());
        target.purpose = OwnedDatabasePurpose::RestoreTest;
        assert!(target.validate(&credentials).is_ok());
        target.port = 3307;
        assert!(target.validate(&credentials).is_err());
        target.port = 3306;
        assert!(cleanup_owned_database(&credentials, &target, "production").is_err());
        target.marker_token = target.id.clone();
        assert!(target.validate(&credentials).is_err());
    }

    #[test]
    fn bad_scope_and_checksum_return_before_database_access() {
        let credentials = credentials();
        let id = "0123456789abcdef0123456789abcdef";
        let mut target = OwnedDatabase {
            id: id.into(),
            database: "production".into(),
            host: credentials.host.clone(),
            port: 3306,
            purpose: OwnedDatabasePurpose::Clone,
            marker_token: "fedcba9876543210fedcba9876543210".into(),
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/restore-safe.sql");
        let mut file = storage::open_snapshot(&path).unwrap();
        let result = import_dump(&credentials, &mut file, "wrong", &target);
        assert!(!result.created && result.error.is_some());
        target.database = format!("fxsi_clone_{id}");
        let result = import_dump(&credentials, &mut file, "wrong", &target);
        assert!(!result.created && result.error.unwrap().contains("checksum"));
    }
}
