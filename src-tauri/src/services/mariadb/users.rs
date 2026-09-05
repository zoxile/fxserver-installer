use crate::{
    models::mariadb::{
        MariaDBCredentials, MariaDBUser, MariaDBUserAccess, MariaDBUserConfig,
        MariaDBUserPrivilege, MariaDBUserUpdateConfig,
    },
    services::mariadb::{
        permissions::{escape_identifier, escape_string, normalize_privileges},
        query::run_admin_query,
    },
};

pub fn list_users(credentials: MariaDBCredentials) -> Result<Vec<MariaDBUser>, String> {
    let query = "SELECT User, Host, plugin, password_expired FROM mysql.user ORDER BY User, Host;"
        .to_string();
    let result = crate::services::mariadb::query::execute_query(credentials, query)?;

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
    let user = account(&config.username, &config.host);
    let password = escape_string(&config.password);
    let create_user = format!("CREATE USER IF NOT EXISTS {user} IDENTIFIED BY {password};");
    run_admin_query(credentials.clone(), create_user)?;

    let alter_user = format!("ALTER USER {user} IDENTIFIED BY {password};");
    run_admin_query(credentials.clone(), alter_user)?;

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
    let user = account(&config.username, &config.host);

    if let Some(password) = config.password.filter(|value| !value.trim().is_empty()) {
        let alter_user = format!(
            "ALTER USER {user} IDENTIFIED BY {};",
            escape_string(&password)
        );
        run_admin_query(credentials.clone(), alter_user)?;
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

pub fn get_user_access(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
) -> Result<MariaDBUserAccess, String> {
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
    let database = escape_identifier(&database)?;
    let privileges = normalize_privileges(privileges);
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
    let result = crate::services::mariadb::query::execute_query(
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
    let result = crate::services::mariadb::query::execute_query(
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
    let result = crate::services::mariadb::query::execute_query(
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
