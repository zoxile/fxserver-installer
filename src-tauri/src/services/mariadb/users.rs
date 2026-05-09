use crate::{
    models::mariadb::{MariaDBCredentials, MariaDBUserConfig},
    services::mariadb::{
        permissions::{escape_identifier, escape_string, normalize_privileges},
        query::run_admin_query,
    },
};

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

fn account(username: &str, host: &str) -> String {
    format!("{}@{}", escape_string(username), escape_string(host))
}
