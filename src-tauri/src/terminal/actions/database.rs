//! Database forms only build requests; shared commands/services own SQL and leases.
use super::{json, Action, Context, Field, FieldKind, Outcome, Submission};
use crate::{
    commands::{
        self, database_admin as admin, database_browser as browser, database_credentials as logins,
        database_inspection as inspection, sql_diagnostics,
    },
    models::mariadb::{
        MariaDBBackupOptions, MariaDBCredentials, MariaDBInstallOptions, MariaDBUserUpdateConfig,
    },
    services::mariadb::{
        access, backup, backup_storage, detect, install, permissions, query, service, users,
    },
    terminal::settings::Settings,
};
use sha2::{Digest, Sha256};

const YES_NO: &[&str] = &["no", "yes"];
const OPERATORS: &[&str] = &[
    "eq",
    "ne",
    "lt",
    "lte",
    "gt",
    "gte",
    "contains",
    "isNull",
    "isNotNull",
];

fn action(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    fields: Vec<Field>,
    confirmation: Option<&'static str>,
) -> Action {
    Action {
        id,
        group: "Database",
        title,
        description,
        fields,
        confirmation,
    }
}

fn choice(key: &'static str, label: &'static str, value: &str, choices: &[&str]) -> Field {
    Field::text(key, label, value).choice(choices)
}

fn database_field(context: &Context) -> Field {
    Field::text(
        "database",
        "Database",
        context
            .credentials
            .database
            .as_deref()
            .unwrap_or(&context.settings.database),
    )
}

fn table_fields(context: &Context) -> Vec<Field> {
    vec![
        database_field(context),
        Field::text("table", "Table (exact name)", ""),
    ]
}

fn account_fields() -> Vec<Field> {
    vec![
        Field::text("username", "Account name", ""),
        Field::text("host", "Account host", "localhost"),
    ]
}

fn password_fields() -> Vec<Field> {
    vec![
        Field::text("new_password", "New password", "").secret(),
        Field::text("repeat_password", "Repeat new password", "").secret(),
        choice(
            "native_password",
            "Explicit mysql_native_password compatibility",
            "no",
            YES_NO,
        ),
    ]
}

fn row_fields(context: &Context) -> Vec<Field> {
    let mut fields = table_fields(context);
    fields.extend([
        Field::text("page_size", "Rows per page (1-200)", "25"),
        Field::text("offset", "Row offset (0-1000000)", "0"),
        Field::text(
            "sort_column",
            "Sort column (use a unique key for stable pages)",
            "",
        )
        .optional(),
        choice(
            "direction",
            "Sort direction",
            "ascending",
            &["ascending", "descending"],
        ),
        Field::text("filter_column", "Filter column (optional)", "").optional(),
        choice("filter_operator", "Filter operator", "eq", OPERATORS),
        Field::text("filter_value", "Filter value (empty string is a value)", "").optional(),
        Field::text("filter2_column", "Second AND filter column (optional)", "").optional(),
        choice(
            "filter2_operator",
            "Second filter operator",
            "eq",
            OPERATORS,
        ),
        Field::text("filter2_value", "Second filter value", "").optional(),
    ]);
    fields
}

/// Purely in-memory: no discovery, filesystem, credential-store or network calls.
pub(super) fn list(context: &Context) -> Vec<Action> {
    let mut actions = vec![
        action("database.status", "MariaDB installation and service status", "Refresh local installation detection.", vec![], None),
        action("database.packages", "MariaDB package and update information", "Read the configured trusted package source; no changes.", vec![], None),
        action("database.versions", "MariaDB series and package versions", "Blank series lists choices; a series lists available releases.", vec![Field::text("series", "Series (optional)", "").optional()], None),
        action("database.update", "Update MariaDB", "Compatible package update; may restart the service. Back up first.", vec![], Some("UPDATE MARIADB")),
        action("database.uninstall", "Uninstall MariaDB software", "Remove the package without intentionally deleting database files. Back up first.", vec![], Some("UNINSTALL MARIADB")),
        action("database.start", "Start MariaDB service", "Operate the locally detected service, not the remote database endpoint.", vec![], Some("START MARIADB")),
        action("database.stop", "Stop MariaDB service", "Disconnects clients of the locally detected service.", vec![], Some("STOP MARIADB")),
        action("database.restart", "Restart MariaDB service", "Interrupts clients of the locally detected service.", vec![], Some("RESTART MARIADB")),
        action("database.connect", "Validate and use database login", "Updates this session only; remembering a login is a separate opt-in action.", vec![
            Field::text("host", "Server host", &context.credentials.host),
            Field::text("port", "Server port", context.credentials.port.to_string()),
            Field::text("username", "Login username", &context.credentials.username),
            Field::text("password", "Login password (blank means empty)", "").optional().secret(),
            database_field(context).optional(),
        ], None),
        action("database.remember-login", "Remember current database login", "Validate and encrypt using the OS protected store. Never writes credentials to terminal.json.", vec![], Some("REMEMBER LOGIN")),
        action("database.load-login", "Load remembered database login", "Load and validate the login bound to this configured endpoint and server profile.", vec![], None),
        action("database.forget-login", "Forget remembered database login", "Remove the protected login for this endpoint/profile; the current session remains connected.", vec![], Some("FORGET LOGIN")),
        action("database.list", "List databases", "List databases visible to the current login.", vec![], None),
        action("database.tables", "List table metadata", "At most 500 tables, including engine, estimated rows and sizes.", vec![database_field(context)], None),
        action("database.metadata", "Inspect table columns and indexes", "Bounded metadata including row-edit safety information.", table_fields(context), None),
        action("database.rows", "Browse filtered table rows", "Bounded read-only page; cells and total page size are capped. NULL is distinct from empty text.", row_fields(context), None),
        action("database.inspect", "Inspect database and server", "Read-only bounded snapshots; variables are redacted and process SQL is omitted.", vec![
            choice("view", "Inspection", "overview", &["overview", "status", "processes", "variables", "charsets", "engines", "users", "grants"]),
            database_field(context).optional(),
        ], None),
        action("database.sql-inspection", "Inspect SQL compatibility", "Inspect SQL mode, charset, collations and foreign-key types without executing supplied SQL.", vec![database_field(context), Field::text("table", "Table (optional)", "").optional()], None),
        action("database.create-preview", "Preview create database", "Generate a one-shot 90-second preview. No DDL is executed until Apply.", vec![database_field(context)], None),
        action("database.table-preview", "Preview table maintenance", "One exact base table. System databases and unsupported repair engines are refused.", {
            let mut fields = table_fields(context);
            fields.push(choice("operation", "Operation", "check", &["check", "analyze", "optimize", "repair", "empty", "drop"]));
            fields
        }, None),
        action("database.admin-apply", "Apply reviewed database/table action", "Single-use preview; exact target confirmation and unchanged metadata are required.", vec![
            Field::text("token", "Preview token", ""), Field::text("target", "Exact confirmation from preview", ""),
        ], Some("APPLY DATABASE ACTION")),
        action("database.users", "List database accounts", "Bounded account names and plugins only; no password hashes.", vec![], None),
        action("database.grants", "Inspect visible explicit grants", "Global, database, table and column privileges; no authentication hashes or expanded role inheritance.", vec![], None),
    ];
    let mut install_fields = vec![Field::text(
        "version",
        "Version or series (blank uses package default)",
        "",
    )
    .optional()];
    if cfg!(windows) {
        install_fields.extend([
            Field::text("new_password", "New root password", "").secret(),
            Field::text("repeat_password", "Repeat root password", "").secret(),
        ]);
    }
    actions.push(action("database.install", "Install MariaDB", "Local installation only. Linux keeps distro defaults and authentication; Windows initializes a new root password.", install_fields, Some("INSTALL MARIADB")));
    let mut export = row_fields(context);
    export.retain(|field| !["offset", "page_size"].contains(&field.key));
    export.push(Field::text("output", "New absolute .csv file path", ""));
    actions.push(action("database.export", "Export filtered table CSV", "First 5000 matching rows, at most 8 MiB; existing files never overwritten. Spreadsheet formulas are neutralized.", export, Some("EXPORT DATABASE DATA")));
    let mut create = table_fields(context);
    create.extend([
        choice(
            "template",
            "Table template",
            "records",
            &["records", "key-value", "id-only"],
        ),
        Field::text("key_column", "Primary-key column", "id"),
        Field::text("value_column", "Payload column (records/key-value)", "data"),
        choice(
            "value_type",
            "Payload type",
            "TEXT",
            &["TEXT", "JSON", "VARCHAR"],
        ),
    ]);
    actions.push(action("database.create-table-preview", "Preview create table", "InnoDB/utf8mb4. Records: auto-increment BIGINT key plus nullable payload. Key-value: VARCHAR(191) key plus payload.", create, None));
    for (id, title, description) in [
        ("database.user-create", "Create database account", "Strict CREATE USER; an existing account is never overwritten. Grant access separately."),
        ("database.user-password", "Change account password", "Preserves the authentication plugin unless compatibility mode is explicitly selected."),
        ("database.user-delete", "Delete database account", "Removes only the exact user@host account; review active application dependencies first."),
    ] {
        let mut fields = account_fields();
        if id != "database.user-delete" { fields.extend(password_fields()); }
        fields.push(Field::text("target", "Confirm exact username@host", ""));
        actions.push(action(id, title, description, fields, Some("CHANGE DATABASE ACCOUNT")));
    }
    let mut grant = account_fields();
    grant.extend([
        database_field(context),
        choice(
            "privileges",
            "Database-scoped privilege set",
            "read-only",
            &["read-only", "read-write", "all-database", "custom"],
        ),
        Field::text(
            "custom_privileges",
            "Custom privileges (comma-separated names)",
            "",
        )
        .optional(),
        Field::text("target", "Confirm exact username@host", ""),
    ]);
    actions.push(action("database.user-grant", "Grant account database privileges", "Additive grants on one exact database. No global privileges, grant option or implicit revocation.", grant, Some("GRANT DATABASE ACCESS")));
    actions.push(action("database.backup", "Back up a database", "One explicitly selected database; never overwrite a dump. Full backups may contain routines, triggers and events.", vec![
        database_field(context), Field::text("output", "Existing absolute output directory", ""),
        Field::text("file_name", "New dump filename (optional)", "").optional(),
        Field::text("table", "One table (blank backs up the database)", "").optional(),
        choice("mode", "Backup contents", "full", &["full", "schema-only", "data-only"]),
        choice("objects", "Include routines, triggers and events", "yes", YES_NO),
    ], Some("BACK UP DATABASE")));
    actions.extend([
        action("database.restore-preview", "Preview isolated restore test", "Preflight a constrained UTF-8 dump up to 32 MiB. Never targets an existing database; routines/triggers and unsafe SQL are refused.", vec![
            Field::text("source", "Absolute SQL dump path", ""), Field::text("receipt", "New absolute .json recovery receipt path", ""),
        ], None),
        action("database.restore-apply", "Apply isolated restore test", "Creates only the new app-owned database shown in preview. A recovery receipt is persisted before any SQL.", vec![
            Field::text("token", "Restore preview token", ""), Field::text("target", "Exact generated test database from preview", ""),
        ], Some("RESTORE TEST DATABASE")),
        action("database.restore-cleanup", "Remove owned restore-test database", "Requires the recovery receipt, matching endpoint and the database ownership marker. Never removes an arbitrary database.", vec![
            Field::text("receipt", "Absolute recovery receipt .json path", ""), Field::text("target", "Exact generated test database to delete", ""),
        ], Some("DELETE RESTORE TEST")),
    ]);
    actions
}

fn optional(submission: &Submission, key: &str) -> Option<String> {
    let value = submission.get(key).trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn bounded(submission: &Submission, key: &str, min: usize, max: usize) -> Result<usize, String> {
    let value = submission.number::<usize>(key)?;
    if !(min..=max).contains(&value) {
        return Err(format!("{key} must be between {min} and {max}."));
    }
    Ok(value)
}

fn validate_form(action: &Action, submission: &Submission) -> Result<(), String> {
    for field in &action.fields {
        let value = submission.get(field.key);
        if value.len() > 8192 {
            return Err(format!("{} exceeds 8192 bytes.", field.label));
        }
        if field.required && value.trim().is_empty() {
            return Err(format!("{} is required.", field.label));
        }
        if let FieldKind::Choice(options) = &field.kind {
            if !options.iter().any(|option| option == value) {
                return Err(format!("Select a valid {}.", field.label));
            }
        }
    }
    Ok(())
}

fn filter(
    submission: &Submission,
    column: &str,
    operator: &str,
    value: &str,
) -> Result<Option<browser::BrowserFilter>, String> {
    let Some(column) = optional(submission, column) else {
        if !submission.get(value).is_empty() {
            return Err("A filter value requires a filter column.".into());
        }
        return Ok(None);
    };
    let operator = match submission.get(operator) {
        "eq" => browser::FilterOperator::Eq,
        "ne" => browser::FilterOperator::Ne,
        "lt" => browser::FilterOperator::Lt,
        "lte" => browser::FilterOperator::Lte,
        "gt" => browser::FilterOperator::Gt,
        "gte" => browser::FilterOperator::Gte,
        "contains" => browser::FilterOperator::Contains,
        "isNull" => browser::FilterOperator::IsNull,
        "isNotNull" => browser::FilterOperator::IsNotNull,
        _ => return Err("Invalid filter operator.".into()),
    };
    let null = matches!(
        operator,
        browser::FilterOperator::IsNull | browser::FilterOperator::IsNotNull
    );
    if null && !submission.get(value).is_empty() {
        return Err("NULL filters take no value.".into());
    }
    if submission.get(value).len() > 512 {
        return Err("Filter values are limited to 512 bytes.".into());
    }
    Ok(Some(browser::BrowserFilter {
        column,
        operator,
        value: (!null).then(|| submission.get(value).to_owned()),
    }))
}

fn browser_request(
    submission: &Submission,
    export: bool,
) -> Result<browser::BrowserRequest, String> {
    Ok(browser::BrowserRequest {
        database: submission.required("database")?.into(),
        table: submission.required("table")?.into(),
        filters: [
            filter(
                submission,
                "filter_column",
                "filter_operator",
                "filter_value",
            )?,
            filter(
                submission,
                "filter2_column",
                "filter2_operator",
                "filter2_value",
            )?,
        ]
        .into_iter()
        .flatten()
        .collect(),
        sort_column: optional(submission, "sort_column"),
        descending: submission.get("direction") == "descending",
        offset: if export {
            0
        } else {
            bounded(submission, "offset", 0, 1_000_000)?
        },
        page_size: if export {
            200
        } else {
            bounded(submission, "page_size", 1, 200)?
        },
    })
}

fn new_password(submission: &Submission) -> Result<String, String> {
    let password = submission.get("new_password");
    if password.trim().is_empty() || password.chars().any(char::is_control) {
        return Err("Use a nonempty password without control characters.".into());
    }
    if password != submission.get("repeat_password") {
        return Err("Passwords do not match.".into());
    }
    Ok(password.into())
}

fn privileges(submission: &Submission) -> Result<Vec<String>, String> {
    let privileges = match submission.get("privileges") {
        "read-only" => vec!["SELECT".into(), "SHOW VIEW".into()],
        "read-write" => ["SELECT", "INSERT", "UPDATE", "DELETE", "SHOW VIEW"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        "all-database" => vec!["ALL PRIVILEGES".into()],
        "custom" => submission
            .required("custom_privileges")?
            .split(',')
            .map(|value| value.trim().to_owned())
            .collect(),
        _ => return Err("Select a privilege set.".into()),
    };
    permissions::normalize_privileges(privileges.clone())?;
    Ok(privileges)
}

fn admin_request(submission: &Submission, workspace: &str) -> Result<admin::AdminRequest, String> {
    let operation = match submission.id.as_str() {
        "database.create-preview" => admin::AdminAction::CreateDatabase,
        "database.create-table-preview" => admin::AdminAction::CreateTable,
        _ => match submission.get("operation") {
            "check" => admin::AdminAction::Check,
            "analyze" => admin::AdminAction::Analyze,
            "optimize" => admin::AdminAction::Optimize,
            "repair" => admin::AdminAction::Repair,
            "empty" => admin::AdminAction::Empty,
            "drop" => admin::AdminAction::Drop,
            _ => return Err("Choose a table operation.".into()),
        },
    };
    let columns = if operation == admin::AdminAction::CreateTable {
        table_columns(submission)?
    } else {
        vec![]
    };
    Ok(admin::AdminRequest {
        workspace_id: workspace.into(),
        database: submission.required("database")?.into(),
        table: if operation == admin::AdminAction::CreateDatabase {
            None
        } else {
            Some(submission.required("table")?.into())
        },
        action: operation,
        columns,
    })
}

fn table_columns(submission: &Submission) -> Result<Vec<admin::ColumnSpec>, String> {
    let template = submission.required("template")?;
    if !["records", "key-value", "id-only"].contains(&template) {
        return Err("Choose a table template.".into());
    }
    let key_value = template == "key-value";
    let mut columns = vec![admin::ColumnSpec {
        name: submission.required("key_column")?.into(),
        data_type: if key_value { "VARCHAR" } else { "BIGINT" }.into(),
        length: key_value.then(|| "191".into()),
        unsigned: !key_value,
        nullable: false,
        default_kind: "none".into(),
        default_value: None,
        auto_increment: !key_value,
        primary: true,
        unique: false,
    }];
    if template != "id-only" {
        let kind = submission.required("value_type")?;
        if !["TEXT", "JSON", "VARCHAR"].contains(&kind) {
            return Err("Choose a payload type.".into());
        }
        let name = submission.required("value_column")?;
        if name.eq_ignore_ascii_case(&columns[0].name) {
            return Err("Key and payload columns must differ.".into());
        }
        columns.push(admin::ColumnSpec {
            name: name.into(),
            data_type: kind.into(),
            length: (kind == "VARCHAR").then(|| "255".into()),
            unsigned: false,
            nullable: true,
            default_kind: "null".into(),
            default_value: None,
            auto_increment: false,
            primary: false,
            unique: false,
        });
    }
    Ok(columns)
}

/// Stable, non-secret key shared by startup and the remember/load/forget forms.
pub(in crate::terminal) fn saved_login_id(settings: &Settings) -> String {
    let identity = serde_json::to_vec(&(
        "terminal-login-v1",
        &settings.database_host,
        settings.database_port,
        &settings.database_user,
        &settings.profile,
        &settings.artifact_path,
        &settings.tx_data_path,
    ))
    .expect("string tuple serialization");
    format!("terminal-{:x}", Sha256::digest(identity))
}

pub(in crate::terminal) async fn load_saved_login(
    settings: &Settings,
) -> Result<Option<MariaDBCredentials>, String> {
    let saved =
        logins::load_database_login_at(logins::terminal_login_root()?, saved_login_id(settings))
            .await?;
    saved
        .map(|mut credentials| {
            validate_login_identity(settings, &credentials)?;
            credentials.database =
                (!settings.database.is_empty()).then(|| settings.database.clone());
            Ok(credentials)
        })
        .transpose()
}

fn validate_login_identity(
    settings: &Settings,
    credentials: &MariaDBCredentials,
) -> Result<(), String> {
    if credentials.host != settings.database_host
        || credentials.port != settings.database_port
        || credentials.username != settings.database_user
    {
        return Err(
            "Saved login does not match the configured database endpoint and username.".into(),
        );
    }
    Ok(())
}

fn text(message: impl Into<String>) -> Result<Outcome, String> {
    Ok(Outcome::Text(message.into()))
}

pub(super) async fn execute(context: &Context, submission: &Submission) -> Result<Outcome, String> {
    let action = list(context)
        .into_iter()
        .find(|action| action.id == submission.id)
        .ok_or("Unknown database action.")?;
    validate_form(&action, submission)?;
    let result = execute_validated(context, submission).await;
    result.map_err(|error| {
        super::redact(
            &error,
            &[
                &context.credentials.password,
                submission.get("password"),
                submission.get("new_password"),
                submission.get("repeat_password"),
            ],
        )
    })
}

async fn execute_validated(context: &Context, submission: &Submission) -> Result<Outcome, String> {
    let mut credentials = context.credentials.clone();
    credentials.database = None;
    match submission.id.as_str() {
        "database.status" => json(
            &commands::run_blocking(|| {
                detect::clear_detection_cache();
                Ok(detect::detect_mariadb())
            })
            .await?,
        ),
        "database.packages" => {
            json(&commands::run_blocking(|| Ok(install::get_package_info())).await?)
        }
        "database.versions" => {
            if let Some(series) = optional(submission, "series") {
                json(&commands::run_blocking(move || install::list_releases(&series)).await?)
            } else {
                json(&commands::run_blocking(install::list_series).await?)
            }
        }
        "database.install" | "database.update" | "database.uninstall" | "database.start"
        | "database.stop" | "database.restart" => {
            let id = submission.id.clone();
            let version = optional(submission, "version");
            let root_password = if id == "database.install" && cfg!(windows) {
                new_password(submission)?
            } else {
                String::new()
            };
            let report_context = context.clone();
            let report_password = root_password.clone();
            text(
                commands::run_blocking(move || {
                    let _lease = access::maintenance_access()?;
                    let report =
                        |message: &str| report_context.report(message, &[&report_password]);
                    match id.as_str() {
                        "database.install" => install::install_mariadb(
                            MariaDBInstallOptions {
                                version,
                                root_password,
                                service_name: "mariadb".into(),
                                port: 3306,
                                install_dir: None,
                                data_dir: None,
                                allow_remote_root_access: false,
                                create_anonymous_user: false,
                                skip_networking: false,
                                optimize_for_transactions: false,
                                use_utf8: false,
                                page_size: None,
                                buffer_pool_size: None,
                                install_heidi_sql: false,
                                install_development_files: false,
                            },
                            &report,
                        ),
                        "database.update" => install::update_mariadb(&report),
                        "database.uninstall" => install::uninstall_mariadb(&report),
                        _ => {
                            match id.as_str() {
                                "database.start" => service::start_service(None),
                                "database.stop" => service::stop_service(None),
                                _ => service::restart_service(None),
                            }?;
                            Ok("MariaDB service operation completed.".into())
                        }
                    }
                })
                .await?,
            )
        }
        "database.connect" => {
            let credentials = MariaDBCredentials {
                host: submission.required("host")?.into(),
                port: bounded(submission, "port", 1, 65535)? as u16,
                username: submission.required("username")?.into(),
                password: submission.get("password").into(),
                database: optional(submission, "database"),
            };
            let checked = credentials.clone();
            commands::run_blocking(move || {
                let _lease = access::database_access()?;
                query::validate_connection(checked)
            })
            .await?;
            Ok(Outcome::Login(credentials))
        }
        "database.remember-login" => {
            validate_login_identity(&context.settings, &context.credentials)?;
            logins::save_database_login_at(
                logins::terminal_login_root()?,
                saved_login_id(&context.settings),
                context.credentials.clone(),
            )
            .await?;
            text("Validated login encrypted in the OS-protected store. No password was written to terminal.json.")
        }
        "database.load-login" => {
            let credentials = load_saved_login(&context.settings)
                .await?
                .ok_or("No login is remembered for this endpoint/profile.")?;
            let checked = credentials.clone();
            commands::run_blocking(move || {
                let _lease = access::database_access()?;
                query::validate_connection(checked)
            })
            .await?;
            Ok(Outcome::Login(credentials))
        }
        "database.forget-login" => {
            logins::clear_database_login_at(
                logins::terminal_login_root()?,
                saved_login_id(&context.settings),
            )
            .await?;
            text("Remembered login removed. The in-memory session is unchanged.")
        }
        "database.list" => json(
            &commands::run_blocking(move || {
                let _lease = access::database_access()?;
                query::list_databases(credentials)
            })
            .await?,
        ),
        "database.tables" => json(
            &admin::list_database_admin_tables(
                credentials,
                submission.required("database")?.into(),
            )
            .await?,
        ),
        "database.metadata" => json(
            &browser::get_database_browser_metadata(
                credentials,
                submission.required("database")?.into(),
                submission.required("table")?.into(),
            )
            .await?,
        ),
        "database.rows" => {
            let request = browser_request(submission, false)?;
            let metadata = browser::get_database_browser_metadata(
                credentials.clone(),
                request.database.clone(),
                request.table.clone(),
            )
            .await?;
            let page = browser::get_database_browser_rows(credentials, request).await?;
            json(&serde_json::json!({"metadata": metadata, "page": page}))
        }
        "database.export" => json(
            &browser::export_database_browser_csv(
                credentials,
                browser_request(submission, true)?,
                submission.required("output")?.into(),
            )
            .await?,
        ),
        "database.inspect" | "database.users" | "database.grants" => {
            let view = match submission.id.as_str() {
                "database.users" => "users",
                "database.grants" => "grants",
                _ => submission.get("view"),
            };
            let view = match view {
                "overview" => inspection::InspectionView::Overview,
                "status" => inspection::InspectionView::Status,
                "processes" => inspection::InspectionView::Processes,
                "variables" => inspection::InspectionView::Variables,
                "charsets" => inspection::InspectionView::Charsets,
                "engines" => inspection::InspectionView::Engines,
                "users" => inspection::InspectionView::Users,
                "grants" => inspection::InspectionView::Grants,
                _ => return Err("Unknown inspection view.".into()),
            };
            json(
                &inspection::inspect_database(
                    credentials,
                    view,
                    submission.get("database").trim().into(),
                )
                .await?,
            )
        }
        "database.sql-inspection" => json(
            &sql_diagnostics::inspect_database_sql(
                credentials,
                submission.required("database")?.into(),
                optional(submission, "table"),
            )
            .await?,
        ),
        "database.create-preview" | "database.create-table-preview" | "database.table-preview" => {
            json(
                &admin::preview_database_admin_action(
                    credentials,
                    admin_request(submission, &saved_login_id(&context.settings))?,
                )
                .await?,
            )
        }
        "database.admin-apply" => json(
            &admin::apply_database_admin_action_headless(
                saved_login_id(&context.settings),
                submission.required("token")?.into(),
                submission.required("target")?.into(),
            )
            .await?,
        ),
        "database.user-create"
        | "database.user-password"
        | "database.user-delete"
        | "database.user-grant" => {
            let username = submission.required("username")?.to_owned();
            let host = submission.required("host")?.to_owned();
            if submission.get("target") != format!("{username}@{host}") {
                return Err("Exact username@host confirmation does not match.".into());
            }
            let id = submission.id.clone();
            let password =
                if ["database.user-create", "database.user-password"].contains(&id.as_str()) {
                    Some(new_password(submission)?)
                } else {
                    None
                };
            let native_password = submission.get("native_password") == "yes";
            let grants = if id == "database.user-grant" {
                privileges(submission)?
            } else {
                vec![]
            };
            let database = if id == "database.user-grant" {
                let database = submission.required("database")?;
                backup_storage::validate_database(database)?;
                Some(database.to_owned())
            } else {
                None
            };
            commands::run_blocking(move || {
                let _lease = access::maintenance_access()?;
                match id.as_str() {
                    "database.user-create" => users::create_user(
                        credentials,
                        username,
                        host,
                        password.unwrap_or_default(),
                        native_password,
                    ),
                    "database.user-password" => users::update_user(
                        credentials,
                        MariaDBUserUpdateConfig {
                            username,
                            host,
                            password,
                            native_password,
                            database: None,
                            privileges: vec![],
                        },
                    ),
                    "database.user-delete" => users::drop_user(credentials, username, host),
                    _ => users::grant_permissions(
                        credentials,
                        username,
                        host,
                        database.ok_or("Select a database.")?,
                        grants,
                    ),
                }
            })
            .await?;
            text("Account operation completed. Refresh accounts and grants before another change.")
        }
        "database.restore-preview" => json(
            &crate::services::mariadb::safe_restore::preview(
                credentials,
                submission.required("source")?.into(),
                submission.required("receipt")?.into(),
            )
            .await?,
        ),
        "database.restore-apply" => json(
            &crate::services::mariadb::safe_restore::apply(
                credentials,
                submission.required("token")?.into(),
                submission.required("target")?.into(),
            )
            .await?,
        ),
        "database.restore-cleanup" => {
            crate::services::mariadb::safe_restore::cleanup(
                credentials,
                submission.required("receipt")?.into(),
                submission.required("target")?.into(),
            )
            .await?;
            text("Owned restore-test database removed. The recovery receipt was retained.")
        }
        "database.backup" => {
            let database = submission.required("database")?.to_owned();
            backup_storage::validate_database(&database)?;
            let objects = submission.get("objects") == "yes";
            let options = MariaDBBackupOptions {
                output_dir: submission.required("output")?.into(),
                file_name: optional(submission, "file_name"),
                database: Some(database),
                tables: optional(submission, "table").into_iter().collect(),
                all_databases: false,
                schema_only: submission.get("mode") == "schema-only",
                data_only: submission.get("mode") == "data-only",
                include_routines: objects,
                include_triggers: objects,
                include_events: objects,
                single_transaction: true,
                add_drop_statements: false,
                where_clause: None,
            };
            json(
                &commands::run_blocking(move || {
                    let _lease = access::database_access()?;
                    backup::create_backup(credentials, options)
                })
                .await?,
            )
        }
        _ => Err("Unknown database action.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{commands::fxserver::FxserverManager, models::fxserver::FxserverRconConfig};
    use std::collections::{BTreeMap, HashSet};

    fn context() -> Context {
        Context {
            settings: Settings::default(),
            credentials: MariaDBCredentials {
                host: "localhost".into(),
                port: 3306,
                username: "root".into(),
                password: "never-render-this".into(),
                database: Some("fixture".into()),
            },
            rcon: FxserverRconConfig {
                host: "127.0.0.1".into(),
                port: 30120,
                password: "private-rcon".into(),
            },
            manager: FxserverManager::default(),
            progress: None,
        }
    }

    fn form(id: &str) -> (Action, Submission) {
        let action = list(&context())
            .into_iter()
            .find(|action| action.id == id)
            .unwrap();
        let submission = Submission {
            id: id.into(),
            values: action
                .fields
                .iter()
                .map(|field| (field.key.into(), field.value.clone()))
                .collect(),
        };
        (action, submission)
    }

    fn set(submission: &mut Submission, key: &str, value: &str) {
        submission.values.insert(key.into(), value.into());
    }

    #[test]
    fn catalog_is_unique_and_never_embeds_session_secrets() {
        let actions = list(&context());
        assert!(actions.len() >= 34);
        let mut ids = HashSet::new();
        for action in actions {
            assert!(action.id.starts_with("database."));
            assert!(ids.insert(action.id));
            let mut keys = HashSet::new();
            for field in action.fields {
                assert!(
                    keys.insert(field.key),
                    "{} repeats {}",
                    action.id,
                    field.key
                );
                assert!(!field.value.contains("never-render-this"));
                assert!(!field.value.contains("private-rcon"));
                if field.key.contains("password") && field.key != "native_password" {
                    assert!(matches!(field.kind, FieldKind::Secret));
                    assert!(field.value.is_empty());
                }
                if let FieldKind::Choice(values) = field.kind {
                    assert!(values.contains(&field.value));
                }
            }
        }
    }

    #[test]
    fn all_mutating_forms_require_explicit_confirmation() {
        for id in [
            "install",
            "update",
            "uninstall",
            "start",
            "stop",
            "restart",
            "remember-login",
            "forget-login",
            "admin-apply",
            "user-create",
            "user-password",
            "user-delete",
            "user-grant",
            "restore-apply",
            "restore-cleanup",
            "export",
            "backup",
        ] {
            let (action, _) = form(&format!("database.{id}"));
            assert!(action.confirmation.is_some(), "{id}");
        }
    }

    #[test]
    fn forms_validate_required_fields_choices_and_lengths() {
        let (action, mut submission) = form("database.user-grant");
        assert!(validate_form(&action, &submission).is_err());
        for field in &action.fields {
            if field.required && submission.get(field.key).is_empty() {
                set(&mut submission, field.key, "fixture");
            }
        }
        assert!(validate_form(&action, &submission).is_ok());
        set(&mut submission, "privileges", "GRANT OPTION");
        assert!(validate_form(&action, &submission).is_err());
        set(&mut submission, "privileges", "custom");
        set(&mut submission, "username", &"x".repeat(8193));
        assert!(validate_form(&action, &submission).is_err());
    }

    #[test]
    fn row_builders_bound_pages_and_preserve_literal_filters() {
        let (_, mut submission) = form("database.rows");
        set(&mut submission, "table", "players");
        set(&mut submission, "filter_column", "name");
        set(&mut submission, "filter_value", " ' OR 1=1; -- ");
        let request = browser_request(&submission, false).unwrap();
        assert_eq!(request.filters[0].value.as_deref(), Some(" ' OR 1=1; -- "));
        assert_eq!(request.page_size, 25);
        for value in ["0", "201", "-1", "NaN"] {
            set(&mut submission, "page_size", value);
            assert!(browser_request(&submission, false).is_err());
        }
        set(&mut submission, "page_size", "200");
        set(&mut submission, "offset", "1000001");
        assert!(browser_request(&submission, false).is_err());
        assert_eq!(browser_request(&submission, true).unwrap().offset, 0);
    }

    #[test]
    fn null_filters_and_empty_text_remain_distinct() {
        let (_, mut submission) = form("database.rows");
        set(&mut submission, "table", "players");
        set(&mut submission, "filter_column", "name");
        let request = browser_request(&submission, false).unwrap();
        assert_eq!(request.filters[0].value.as_deref(), Some(""));
        set(&mut submission, "filter_operator", "isNull");
        assert!(browser_request(&submission, false).unwrap().filters[0]
            .value
            .is_none());
        set(&mut submission, "filter_value", "not ignored");
        assert!(browser_request(&submission, false).is_err());
        set(&mut submission, "filter_column", "");
        assert!(browser_request(&submission, false).is_err());
    }

    #[test]
    fn password_confirmation_preserves_significant_whitespace() {
        let (_, mut submission) = form("database.user-create");
        set(&mut submission, "new_password", " spaced secret ");
        set(&mut submission, "repeat_password", " spaced secret ");
        assert_eq!(new_password(&submission).unwrap(), " spaced secret ");
        set(&mut submission, "repeat_password", "spaced secret");
        assert!(new_password(&submission).is_err());
        set(&mut submission, "new_password", "bad\nsecret");
        set(&mut submission, "repeat_password", "bad\nsecret");
        assert!(new_password(&submission).is_err());
    }

    #[test]
    fn privilege_builder_rejects_sql_and_global_privileges() {
        let (_, mut submission) = form("database.user-grant");
        assert_eq!(privileges(&submission).unwrap(), ["SELECT", "SHOW VIEW"]);
        set(&mut submission, "privileges", "custom");
        for value in [
            "SELECT; DROP DATABASE x",
            "SUPER",
            "FILE",
            "GRANT OPTION",
            "ALL PRIVILEGES,SELECT",
            "",
        ] {
            set(&mut submission, "custom_privileges", value);
            assert!(privileges(&submission).is_err(), "{value}");
        }
        set(
            &mut submission,
            "custom_privileges",
            "SELECT, INSERT, UPDATE",
        );
        assert_eq!(privileges(&submission).unwrap().len(), 3);
    }

    #[test]
    fn table_templates_produce_typed_specs_and_bound_workspace() {
        let (_, mut submission) = form("database.create-table-preview");
        set(&mut submission, "table", "records");
        for template in ["records", "key-value", "id-only"] {
            set(&mut submission, "template", template);
            let request = admin_request(&submission, "fixture-profile").unwrap();
            assert_eq!(request.workspace_id, "fixture-profile");
            assert_eq!(request.action, admin::AdminAction::CreateTable);
            assert_eq!(
                request.columns.len(),
                if template == "id-only" { 1 } else { 2 }
            );
            assert!(request.columns[0].primary);
            assert!(!request.columns[0].nullable);
        }
        set(&mut submission, "template", "records");
        set(&mut submission, "value_column", "ID");
        assert!(table_columns(&submission).is_err());
    }

    #[test]
    fn remembered_login_key_binds_endpoint_and_server_context_not_selected_database() {
        let settings = Settings::default();
        let key = saved_login_id(&settings);
        assert_eq!(key.len(), 73);
        backup_storage::validate_id(&key).unwrap();
        let mut changed = settings.clone();
        changed.database = "another-database".into();
        assert_eq!(key, saved_login_id(&changed));
        for field in ["host", "port", "user", "profile", "artifact", "txdata"] {
            let mut changed = settings.clone();
            match field {
                "host" => changed.database_host = "other".into(),
                "port" => changed.database_port = 3307,
                "user" => changed.database_user = "other".into(),
                "profile" => changed.profile = "other".into(),
                "artifact" => changed.artifact_path = "other".into(),
                _ => changed.tx_data_path = "other".into(),
            }
            assert_ne!(key, saved_login_id(&changed), "{field}");
        }
        let mut credentials = context().credentials;
        assert!(validate_login_identity(&settings, &credentials).is_ok());
        credentials.host = "other".into();
        assert!(validate_login_identity(&settings, &credentials).is_err());
    }

    #[tokio::test]
    async fn invalid_actions_fail_before_service_or_database_access() {
        let context = context();
        let submission = Submission {
            id: "database.execute-sql".into(),
            values: BTreeMap::new(),
        };
        assert!(execute(&context, &submission).await.is_err());
        let (_, mut submission) = form("database.user-delete");
        set(&mut submission, "username", "fixture");
        set(&mut submission, "target", "another@localhost");
        assert!(execute(&context, &submission).await.is_err());
        let (_, mut submission) = form("database.user-create");
        set(&mut submission, "username", "fixture");
        set(&mut submission, "target", "fixture@localhost");
        set(&mut submission, "new_password", "one");
        set(&mut submission, "repeat_password", "two");
        assert!(execute(&context, &submission).await.is_err());
    }
}
