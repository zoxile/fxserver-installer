pub fn escape_identifier(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Identifier cannot be empty.".to_string());
    }

    Ok(format!("`{}`", value.replace('`', "``")))
}

pub fn escape_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

pub fn normalize_privileges(privileges: Vec<String>) -> String {
    let normalized: Vec<String> = privileges
        .into_iter()
        .map(|privilege| privilege.trim().to_uppercase())
        .filter(|privilege| !privilege.is_empty())
        .collect();

    if normalized.is_empty() {
        "ALL PRIVILEGES".to_string()
    } else {
        normalized.join(", ")
    }
}
