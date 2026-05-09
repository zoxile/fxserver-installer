use crate::{
    models::mariadb::{
        MariaDBCredentials, MariaDBUser, MariaDBUserConfig, MariaDBUserUpdateConfig,
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

fn account(username: &str, host: &str) -> String {
    format!("{}@{}", escape_string(username), escape_string(host))
}
