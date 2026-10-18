use super::settings::Settings;
use crate::{
    commands::fxserver::FxserverManager,
    models::{fxserver::FxserverRconConfig, mariadb::MariaDBCredentials},
};
use std::collections::BTreeMap;

pub(super) mod database;
pub(super) mod server;
pub(super) mod tools;

#[derive(Clone)]
pub(super) struct Context {
    pub settings: Settings,
    pub credentials: MariaDBCredentials,
    pub rcon: FxserverRconConfig,
    pub manager: FxserverManager,
    pub progress: Option<std::sync::mpsc::SyncSender<String>>,
}

impl Context {
    pub fn report(&self, text: &str, extra_secrets: &[&str]) {
        if let Some(sender) = &self.progress {
            let mut secrets = vec![
                self.credentials.password.as_str(),
                self.rcon.password.as_str(),
            ];
            secrets.extend_from_slice(extra_secrets);
            let _ = sender.try_send(crate::terminal::safe_text(&redact(text, &secrets)));
        }
    }
}

pub(super) fn redact(text: &str, secrets: &[&str]) -> String {
    let mut secrets: Vec<_> = secrets
        .iter()
        .copied()
        .filter(|value| !value.is_empty())
        .collect();
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let mut text = text.to_owned();
    for secret in secrets {
        if let Ok(encoded) = serde_json::to_string(secret) {
            text = text.replace(&encoded[1..encoded.len() - 1], "[redacted]");
        }
        text = text.replace(secret, "[redacted]");
    }
    text
}

#[derive(Clone)]
pub(super) enum FieldKind {
    Text,
    Secret,
    Choice(Vec<String>),
}

#[derive(Clone)]
pub(super) struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
    pub kind: FieldKind,
    pub required: bool,
}

impl Field {
    pub fn text(key: &'static str, label: &'static str, value: impl Into<String>) -> Self {
        Self {
            key,
            label,
            value: value.into(),
            kind: FieldKind::Text,
            required: true,
        }
    }
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }
    pub fn secret(mut self) -> Self {
        self.kind = FieldKind::Secret;
        self
    }
    pub fn choice(mut self, values: &[&str]) -> Self {
        self.kind = FieldKind::Choice(values.iter().map(|s| (*s).into()).collect());
        self
    }
}

#[derive(Clone)]
pub(super) struct Action {
    pub id: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub fields: Vec<Field>,
    pub confirmation: Option<&'static str>,
}

#[derive(Clone)]
pub(super) struct Submission {
    pub id: String,
    pub values: BTreeMap<String, String>,
}

impl Submission {
    pub fn get(&self, key: &str) -> &str {
        self.values.get(key).map(String::as_str).unwrap_or("")
    }
    pub fn required(&self, key: &str) -> Result<&str, String> {
        let value = self.get(key).trim();
        if value.is_empty() {
            Err(format!("{key} is required."))
        } else {
            Ok(value)
        }
    }
    pub fn number<T: std::str::FromStr>(&self, key: &str) -> Result<T, String> {
        self.required(key)?
            .parse()
            .map_err(|_| format!("{key} must be a valid number."))
    }
}

pub(super) enum Outcome {
    Text(String),
    Login(MariaDBCredentials),
    RconPassword(String),
}

pub(super) fn list(context: &Context) -> Vec<Action> {
    let mut actions = database::list(context);
    actions.extend(server::list(context));
    actions.extend(tools::list(context));
    actions
}

pub(super) async fn execute(context: &Context, submission: &Submission) -> Result<Outcome, String> {
    match submission.id.split('.').next() {
        Some("database") => database::execute(context, submission).await,
        Some("server" | "resource" | "artifact") => server::execute(context, submission).await,
        Some("config" | "tools" | "logs") => tools::execute(context, submission).await,
        _ => Err("Unknown terminal action.".into()),
    }
}

pub(super) fn json(value: &impl serde::Serialize) -> Result<Outcome, String> {
    serde_json::to_string_pretty(value)
        .map(Outcome::Text)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            settings: Settings::default(),
            manager: FxserverManager::default(),
            progress: None,
            credentials: MariaDBCredentials {
                host: "localhost".into(),
                port: 3306,
                username: "root".into(),
                password: "private-db".into(),
                database: None,
            },
            rcon: FxserverRconConfig {
                host: "127.0.0.1".into(),
                port: 30120,
                password: "private-rcon".into(),
            },
        }
    }

    #[test]
    fn catalog_is_unique_and_choices_are_valid_without_exposing_session_secrets() {
        let actions = list(&context());
        assert!(actions.len() >= 60);
        let mut ids = std::collections::BTreeSet::new();
        for action in actions {
            assert!(ids.insert(action.id), "Duplicate action {}", action.id);
            let mut fields = std::collections::BTreeSet::new();
            for field in action.fields {
                assert!(
                    fields.insert(field.key),
                    "Duplicate field {} in {}",
                    field.key,
                    action.id
                );
                assert!(!field.value.contains("private-"));
                if let FieldKind::Choice(options) = field.kind {
                    assert!(
                        !options.is_empty() && options.contains(&field.value),
                        "Invalid choice {} in {}",
                        field.key,
                        action.id
                    );
                }
            }
        }
    }

    #[test]
    fn progress_is_bounded_and_redacts_raw_and_json_escaped_secrets() {
        let mut context = context();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        context.progress = Some(send);
        let extra = "test\"password";
        context.report(
            &format!(
                "private-db private-rcon {} {}\x1b",
                extra,
                serde_json::to_string(extra).unwrap()
            ),
            &[extra],
        );
        context.report("This cannot block the worker when the queue is full", &[]);
        let text = receive.try_recv().unwrap();
        assert!(!text.contains("private-") && !text.contains("password") && !text.contains('\x1b'));
        assert!(receive.try_recv().is_err());
    }
}
