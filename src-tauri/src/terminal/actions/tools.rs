use super::{json, server, Action, Context, Field, Outcome, Submission};
use crate::{
    commands::{config_history as history, config_validation, fxserver, jooat, logs},
    models::fxserver::{ServerConfigFile, ServerConfigRequest, TxDataLogRequest},
};
use serde_json::{json as value, Value};
use std::path::Path;

const MAX_JSON_BYTES: usize = 512 * 1024;
const MAX_FORMATTED_BYTES: usize = 2 * 1024 * 1024;

fn profile_fields(context: &Context) -> Vec<Field> {
    vec![
        Field::text(
            "tx_data_path",
            "txData directory",
            &context.settings.tx_data_path,
        ),
        Field::text("profile", "Profile", &context.settings.profile),
    ]
}

fn config_fields(context: &Context) -> Vec<Field> {
    let mut fields = profile_fields(context);
    fields.push(Field::text("path", "Absolute cfg path", ""));
    fields
}

fn version_fields(context: &Context) -> Vec<Field> {
    let mut fields = config_fields(context);
    fields.push(Field::text("version_id", "History version ID", ""));
    fields
}

fn helper_fields(context: &Context, permission: bool, apply: bool) -> Vec<Field> {
    let mut fields = config_fields(context);
    if permission {
        fields.extend([
            Field::text("kind", "Permission command", "ACE")
                .choice(&["ACE", "Principal inheritance"]),
            Field::text("principal", "Principal (or inheriting child)", ""),
            Field::text("target", "ACE object (or parent principal)", ""),
            Field::text("access", "ACE access (unused for inheritance)", "allow")
                .choice(&["allow", "deny"]),
        ]);
    } else {
        fields.extend([
            Field::text("setting", "Common setting", "sv_hostname").choice(&[
                "sv_hostname",
                "sv_maxclients",
                "sets sv_projectName",
                "sets sv_projectDesc",
                "sets tags",
                "sets locale",
            ]),
            Field::text("value", "New value", ""),
        ]);
    }
    if apply {
        fields.extend([
            Field::text("revision", "Current revision (from helper preview)", ""),
            Field::text(
                "validation_revision",
                "Validation revision (from helper preview)",
                "",
            ),
        ]);
    }
    fields
}

pub(super) fn list(context: &Context) -> Vec<Action> {
    let mut restore_fields = version_fields(context);
    restore_fields.push(Field::text(
        "revision",
        "Current file revision (from read/list)",
        "",
    ));
    vec![
        Action {
            id: "config.list", group: "Config", title: "List cfg files",
            description: "List top-level cfg metadata and revisions without displaying contents.",
            fields: profile_fields(context), confirmation: None,
        },
        Action {
            id: "config.read", group: "Config", title: "Read cfg file",
            description: "Explicitly display one cfg and its revision. Contents may contain secrets.",
            fields: config_fields(context), confirmation: None,
        },
        Action {
            id: "config.validate", group: "Config", title: "Validate cfg file",
            description: "Validate the current cfg and include graph without displaying secret values.",
            fields: config_fields(context), confirmation: None,
        },
        Action {
            id: "config.history.list", group: "Config", title: "List config history",
            description: "List encrypted history metadata without displaying saved contents.",
            fields: config_fields(context), confirmation: None,
        },
        Action {
            id: "config.history.read", group: "Config", title: "Read config history version",
            description: "Explicitly display one saved cfg version. Contents may contain secrets.",
            fields: version_fields(context), confirmation: None,
        },
        Action {
            id: "config.history.restore", group: "Config", title: "Restore config history version",
            description: "Restore a saved version only if the current file revision still matches.",
            fields: restore_fields,
            confirmation: Some("Restore this saved cfg version? The managed server must be stopped. The current version will be retained in encrypted history."),
        },
        Action {
            id: "config.helper.value.preview", group: "Config", title: "Preview common cfg value",
            description: "Preview a single-command line replacement or addition. Inline comments on a replaced line are removed.",
            fields: helper_fields(context, false, false), confirmation: None,
        },
        Action {
            id: "config.helper.value.apply", group: "Config", title: "Apply common cfg value",
            description: "Apply a reviewed helper using its current-file and validation revisions.",
            fields: helper_fields(context, false, true),
            confirmation: Some("Apply this reviewed cfg value? The managed server must be stopped. The old config is retained in encrypted history."),
        },
        Action {
            id: "config.helper.permission.preview", group: "Config", title: "Preview cfg permission rule",
            description: "Preview an ACE or principal inheritance addition without displaying existing cfg contents.",
            fields: helper_fields(context, true, false), confirmation: None,
        },
        Action {
            id: "config.helper.permission.apply", group: "Config", title: "Apply cfg permission rule",
            description: "Apply a reviewed permission rule using its current-file and validation revisions.",
            fields: helper_fields(context, true, true),
            confirmation: Some("Apply this permission rule? It can grant or deny server privileges. The managed server must be stopped and the old config is retained in encrypted history."),
        },
        Action {
            id: "logs.application.list", group: "Logs", title: "List application log",
            description: "Show application log metadata without reading log contents.",
            fields: vec![], confirmation: None,
        },
        Action {
            id: "logs.server.tail", group: "Logs", title: "Tail txData server log",
            description: "Explicitly view an existing server/admin log, up to 5000 lines / 8 MiB. Logs may contain secrets.",
            fields: vec![
                Field::text("tx_data_path", "txData directory", &context.settings.tx_data_path),
                Field::text("profile", "Profile (empty = root logs)", &context.settings.profile).optional(),
                Field::text("log_name", "Log file", "fxserver.log").choice(&["fxserver.log", "admin.log", "server.log"]),
                Field::text("max_lines", "Maximum lines (50-5000)", "200"),
            ], confirmation: None,
        },
        Action {
            id: "logs.application.tail", group: "Logs", title: "Tail application log",
            description: "Explicitly view up to 5000 lines / 4 MiB. Logs may contain secrets.",
            fields: vec![Field::text("max_lines", "Maximum lines (1-5000)", "200")], confirmation: None,
        },
        Action {
            id: "logs.client.list", group: "Logs", title: "List FiveM client logs",
            description: "Windows only. List log metadata without reading log contents.",
            fields: vec![Field::text("directory", "Client log directory (empty = default)", "").optional()],
            confirmation: None,
        },
        Action {
            id: "logs.client.tail", group: "Logs", title: "Tail FiveM client log",
            description: "Windows only. Explicitly view one file, up to 5000 lines / 4 MiB; may contain secrets.",
            fields: vec![
                Field::text("directory", "Client log directory (empty = default)", "").optional(),
                Field::text("file_name", "Log filename (from list)", ""),
                Field::text("max_lines", "Maximum lines (1-5000)", "200"),
            ], confirmation: None,
        },
        Action {
            id: "tools.json.format", group: "Tools", title: "Format JSON text",
            description: "Display formatted single-line JSON; never writes a file. The text form is limited to 8 KiB.",
            fields: vec![Field::text("content", "JSON text", "").secret()], confirmation: None,
        },
        Action {
            id: "tools.json.file", group: "Tools", title: "Validate or format JSON file",
            description: "Read an unlinked UTF-8 file up to 512 KiB. Format explicitly displays its contents.",
            fields: vec![
                Field::text("path", "Absolute JSON file path", ""),
                Field::text("mode", "Operation", "Validate").choice(&["Validate", "Format"]),
            ], confirmation: None,
        },
        Action {
            id: "tools.jooat.status", group: "Tools", title: "Offline JOOAT resolver status",
            description: "Inspect the existing resolver database. No downloads or writes.",
            fields: vec![], confirmation: None,
        },
        Action {
            id: "tools.jooat.resolve", group: "Tools", title: "Resolve JOOAT hashes offline",
            description: "Look up at most 100 whitespace-separated hashes in the installed database.",
            fields: vec![Field::text("queries", "Hex, signed or unsigned hashes", "")], confirmation: None,
        },
    ]
}

pub(super) async fn execute(context: &Context, submission: &Submission) -> Result<Outcome, String> {
    match submission.id.as_str() {
        "config.list" => {
            let result = fxserver::read_server_config(ServerConfigRequest {
                tx_data_path: submission.required("tx_data_path")?.into(),
                profile: submission.required("profile")?.into(),
            })
            .await?;
            json(&value!({
                "txDataPath": result.tx_data_path,
                "profile": result.profile,
                "dataPath": result.data_path,
                "files": result.files.iter().map(config_metadata).collect::<Vec<_>>(),
            }))
        }
        "config.read" => {
            let file = history::read_config_history_file(config_request(submission)?).await?;
            json(&value!({"revision": history::content_revision(&file.content), "file": file}))
        }
        "config.validate" => {
            let request = config_request(submission)?;
            let file = history::read_config_history_file(request.clone()).await?;
            json(&config_validation::validate_config_change(request, file.content).await?)
        }
        "config.helper.value.preview"
        | "config.helper.value.apply"
        | "config.helper.permission.preview"
        | "config.helper.permission.apply" => {
            let request = config_request(submission)?;
            let change = helper_change(submission)?;
            if submission.id.ends_with(".preview") {
                return json(
                    &crate::commands::run_blocking(move || {
                        config_validation::preview_config_helper(&request, &change)
                    })
                    .await?,
                );
            }
            let revision = revision(submission)?;
            let validation_revision = submission.required("validation_revision")?.to_string();
            server::require_selected_profile(context, &request.tx_data_path, &request.profile)?;
            let lease = server::stopped_lease(context).await?;
            let manager = context.manager.clone();
            let file = crate::commands::run_blocking(move || {
                let _lease = lease;
                manager.with_stopped_server(|| {
                    config_validation::apply_config_helper(
                        &history::core_history_root()?,
                        &request,
                        &change,
                        &revision,
                        &validation_revision,
                    )
                })
            })
            .await?;
            json(&value!({"applied": true, "file": config_metadata(&file)}))
        }
        "config.history.list" => {
            let request = config_request(submission)?;
            json(
                &crate::commands::run_blocking(move || {
                    history::list_history_in(&history::core_history_root()?, &request)
                })
                .await?,
            )
        }
        "config.history.read" => {
            let request = config_request(submission)?;
            let id = submission.required("version_id")?.to_string();
            json(
                &crate::commands::run_blocking(move || {
                    history::read_history_in(&history::core_history_root()?, &request, &id)
                })
                .await?,
            )
        }
        "config.history.restore" => {
            let request = config_request(submission)?;
            let id = submission.required("version_id")?.to_string();
            let revision = revision(submission)?;
            server::require_selected_profile(context, &request.tx_data_path, &request.profile)?;
            let lease = server::stopped_lease(context).await?;
            let manager = context.manager.clone();
            let file = crate::commands::run_blocking(move || {
                let _lease = lease;
                manager.with_stopped_server(|| {
                    history::restore_history_with_revision(
                        &history::core_history_root()?,
                        &request,
                        &id,
                        &revision,
                    )
                })
            })
            .await?;
            json(&value!({"restored": true, "file": config_metadata(&file)}))
        }
        "logs.application.list" => json(
            &crate::commands::run_blocking(|| logs::list_app_logs_at(&logs::core_app_log_path()?))
                .await?,
        ),
        "logs.application.tail" => {
            let lines = max_lines(submission)?;
            json(
                &crate::commands::run_blocking(move || {
                    logs::read_app_logs_at(&logs::core_app_log_path()?, lines)
                })
                .await?,
            )
        }
        "logs.client.list" => {
            json(&logs::list_client_logs(optional(submission, "directory")).await?)
        }
        "logs.client.tail" => json(
            &logs::read_client_logs(logs::ClientLogRequest {
                directory: optional(submission, "directory"),
                file_name: Some(submission.required("file_name")?.into()),
                max_lines: Some(max_lines(submission)?),
            })
            .await?,
        ),
        "logs.server.tail" => {
            let lines = max_lines(submission)?;
            if lines < 50 {
                return Err(
                    "The shared server log reader requires between 50 and 5000 lines.".into(),
                );
            }
            let data_path = submission.required("tx_data_path")?;
            if !Path::new(data_path).is_absolute() {
                return Err("Choose an absolute txData directory.".into());
            }
            json(
                &fxserver::read_txdata_log(TxDataLogRequest {
                    data_path: data_path.into(),
                    profile: optional(submission, "profile"),
                    log_name: submission.required("log_name")?.into(),
                    max_lines: Some(lines),
                })
                .await?,
            )
        }
        "tools.json.format" => {
            let content = submission.required("content")?;
            if content.len() > 8 * 1024 {
                return Err(
                    "The JSON text form is limited to 8 KiB. Use the file action for larger JSON."
                        .into(),
                );
            }
            format_json(content, true)
        }
        "tools.json.file" => {
            let path = submission.required("path")?.to_string();
            let format = match submission.required("mode")? {
                "Validate" => false,
                "Format" => true,
                _ => return Err("Choose Validate or Format.".into()),
            };
            crate::commands::run_blocking(move || {
                if !Path::new(&path).is_absolute() {
                    return Err("Choose an absolute JSON file path.".into());
                }
                history::ensure_unlinked_path(Path::new(&path))?;
                format_json(&history::read_bounded_config(Path::new(&path))?, format)
            })
            .await
        }
        "tools.jooat.status" => json(&jooat::resolver_status().await?),
        "tools.jooat.resolve" => {
            let queries = submission
                .required("queries")?
                .split_whitespace()
                .map(str::to_string)
                .collect();
            json(&jooat::resolve_offline(queries).await?)
        }
        _ => Err("Unknown config, tools or logs action.".into()),
    }
}

fn helper_change(submission: &Submission) -> Result<config_validation::ConfigHelperChange, String> {
    if submission.id.starts_with("config.helper.value.") {
        return Ok(config_validation::ConfigHelperChange::Value {
            setting: submission.required("setting")?.into(),
            value: submission.required("value")?.into(),
        });
    }
    match submission.required("kind")? {
        "ACE" => Ok(config_validation::ConfigHelperChange::Ace {
            principal: submission.required("principal")?.into(),
            object: submission.required("target")?.into(),
            access: submission.required("access")?.into(),
        }),
        "Principal inheritance" => Ok(config_validation::ConfigHelperChange::Principal {
            child: submission.required("principal")?.into(),
            parent: submission.required("target")?.into(),
        }),
        _ => Err("Choose ACE or Principal inheritance.".into()),
    }
}

fn config_request(submission: &Submission) -> Result<history::ConfigFileRequest, String> {
    Ok(history::ConfigFileRequest {
        tx_data_path: submission.required("tx_data_path")?.into(),
        profile: submission.required("profile")?.into(),
        path: submission.required("path")?.into(),
    })
}

fn config_metadata(file: &ServerConfigFile) -> Value {
    value!({
        "name": file.name, "path": file.path, "size": file.size,
        "modified": file.modified, "revision": history::content_revision(&file.content),
    })
}

fn optional(submission: &Submission, key: &str) -> Option<String> {
    let value = submission.get(key).trim();
    (!value.is_empty()).then(|| value.into())
}

fn max_lines(submission: &Submission) -> Result<usize, String> {
    let value = submission.number("max_lines")?;
    if !(1..=5000).contains(&value) {
        return Err("Maximum lines must be between 1 and 5000.".into());
    }
    Ok(value)
}

fn revision(submission: &Submission) -> Result<String, String> {
    let revision = submission.required("revision")?;
    if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Use the 64-character current file revision from config read/list, not a history or validation revision.".into());
    }
    Ok(revision.to_ascii_lowercase())
}

fn format_json(content: &str, format: bool) -> Result<Outcome, String> {
    if content.len() > MAX_JSON_BYTES {
        return Err("JSON input is limited to 512 KiB.".into());
    }
    let parsed: Value = serde_json::from_str(content).map_err(|error| {
        format!(
            "Invalid JSON at line {}, column {}.",
            error.line(),
            error.column()
        )
    })?;
    if !format {
        return Ok(Outcome::Text(format!(
            "JSON is valid ({} bytes). No file was changed.",
            content.len()
        )));
    }
    let output =
        serde_json::to_string_pretty(&parsed).map_err(|_| "JSON could not be formatted.")?;
    if output.len() > MAX_FORMATTED_BYTES {
        return Err("Formatted JSON exceeds the 2 MiB display limit.".into());
    }
    Ok(Outcome::Text(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "fxi-tools-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn context() -> Context {
        Context {
            settings: crate::terminal::settings::Settings::default(),
            credentials: crate::models::mariadb::MariaDBCredentials {
                host: "localhost".into(),
                port: 3306,
                username: "fixture".into(),
                password: String::new(),
                database: None,
            },
            rcon: crate::models::fxserver::FxserverRconConfig {
                host: "localhost".into(),
                port: 30120,
                password: String::new(),
            },
            manager: fxserver::FxserverManager::default(),
            progress: None,
        }
    }

    fn submission(id: &str, entries: &[(&str, &str)]) -> Submission {
        Submission {
            id: id.into(),
            values: entries
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect(),
        }
    }

    fn text(outcome: Outcome) -> String {
        let Outcome::Text(text) = outcome else {
            panic!("Expected a text tools outcome")
        };
        text
    }

    #[test]
    fn action_catalog_has_unique_ids_and_requires_restore_confirmation() {
        let actions = list(&context());
        let ids: std::collections::HashSet<_> = actions.iter().map(|action| action.id).collect();
        assert_eq!(ids.len(), actions.len());
        let restore = actions
            .iter()
            .find(|action| action.id == "config.history.restore")
            .unwrap();
        assert!(restore.confirmation.is_some());
        assert!(restore
            .fields
            .iter()
            .any(|field| field.key == "revision" && field.required));
        assert_eq!(
            actions
                .iter()
                .filter(|action| action.confirmation.is_some())
                .count(),
            3
        );
    }

    fn config_mutations(tx_data: &Path, path: &Path, content: &str) -> Vec<Submission> {
        let tx_data = tx_data.to_string_lossy();
        let path = path.to_string_lossy();
        let revision = history::content_revision(content);
        [
            "config.helper.value.apply",
            "config.helper.permission.apply",
            "config.history.restore",
        ]
        .into_iter()
        .map(|id| {
            submission(
                id,
                &[
                    ("tx_data_path", tx_data.as_ref()),
                    ("profile", "default"),
                    ("path", path.as_ref()),
                    ("revision", &revision),
                    ("validation_revision", &revision),
                    ("version_id", "fixture-history-version"),
                    ("setting", "sv_hostname"),
                    ("value", "Changed"),
                    ("kind", "ACE"),
                    ("principal", "resource.fixture"),
                    ("target", "command.fixture"),
                    ("access", "allow"),
                ],
            )
        })
        .collect()
    }

    #[tokio::test]
    async fn config_mutations_reject_other_profiles_before_acquiring_a_lease() {
        let fixture = Fixture::new();
        let tx_data = fixture.0.join("txData");
        let path = fixture.0.join("server.cfg");
        let original = "sv_hostname Original\n";
        fs::write(&path, original).unwrap();
        let mut ctx = context();
        ctx.settings.tx_data_path = tx_data.to_string_lossy().into();
        // A lease attempt would fail differently: this artifact folder does not exist.
        ctx.settings.artifact_path = fixture.0.join("missing-artifacts").to_string_lossy().into();
        for action in config_mutations(&tx_data, &path, original) {
            for key in ["tx_data_path", "profile"] {
                let mut mismatched = action.clone();
                mismatched.values.insert(key.into(), "other-target".into());
                let error = execute(&ctx, &mismatched)
                    .await
                    .err()
                    .expect("Mutation must fail");
                assert!(
                    error.contains("Select this txData folder and profile in Settings"),
                    "{}: {error}",
                    action.id
                );
                assert_eq!(fs::read_to_string(&path).unwrap(), original);
            }
        }
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn config_mutations_respect_an_existing_artifact_lease() {
        let fixture = Fixture::new();
        let artifact = fixture.0.join("artifacts");
        let tx_data = fixture.0.join("txData");
        let path = fixture.0.join("server.cfg");
        let original = "sv_hostname Original\n";
        fs::create_dir(&artifact).unwrap();
        fs::write(&path, original).unwrap();
        let mut ctx = context();
        ctx.settings.artifact_path = artifact.to_string_lossy().into();
        ctx.settings.tx_data_path = tx_data.to_string_lossy().into();
        // Two independent handles exercise the cross-process lock without launching a server.
        let lease = crate::platform::ServerLease::acquire(&artifact).unwrap();
        for action in config_mutations(&tx_data, &path, original) {
            let error = execute(&ctx, &action)
                .await
                .err()
                .expect("Mutation must fail");
            assert!(
                error.contains("already managed by another desktop or terminal session"),
                "{}: {error}",
                action.id
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            assert_eq!(fs::read_dir(&artifact).unwrap().count(), 0);
        }
        drop(lease);
        assert!(crate::platform::ServerLease::acquire(&artifact).is_ok());
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
    }

    #[tokio::test]
    async fn config_actions_only_reveal_contents_on_explicit_read() {
        let fixture = Fixture::new();
        let data = fixture.0.join("data");
        let tx = fixture.0.join("txData");
        fs::create_dir_all(tx.join("default")).unwrap();
        fs::create_dir(&data).unwrap();
        fs::write(
            tx.join("default/config.json"),
            value!({"dataPath": data}).to_string(),
        )
        .unwrap();
        let path = data.join("server.cfg");
        fs::write(
            &path,
            "rcon_password fixture-secret\nset marker \"\u{1b}[31m\"\n",
        )
        .unwrap();
        let ctx = context();
        let tx_string = tx.to_string_lossy();
        let path_string = path.to_string_lossy();
        let fields = [
            ("tx_data_path", tx_string.as_ref()),
            ("profile", "default"),
            ("path", path_string.as_ref()),
        ];
        for id in ["config.list", "config.validate"] {
            let output = text(execute(&ctx, &submission(id, &fields)).await.unwrap());
            assert!(!output.contains("fixture-secret"));
            assert!(!output.contains('\u{1b}'));
        }
        let output = text(
            execute(&ctx, &submission("config.read", &fields))
                .await
                .unwrap(),
        );
        assert!(output.contains("fixture-secret"));
        assert!(!output.contains('\u{1b}'));
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
    }

    #[tokio::test]
    async fn json_file_actions_are_read_only_and_bounded() {
        let fixture = Fixture::new();
        let path = fixture.0.join("input.json");
        let source = r#"{"token":"fixture-secret"}"#;
        fs::write(&path, source).unwrap();
        let path = path.to_string_lossy();
        let ctx = context();
        let output = text(
            execute(
                &ctx,
                &submission("tools.json.file", &[("path", &path), ("mode", "Validate")]),
            )
            .await
            .unwrap(),
        );
        assert!(!output.contains("fixture-secret"));
        let output = text(
            execute(
                &ctx,
                &submission("tools.json.file", &[("path", &path), ("mode", "Format")]),
            )
            .await
            .unwrap(),
        );
        assert!(output.contains("fixture-secret"));
        assert_eq!(fs::read_to_string(path.as_ref()).unwrap(), source);
        fs::write(path.as_ref(), " ".repeat(MAX_JSON_BYTES + 1)).unwrap();
        assert!(execute(
            &ctx,
            &submission("tools.json.file", &[("path", &path), ("mode", "Validate")])
        )
        .await
        .is_err());
        assert!(execute(&ctx, &submission("tools.unknown", &[]))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn server_log_action_uses_shared_selection_and_tail_limits() {
        let fixture = Fixture::new();
        let logs = fixture.0.join("default/logs");
        fs::create_dir_all(&logs).unwrap();
        let source = (0..100)
            .map(|line| format!("entry-{line}\n"))
            .collect::<String>();
        fs::write(logs.join("fxserver.log"), &source).unwrap();
        let root = fixture.0.to_string_lossy();
        let ctx = context();
        let fields = [
            ("tx_data_path", root.as_ref()),
            ("profile", "default"),
            ("log_name", "fxserver.log"),
            ("max_lines", "50"),
        ];
        let output: Value = serde_json::from_str(&text(
            execute(&ctx, &submission("logs.server.tail", &fields))
                .await
                .unwrap(),
        ))
        .unwrap();
        let content = output["content"].as_str().unwrap();
        assert_eq!(content.lines().count(), 50);
        assert!(content.starts_with("entry-50\n"));
        assert_eq!(
            fs::read_to_string(logs.join("fxserver.log")).unwrap(),
            source
        );
        let mut invalid = submission("logs.server.tail", &fields);
        invalid.values.insert("max_lines".into(), "49".into());
        assert!(execute(&ctx, &invalid).await.is_err());
        invalid.values.insert("max_lines".into(), "50".into());
        invalid
            .values
            .insert("log_name".into(), "../outside.log".into());
        assert!(execute(&ctx, &invalid).await.is_err());
    }

    #[test]
    fn config_metadata_excludes_secrets_but_carries_the_current_revision() {
        let file = ServerConfigFile {
            name: "server.cfg".into(),
            path: "/fixture/server.cfg".into(),
            content: "rcon_password fixture-secret".into(),
            size: 28,
            modified: None,
            has_rcon_password: true,
            has_rconlog: false,
        };
        let metadata = config_metadata(&file);
        assert!(metadata.get("content").is_none());
        assert!(!metadata.to_string().contains("fixture-secret"));
        assert_eq!(
            metadata["revision"],
            history::content_revision(&file.content)
        );
    }

    #[test]
    fn json_validation_never_echoes_values_and_formatting_escapes_controls() {
        let content = r#"{"password":"fixture-secret","control":"\u001b[31m"}"#;
        assert!(!text(format_json(content, false).unwrap()).contains("fixture-secret"));
        let output = text(format_json(content, true).unwrap());
        assert!(output.contains("fixture-secret"));
        assert!(!output.contains('\u{1b}'));
        assert!(!format_json(r#"{"fixture-secret": }"#, false)
            .err()
            .unwrap()
            .contains("fixture-secret"));
        assert!(format_json(&" ".repeat(MAX_JSON_BYTES + 1), true).is_err());
    }

    #[test]
    fn tail_and_revision_inputs_are_strictly_bounded() {
        for value in ["0", "5001", "-1", "not-a-number"] {
            assert!(max_lines(&submission(
                "logs.application.tail",
                &[("max_lines", value)]
            ))
            .is_err());
        }
        assert_eq!(
            max_lines(&submission("", &[("max_lines", "1")])).unwrap(),
            1
        );
        assert!(revision(&submission("", &[("revision", "stale")])).is_err());
        let upper = "A".repeat(64);
        assert_eq!(
            revision(&submission("", &[("revision", &upper)])).unwrap(),
            "a".repeat(64)
        );
    }
}
