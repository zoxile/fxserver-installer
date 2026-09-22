use crate::{
    models::mariadb::{
        MariaDBCredentials, MariaDBUser, MariaDBUserAccess, MariaDBUserConfig,
        MariaDBUserPrivilege, MariaDBUserUpdateConfig,
    },
    services::mariadb::permissions::{
        escape_identifier, escape_string, generated_sql, normalize_privileges,
    },
};

fn run_admin_query(credentials: MariaDBCredentials, sql: String) -> Result<(), String> {
    super::query::run_admin_query(credentials, generated_sql(&sql))
}

fn execute_generated_query(
    credentials: MariaDBCredentials,
    sql: String,
) -> Result<crate::models::mariadb::MariaDBQueryResult, String> {
    super::query::execute_query(credentials, generated_sql(&sql))
}

fn validate_account(username: &str, host: &str) -> Result<(), String> {
    if username.is_empty()
        || host.is_empty()
        || username.chars().any(char::is_control)
        || host.chars().any(char::is_control)
    {
        return Err("User and host must be nonempty and contain no control characters.".into());
    }
    Ok(())
}

fn validate_user_change(
    username: &str,
    host: &str,
    password: Option<&str>,
    database: Option<&str>,
    privileges: &[String],
) -> Result<(), String> {
    validate_account(username, host)?;
    if password.is_some_and(|p| p.chars().any(char::is_control)) {
        return Err("Password must not contain control characters.".into());
    }
    if let Some(database) = database.filter(|v| !v.trim().is_empty()) {
        escape_identifier(database)?;
        normalize_privileges(privileges.to_vec())?;
    } else if !privileges.is_empty() {
        normalize_privileges(privileges.to_vec())?;
    }
    Ok(())
}

pub fn list_users(credentials: MariaDBCredentials) -> Result<Vec<MariaDBUser>, String> {
    let query = "SELECT User, Host, plugin, password_expired FROM mysql.user ORDER BY User, Host;"
        .to_string();
    let result = execute_generated_query(credentials, query)?;

    if !result.success {
        return Err(if result.stderr.is_empty() {
            "MariaDB rejected the user list query.".to_string()
        } else {
            result.stderr
        });
    }

    Ok(result
        .rows
        .into_iter()
        .filter(|row| {
            row.first()
                .is_none_or(|username| !username.eq_ignore_ascii_case("PUBLIC"))
        })
        .map(|row| MariaDBUser {
            username: row.first().cloned().unwrap_or_default(),
            host: row.get(1).cloned().unwrap_or_default(),
            plugin: optional_cell(row.get(2)),
            password_expired: optional_cell(row.get(3)),
            locked: None,
        })
        .collect())
}

pub fn create_or_update_user(
    credentials: MariaDBCredentials,
    config: MariaDBUserConfig,
) -> Result<(), String> {
    validate_user_change(
        &config.username,
        &config.host,
        Some(&config.password),
        config.database.as_deref(),
        &config.privileges,
    )?;
    let user = account(&config.username, &config.host);
    let password = escape_string(&config.password);
    let authentication = if config.native_password {
        format!("IDENTIFIED VIA mysql_native_password USING PASSWORD({password})")
    } else {
        format!("IDENTIFIED BY {password}")
    };
    let create_user = format!("CREATE USER IF NOT EXISTS {user} {authentication};");
    run_password_query(credentials.clone(), create_user, &config.password)?;

    let alter_user = password_statement(&user, &password, config.native_password);
    run_password_query(credentials.clone(), alter_user, &config.password)?;

    if let Some(database) = config.database.filter(|value| !value.trim().is_empty()) {
        grant_permissions(
            credentials.clone(),
            config.username.clone(),
            config.host.clone(),
            database,
            config.privileges,
        )?;
    }

    run_admin_query(credentials, "FLUSH PRIVILEGES;".to_string())
}

pub fn update_user(
    credentials: MariaDBCredentials,
    config: MariaDBUserUpdateConfig,
) -> Result<(), String> {
    validate_user_change(
        &config.username,
        &config.host,
        config.password.as_deref(),
        config.database.as_deref(),
        &config.privileges,
    )?;
    if config.native_password
        && config
            .password
            .as_deref()
            .is_none_or(|password| password.trim().is_empty())
    {
        return Err(
            "A password is required to explicitly change authentication to mysql_native_password."
                .into(),
        );
    }
    let user = account(&config.username, &config.host);

    if let Some(password) = config.password.filter(|value| !value.trim().is_empty()) {
        let alter_user =
            password_statement(&user, &escape_string(&password), config.native_password);
        run_password_query(credentials.clone(), alter_user, &password)?;
    }

    if let Some(database) = config.database.filter(|value| !value.trim().is_empty()) {
        grant_permissions(
            credentials.clone(),
            config.username.clone(),
            config.host.clone(),
            database,
            config.privileges,
        )?;
    }

    run_admin_query(credentials, "FLUSH PRIVILEGES;".to_string())
}

fn password_statement(user: &str, password: &str, native_password: bool) -> String {
    if native_password {
        format!(
            "ALTER USER {user} IDENTIFIED VIA mysql_native_password USING PASSWORD({password});"
        )
    } else {
        // SET PASSWORD preserves the account's authentication plugin; unsupported plugins fail explicitly.
        format!("SET PASSWORD FOR {user} = PASSWORD({password});")
    }
}

fn run_password_query(
    credentials: MariaDBCredentials,
    sql: String,
    password: &str,
) -> Result<(), String> {
    run_admin_query(credentials, sql).map_err(|error| {
        if password.is_empty() {
            error
        } else {
            error
                .replace(&escape_string(password), "[redacted]")
                .replace(password, "[redacted]")
        }
    })
}

pub fn get_user_access(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
) -> Result<MariaDBUserAccess, String> {
    validate_account(&username, &host)?;
    let grants = get_grants(credentials.clone(), &username, &host)?;
    let grantee = grantee_literal(&username, &host);
    let schema_privileges = get_schema_privileges(credentials.clone(), &grantee)?;
    let table_privileges = get_table_privileges(credentials, &grantee)?;

    Ok(MariaDBUserAccess {
        username,
        host,
        grants,
        schema_privileges,
        table_privileges,
    })
}

pub fn grant_permissions(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
    database: String,
    privileges: Vec<String>,
) -> Result<(), String> {
    validate_account(&username, &host)?;
    let database = escape_identifier(&database)?;
    let privileges = normalize_privileges(privileges)?;
    let query = format!(
        "GRANT {privileges} ON {database}.* TO {};",
        account(&username, &host)
    );
    run_admin_query(credentials, query)
}

pub fn drop_user(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
) -> Result<(), String> {
    validate_account(&username, &host)?;
    let query = format!("DROP USER IF EXISTS {};", account(&username, &host));
    run_admin_query(credentials, query)
}

fn optional_cell(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && *value != "NULL")
        .map(str::to_string)
}

fn get_grants(
    credentials: MariaDBCredentials,
    username: &str,
    host: &str,
) -> Result<Vec<String>, String> {
    let result = execute_generated_query(
        credentials,
        format!("SHOW GRANTS FOR {};", account(username, host)),
    )?;

    if !result.success {
        return Err(if result.stderr.is_empty() {
            "MariaDB rejected the grants query.".to_string()
        } else {
            result.stderr
        });
    }

    Ok(result
        .rows
        .into_iter()
        .filter_map(|row| row.first().cloned())
        .collect())
}

fn get_schema_privileges(
    credentials: MariaDBCredentials,
    grantee: &str,
) -> Result<Vec<MariaDBUserPrivilege>, String> {
    let result = execute_generated_query(
        credentials,
        format!(
            "SELECT TABLE_SCHEMA, PRIVILEGE_TYPE, IS_GRANTABLE \
             FROM information_schema.SCHEMA_PRIVILEGES \
             WHERE GRANTEE = {} \
             ORDER BY TABLE_SCHEMA, PRIVILEGE_TYPE;",
            escape_string(grantee)
        ),
    )?;

    if !result.success {
        return Err(if result.stderr.is_empty() {
            "MariaDB rejected the schema privilege query.".to_string()
        } else {
            result.stderr
        });
    }

    Ok(result
        .rows
        .into_iter()
        .map(|row| MariaDBUserPrivilege {
            database: row.first().cloned().unwrap_or_default(),
            table: None,
            privilege: row.get(1).cloned().unwrap_or_default(),
            grantable: row.get(2).cloned().unwrap_or_default(),
        })
        .collect())
}

fn get_table_privileges(
    credentials: MariaDBCredentials,
    grantee: &str,
) -> Result<Vec<MariaDBUserPrivilege>, String> {
    let result = execute_generated_query(
        credentials,
        format!(
            "SELECT TABLE_SCHEMA, TABLE_NAME, PRIVILEGE_TYPE, IS_GRANTABLE \
             FROM information_schema.TABLE_PRIVILEGES \
             WHERE GRANTEE = {} \
             ORDER BY TABLE_SCHEMA, TABLE_NAME, PRIVILEGE_TYPE;",
            escape_string(grantee)
        ),
    )?;

    if !result.success {
        return Err(if result.stderr.is_empty() {
            "MariaDB rejected the table privilege query.".to_string()
        } else {
            result.stderr
        });
    }

    Ok(result
        .rows
        .into_iter()
        .map(|row| MariaDBUserPrivilege {
            database: row.first().cloned().unwrap_or_default(),
            table: row.get(1).cloned(),
            privilege: row.get(2).cloned().unwrap_or_default(),
            grantable: row.get(3).cloned().unwrap_or_default(),
        })
        .collect())
}

fn grantee_literal(username: &str, host: &str) -> String {
    format!(
        "'{}'@'{}'",
        username.replace('\'', "''"),
        host.replace('\'', "''")
    )
}

fn account(username: &str, host: &str) -> String {
    format!("{}@{}", escape_string(username), escape_string(host))
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn authentication_changes_require_explicit_opt_in() {
        let user = account("game'user", "localhost");
        let password = escape_string("secret'password");
        let normal = password_statement(&user, &password, false);
        assert!(normal.starts_with("SET PASSWORD FOR "));
        assert!(!normal.contains("IDENTIFIED"));
        let compatible = password_statement(&user, &password, true);
        assert!(compatible.contains("IDENTIFIED VIA mysql_native_password"));
        assert!(compatible.contains(&password));
    }

    #[test]
    fn malicious_privileges_are_rejected_before_any_account_change() {
        let privileges = vec!["SELECT ON *.* TO 'injected'@'%'; --".into()];
        assert!(validate_user_change(
            "fixture",
            "localhost",
            Some("secret"),
            Some("game"),
            &privileges
        )
        .is_err());
        assert!(
            validate_user_change("fixture", "localhost", Some("secret"), None, &privileges)
                .is_err()
        );
        assert!(validate_user_change("fixture\0", "localhost", Some("secret"), None, &[]).is_err());
        assert!(validate_user_change(
            "fixture",
            "localhost",
            Some("secret\nsource malicious.sql"),
            None,
            &[]
        )
        .is_err());
    }
}
