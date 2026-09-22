pub fn escape_identifier(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("Identifier cannot be empty or contain control characters.".to_string());
    }

    Ok(format!("`{}`", value.replace('`', "``")))
}

pub fn escape_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

// Only generated statements use this mode. Arbitrary user queries retain their session modes.
pub fn generated_sql(sql: &str) -> String {
    format!("SET SESSION sql_mode = IF(FIND_IN_SET('NO_BACKSLASH_ESCAPES', @@SESSION.sql_mode), @@SESSION.sql_mode, CONCAT_WS(',', NULLIF(@@SESSION.sql_mode, ''), 'NO_BACKSLASH_ESCAPES'));\n{sql}")
}

pub fn normalize_privileges(privileges: Vec<String>) -> Result<String, String> {
    const ALLOWED: &[&str] = &[
        "ALL PRIVILEGES",
        "ALTER",
        "ALTER ROUTINE",
        "CREATE",
        "CREATE ROUTINE",
        "CREATE TEMPORARY TABLES",
        "CREATE VIEW",
        "DELETE",
        "DELETE HISTORY",
        "DROP",
        "EVENT",
        "EXECUTE",
        "INDEX",
        "INSERT",
        "LOCK TABLES",
        "REFERENCES",
        "SELECT",
        "SHOW VIEW",
        "TRIGGER",
        "UPDATE",
        "USAGE",
    ];
    if privileges.is_empty() {
        return Err("Select at least one database privilege.".into());
    }
    let mut normalized = Vec::new();
    for privilege in privileges {
        let value = privilege.trim().to_ascii_uppercase();
        if !ALLOWED.contains(&value.as_str()) {
            return Err(
                "Invalid database privilege. Choose only supported database-scoped privileges."
                    .into(),
            );
        }
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }
    if normalized.len() > 1 && normalized.iter().any(|p| p == "ALL PRIVILEGES") {
        return Err("ALL PRIVILEGES cannot be combined with individual privileges.".into());
    }
    Ok(normalized.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn privileges_are_an_allowlist_not_sql_fragments() {
        for values in [
            vec![],
            vec![""],
            vec!["SELECT, UPDATE"],
            vec!["SELECT ON *.* TO 'attacker'; --"],
            vec!["ALL PRIVILEGES WITH GRANT OPTION"],
            vec!["SUPER"],
            vec!["SELECT/*x*/"],
            vec!["ALL PRIVILEGES", "SELECT"],
        ] {
            assert!(
                normalize_privileges(values.into_iter().map(str::to_string).collect()).is_err()
            );
        }
        assert_eq!(
            normalize_privileges(vec![" select ".into(), "UPDATE".into(), "SELECT".into()])
                .unwrap(),
            "SELECT, UPDATE"
        );
    }
    #[test]
    fn generated_sql_preserves_nondefault_modes_and_literal_backslashes() {
        let attack = "a\\'; DROP USER 'root'@'localhost'; --";
        assert_eq!(
            escape_string(attack),
            "'a\\''; DROP USER ''root''@''localhost''; --'"
        );
        let sql = generated_sql(&format!("SELECT {};", escape_string(attack)));
        assert!(sql.starts_with("SET SESSION sql_mode = IF(FIND_IN_SET("));
        assert!(sql.contains("@@SESSION.sql_mode, CONCAT_WS(',', NULLIF(@@SESSION.sql_mode, '')"));
        assert!(!sql.contains("sql_mode = ''"));
        for mode in [
            "STRICT_TRANS_TABLES,ANSI_QUOTES",
            "NO_BACKSLASH_ESCAPES,STRICT_ALL_TABLES",
        ] {
            let combined = if mode.split(',').any(|v| v == "NO_BACKSLASH_ESCAPES") {
                mode.into()
            } else {
                format!("{mode},NO_BACKSLASH_ESCAPES")
            };
            assert!(combined.starts_with(mode));
            assert_eq!(combined.matches("NO_BACKSLASH_ESCAPES").count(), 1);
        }
    }
}
