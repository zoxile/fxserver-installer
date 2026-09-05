use serde_json::Value;

const REDACTED: &str = "[redacted: potentially sensitive content]";

pub(super) fn sensitive(value: &str) -> bool {
    let compact: String = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "authorization",
        "apikey",
        "licensekey",
        "licencekey",
        "connectionstring",
        "webhook",
        "privatekey",
        "cfxk",
        "signature",
        "githubpat",
        "credential",
        "cookie",
    ]
    .iter()
    .any(|key| compact.contains(key))
        || value.to_ascii_lowercase().contains("mysql://")
        || value.to_ascii_lowercase().contains("mariadb://")
        || compact == "pwd"
        || value.to_ascii_lowercase().contains("pwd=")
        || value.to_ascii_lowercase().contains("pwd:")
        || value.to_ascii_lowercase().contains("bearer ")
        || value.contains("ghp_")
        || (value.contains("://") && value.contains('@'))
}

fn has_absolute_path(value: &str) -> bool {
    value.as_bytes().windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'/' | b'\\')
    }) || value.contains("\\\\")
        || value.contains("/home/")
        || value.contains("/Users/")
}

fn mask_addresses(value: &str) -> String {
    value
        .split_inclusive(char::is_whitespace)
        .map(|part| {
            let word = part.trim_end();
            let candidate = word.trim_matches(|character: char| {
                matches!(character, '[' | ']' | '(' | ')' | ',' | ';' | '\'' | '"')
            });
            if candidate.parse::<std::net::IpAddr>().is_ok()
                || candidate.parse::<std::net::SocketAddr>().is_ok()
            {
                format!("[address]{}", &part[word.len()..])
            } else {
                part.to_string()
            }
        })
        .collect()
}

pub(super) fn text(value: &str) -> String {
    let mut private_key = false;
    value
        .lines()
        .map(|line| {
            private_key |= line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----");
            let in_key = private_key;
            if line.contains("-----END ") && line.contains("PRIVATE KEY-----") {
                private_key = false;
            }
            if in_key || sensitive(line) || has_absolute_path(line) {
                REDACTED.to_string()
            } else {
                mask_addresses(line)
                    .chars()
                    .filter(|character| !character.is_control() || *character == '\t')
                    .collect()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn json(value: &mut Value) {
    match value {
        Value::Object(values) => {
            *values = std::mem::take(values)
                .into_iter()
                .map(|(key, mut value)| {
                    if sensitive(&key) {
                        value = Value::String(REDACTED.to_string());
                    } else {
                        json(&mut value);
                    }
                    (text(&key), value)
                })
                .collect();
        }
        Value::Array(values) => values.iter_mut().for_each(json),
        Value::String(value) => *value = text(value),
        _ => {}
    }
}

pub(super) fn logs(value: &str, secrets: &[String]) -> String {
    // A bounded tail may begin inside a key. An END before any BEGIN means the
    // leading fragment must be hidden too.
    let mut private_key = value
        .lines()
        .find_map(|line| {
            if line.contains("PRIVATE KEY-----") {
                if line.contains("-----END ") {
                    return Some(true);
                }
                if line.contains("-----BEGIN ") {
                    return Some(false);
                }
            }
            None
        })
        .unwrap_or(false);
    value
        .lines()
        .map(|line| {
            private_key |= line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----");
            if private_key {
                if line.contains("-----END ") && line.contains("PRIVATE KEY-----") {
                    private_key = false;
                }
                return REDACTED.to_string();
            }
            if let Ok(mut value) = serde_json::from_str::<Value>(line) {
                super::redact_json_values(&mut value, secrets);
                json(&mut value);
                serde_json::to_string(&value).unwrap_or_else(|_| REDACTED.to_string())
            } else {
                text(&super::redact_known(line, secrets))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_known_secrets_and_sensitive_json_keys_are_redacted() {
        let secret = "quoted\"credential\\value";
        let input = serde_json::json!({ "message": secret, (secret): "value", "password=do-not-share": "ignored" }).to_string();
        let result = logs(&input, &[secret.into()]);
        assert!(!result.contains("credential"));
        assert!(!result.contains("do-not-share"));
        let value: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["message"], "[redacted]");
    }

    #[test]
    fn leading_private_key_fragments_are_hidden() {
        let result = logs("base64-fragment\n-----END RSA PRIVATE KEY-----\nready", &[]);
        assert!(!result.contains("base64-fragment"));
        assert!(result.ends_with("ready"));
    }

    #[test]
    fn drops_secret_lines_instead_of_guessing_password_boundaries() {
        let input = "set rcon_password \"has spaces and # symbols\"\nMYSQL://root:secret@localhost/db\nAuthorization: Bearer abc\nSteam_WebApiKey xyz\nsv_licenseKey cfxk_private\nhttps://example.invalid/?api_key=private\nserver started";
        let result = text(input);
        for secret in [
            "has spaces",
            "root:",
            "Bearer",
            "xyz",
            "cfxk_private",
            "api_key=private",
        ] {
            assert!(!result.contains(secret), "Found {secret}");
        }
        assert!(result.ends_with("server started"));
    }

    #[test]
    fn nested_json_redaction_keeps_valid_json() {
        let line =
            r#"{"message":"started","context":{"password":"abc","nested":[{"token":"xyz"}]}}"#;
        let result = logs(line, &[]);
        assert!(!result.contains("abc"));
        assert!(!result.contains("xyz"));
        assert!(serde_json::from_str::<Value>(&result).is_ok());
    }

    #[test]
    fn hides_machine_paths_and_network_addresses() {
        assert_eq!(
            text(r"Opened C:\Users\Someone\Private Server\server.cfg"),
            REDACTED
        );
        assert_eq!(
            text("Connected to 192.168.1.7:30120"),
            "Connected to [address]"
        );
    }

    #[test]
    fn redacts_multiline_keys_and_embedded_url_credentials() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nprivate-base64-content\n-----END RSA PRIVATE KEY-----\nhttps://user:private@example.invalid/download\nready";
        for result in [text(input), logs(input, &[])] {
            assert!(!result.contains("private-base64-content"));
            assert!(!result.contains("user:private"));
            assert!(result.ends_with("ready"));
        }
    }
}
