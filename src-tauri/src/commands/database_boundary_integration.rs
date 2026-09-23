//! Invoked only by the opt-in, owned-datadir MariaDB integration harness.
use super::{json_client, parse_json_output, query_json, quote_identifier, sql_text};
use crate::{
    models::mariadb::{MariaDBCredentials, MariaDBUserConfig},
    process::CommandNoWindowExt,
    services::mariadb::{query, users},
};
use std::{ffi::OsString, fs, path::Path, process::Command};

const MODES: [&str; 2] = [
    "STRICT_TRANS_TABLES,ANSI_QUOTES",
    "STRICT_ALL_TABLES,ANSI_QUOTES,NO_BACKSLASH_ESCAPES",
];

pub(super) fn exercise_grant_boundaries(credentials: &MariaDBCredentials) -> Result<(), String> {
    let original: Vec<String> = query_json(credentials, "SELECT JSON_QUOTE(@@GLOBAL.sql_mode);")?;
    let pairs = [("fxi_grant", "fxiXgrant"), ("fxi%grant", "fxiXYZgrant")];
    for (target, lookalike) in pairs {
        for database in [target, lookalike] {
            let database = quote_identifier(database)?;
            let _: Vec<serde_json::Value> = query_json(credentials, &format!(
                "CREATE DATABASE {database}; CREATE TABLE {database}.sample (value INT); INSERT INTO {database}.sample VALUES (7);"
            ))?;
        }
    }
    for mode in MODES {
        let _: Vec<serde_json::Value> = query_json(
            credentials,
            &format!("SET GLOBAL sql_mode={};", sql_text(mode)),
        )?;
        for (target, lookalike) in pairs {
            let username = "fxi_grant_boundary";
            let password = "fixture grant password";
            users::create_or_update_user(
                credentials.clone(),
                MariaDBUserConfig {
                    username: username.into(),
                    host: "localhost".into(),
                    password: password.into(),
                    native_password: false,
                    database: Some(target.into()),
                    privileges: vec!["SELECT".into()],
                },
            )?;
            let login = MariaDBCredentials {
                username: username.into(),
                password: password.into(),
                ..credentials.clone()
            };
            let target_sql = format!("SELECT value FROM {}.sample;", quote_identifier(target)?);
            let allowed = query::execute_query(login.clone(), target_sql)?;
            assert!(
                allowed.success,
                "exact grant target must remain readable: {}",
                allowed.stderr
            );
            assert_eq!(allowed.rows, [vec!["7"]]);
            let lookalike_sql =
                format!("SELECT value FROM {}.sample;", quote_identifier(lookalike)?);
            let exists = query::execute_query(credentials.clone(), lookalike_sql.clone())?;
            assert!(exists.success);
            assert_eq!(exists.rows, [vec!["7"]]);
            let denied = query::execute_query(login, lookalike_sql)?;
            assert!(
                !denied.success,
                "grant on {target} leaked access to {lookalike} in {mode}"
            );
            assert!(
                denied.stderr.contains("1142"),
                "expected SELECT access denial: {}",
                denied.stderr
            );
            let stored: Vec<String> = query_json(
                credentials,
                &format!(
                    "SELECT JSON_QUOTE(Db) FROM mysql.db WHERE User={};",
                    sql_text(username)
                ),
            )?;
            assert_eq!(stored, [target.replace('_', "\\_").replace('%', "\\%")]);
            users::drop_user(credentials.clone(), username.into(), "localhost".into())?;
        }
    }
    let _: Vec<serde_json::Value> = query_json(
        credentials,
        &format!("SET GLOBAL sql_mode={};", sql_text(&original[0])),
    )?;
    Ok(())
}

pub(super) fn exercise_batch_output(credentials: &MariaDBCredentials) -> Result<(), String> {
    let values = [
        "Unicode \u{1f642}\u{e5}\u{4e2d}\nnext\tcolumn\0nul\\backslash",
        "",
        "  leading and trailing  ",
        "literal\\n\\t\\0",
        "carriage\rreturn\r\nnext",
        "trailing carriage\r",
        "",
    ];
    let rows = values
        .iter()
        .enumerate()
        .map(|(id, value)| format!("({id},{})", sql_text(value)))
        .collect::<Vec<_>>()
        .join(",");
    let _: Vec<serde_json::Value> = query_json(credentials, &format!(
        "CREATE TABLE fxi_read_fixture.batch_cells (id INT PRIMARY KEY, value TEXT); INSERT INTO fxi_read_fixture.batch_cells VALUES {rows};"
    ))?;
    for mode in MODES {
        let prefix = format!("SET SESSION sql_mode={};", sql_text(mode));
        let result = query::execute_query(
            credentials.clone(),
            format!("{prefix} SELECT value FROM fxi_read_fixture.batch_cells ORDER BY id;"),
        )?;
        assert!(result.success, "{}", result.stderr);
        assert_eq!(result.columns, ["value"]);
        let expected: Vec<Vec<String>> =
            values.iter().map(|value| vec![value.to_string()]).collect();
        assert_eq!(result.rows, expected, "batch decoding in {mode}");
        assert!(result
            .stdout
            .contains("\\nnext\\tcolumn\\0nul\\\\backslash"));
        let empty = query::execute_query(
            credentials.clone(),
            format!("{prefix} SELECT '' AS first_cell, '' AS second_cell;"),
        )?;
        assert!(empty.success);
        assert_eq!(empty.rows, [vec!["", ""]]);
        let no_rows = query::execute_query(
            credentials.clone(),
            format!("{prefix} SELECT value FROM fxi_read_fixture.batch_cells WHERE 1=0;"),
        )?;
        assert!(no_rows.success);
        assert_eq!(no_rows.columns, ["value"]);
        assert!(no_rows.rows.is_empty());
        let literal = query::execute_query(
            credentials.clone(),
            format!("{prefix} SELECT @@SESSION.sql_mode AS mode, 'slash\\n' AS literal;"),
        )?;
        assert!(literal.success);
        for flag in mode.split(',') {
            assert!(literal.rows[0][0].split(',').any(|actual| actual == flag));
        }
        assert_eq!(
            literal.rows[0][1],
            if mode.contains("NO_BACKSLASH_ESCAPES") {
                "slash\\n"
            } else {
                "slash\n"
            }
        );
    }
    Ok(())
}

pub(super) fn exercise_defaults_isolation(
    credentials: &MariaDBCredentials,
    directory: &Path,
) -> Result<(), String> {
    let _: Vec<serde_json::Value> = query_json(
        credentials,
        "CREATE TABLE fxi_read_fixture.defaults_effects (id INT);",
    )?;
    let ambient = directory.join("my.ini");
    fs::write(
        &ambient,
        "[client]\ninit-command=SET @fxi_ambient_marker=1\nforce\n",
    )
    .map_err(|e| e.to_string())?;
    let (client, _credentials) = json_client(credentials)?;
    let args = client.get_args().map(OsString::from).collect::<Vec<_>>();
    let options = args[0]
        .to_str()
        .and_then(|arg| arg.strip_prefix("--defaults-file="))
        .ok_or("Expected production defaults-file isolation")?;
    // Positive control reads only two fixture files, never the machine's option files.
    let control_file = directory.join("control.cnf");
    fs::write(
        &control_file,
        format!(
            "!include {}\n!include {}\n",
            options.replace('\\', "/"),
            ambient.to_string_lossy().replace('\\', "/")
        ),
    )
    .map_err(|e| e.to_string())?;
    let mut control = Command::new(client.get_program());
    control
        .no_window()
        .arg(format!("--defaults-file={}", control_file.display()))
        .args(&args[1..])
        .env_remove("MYSQL_PWD");
    query::configure_query_command(&mut control, None)?;
    control.arg("--skip-column-names");
    let output = query::run_client(&mut control,
        "SELECT JSON_ARRAY(CAST(@fxi_ambient_marker AS UNSIGNED)); SELECT * FROM fxi_read_fixture.missing_table; INSERT INTO fxi_read_fixture.defaults_effects VALUES (1);".into(), query::QUERY_TIMEOUT)?;
    assert_eq!(parse_json_output::<Vec<u8>>(output.stdout)?, [vec![1]]);
    let control_count: Vec<u64> = query_json(
        credentials,
        "SELECT COUNT(*) FROM fxi_read_fixture.defaults_effects;",
    )?;
    assert_eq!(
        control_count,
        [1],
        "positive-control force option must continue after SQL failure"
    );
    let marker = query::execute_query(
        credentials.clone(),
        "SELECT @fxi_ambient_marker AS marker;".into(),
    )?;
    assert!(marker.success);
    assert_eq!(
        marker.rows,
        [vec!["NULL"]],
        "production client loaded ambient init-command"
    );
    let stopped = query::execute_query(credentials.clone(),
        "SELECT * FROM fxi_read_fixture.missing_table; INSERT INTO fxi_read_fixture.defaults_effects VALUES (2);".into())?;
    assert!(
        !stopped.success,
        "production client must stop on SQL errors"
    );
    let count: Vec<u64> = query_json(
        credentials,
        "SELECT COUNT(*) FROM fxi_read_fixture.defaults_effects;",
    )?;
    assert_eq!(count, [1], "production client inherited ambient force");
    let json_marker: Vec<Vec<Option<u8>>> =
        query_json(credentials, "SELECT JSON_ARRAY(@fxi_ambient_marker);")?;
    assert_eq!(
        json_marker,
        [vec![None]],
        "structured reads loaded ambient init-command"
    );
    fs::remove_file(control_file).map_err(|e| e.to_string())?;
    fs::remove_file(ambient).map_err(|e| e.to_string())?;
    Ok(())
}
