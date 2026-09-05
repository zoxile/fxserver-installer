use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Name(String),
    Text(String),
    Number(String),
    Symbol(char),
    Executable(String),
}

#[derive(Debug)]
pub struct DumpPlan {
    pub tables: Vec<String>,
    pub statements: usize,
}

fn refusal() -> String {
    "Restore test refused: the dump is outside the supported isolated-table SQL subset. Cross-schema names, database directives, routines, triggers, events, views, generated expressions, external engines and client commands are not executed. No SQL was sent.".into()
}

// This is a rejecting grammar, not a SQL rewriter. Only validated original bytes are imported.
fn tokens(sql: &str) -> Result<Vec<Token>, String> {
    let bytes = sql.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'#'
            || (bytes[i..].starts_with(b"--")
                && bytes.get(i + 2).is_some_and(u8::is_ascii_whitespace))
        {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            let start = i + 2;
            let end = sql[start..]
                .find("*/")
                .map(|n| start + n)
                .ok_or_else(refusal)?;
            let body = &sql[start..end];
            if body.contains("/*") || body.starts_with('+') {
                return Err(refusal());
            }
            if body == "M!999999\\- enable the sandbox mode " && result.is_empty() {
                i = end + 2;
                continue;
            }
            if body.starts_with('!') || body.starts_with("M!") {
                result.push(Token::Executable(body.into()));
            }
            i = end + 2;
            continue;
        }
        if c == b'`' || c == b'\'' {
            let start = i;
            i += 1;
            let mut value = String::new();
            let mut segment = i;
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i] == c {
                    value.push_str(&sql[segment..i]);
                    if bytes.get(i + 1) == Some(&c) {
                        value.push(c as char);
                        i += 2;
                        segment = i;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                if c == b'\'' && bytes[i] == b'\\' {
                    value.push_str(&sql[segment..i]);
                    let escaped = *bytes.get(i + 1).ok_or_else(refusal)?;
                    if !escaped.is_ascii() {
                        return Err(refusal());
                    }
                    value.push('\\');
                    value.push(escaped as char);
                    i += 2;
                    segment = i;
                    continue;
                }
                if bytes[i] == 0 {
                    return Err(refusal());
                }
                i += 1;
            }
            if !closed || i == start {
                return Err(refusal());
            }
            if c == b'`' {
                if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                    return Err(refusal());
                }
                result.push(Token::Name(value));
            } else {
                result.push(Token::Text(value));
            }
        } else if c.is_ascii_digit() {
            let start = i;
            if bytes[i..].starts_with(b"0x") || bytes[i..].starts_with(b"0X") {
                i += 2;
                let digits = i;
                while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                if digits == i {
                    return Err(refusal());
                }
            } else {
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'.') {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if bytes.get(i).is_some_and(|b| *b == b'e' || *b == b'E') {
                    i += 1;
                    if bytes.get(i).is_some_and(|b| *b == b'+' || *b == b'-') {
                        i += 1;
                    }
                    let digits = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if digits == i {
                        return Err(refusal());
                    }
                }
            }
            result.push(Token::Number(sql[start..i].into()));
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            result.push(Token::Word(sql[start..i].to_ascii_uppercase()));
        } else if b"(),;=+-@".contains(&c) {
            result.push(Token::Symbol(c as char));
            i += 1;
        } else {
            return Err(refusal());
        }
        if result.len() > 2_000_000 {
            return Err("Restore test refused: dump has too many SQL tokens.".into());
        }
    }
    Ok(result)
}

struct Parser<'a> {
    input: &'a [Token],
    pos: usize,
}
impl<'a> Parser<'a> {
    fn take(&mut self) -> Result<&'a Token, String> {
        let token = self.input.get(self.pos).ok_or_else(refusal)?;
        self.pos += 1;
        Ok(token)
    }
    fn eat(&mut self, word: &str) -> bool {
        if self.input.get(self.pos) == Some(&Token::Word(word.into())) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn word(&mut self, word: &str) -> Result<(), String> {
        if self.eat(word) {
            Ok(())
        } else {
            Err(refusal())
        }
    }
    fn symbol(&mut self, symbol: char) -> bool {
        if self.input.get(self.pos) == Some(&Token::Symbol(symbol)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn require(&mut self, symbol: char) -> Result<(), String> {
        if self.symbol(symbol) {
            Ok(())
        } else {
            Err(refusal())
        }
    }
    fn name(&mut self) -> Result<String, String> {
        match self.take()? {
            Token::Name(value) => Ok(value.clone()),
            _ => Err(refusal()),
        }
    }
    fn number(&mut self) -> Result<(), String> {
        match self.take()? {
            Token::Number(value) if value.bytes().all(|b| b.is_ascii_digit()) => Ok(()),
            _ => Err(refusal()),
        }
    }
    fn text(&mut self) -> Result<(), String> {
        match self.take()? {
            Token::Text(_) => Ok(()),
            _ => Err(refusal()),
        }
    }
    fn literal(&mut self) -> Result<(), String> {
        let signed = self.symbol('-') || self.symbol('+');
        match self.take()? {
            Token::Number(_) => Ok(()),
            Token::Text(_) if !signed => Ok(()),
            Token::Word(word) if !signed && word == "NULL" => Ok(()),
            _ => Err(refusal()),
        }
    }
    fn charset(&mut self) -> Result<(), String> {
        match self.take()? {
            Token::Word(value)
                if ["UTF8", "UTF8MB3", "UTF8MB4", "ASCII", "LATIN1", "BINARY"]
                    .contains(&value.as_str()) =>
            {
                Ok(())
            }
            _ => Err(refusal()),
        }
    }
    fn collation(&mut self) -> Result<(), String> {
        match self.take()? {
            Token::Word(value)
                if ["UTF8_", "UTF8MB3_", "UTF8MB4_", "ASCII_", "LATIN1_"]
                    .iter()
                    .any(|prefix| value.starts_with(prefix))
                    || value == "BINARY" =>
            {
                Ok(())
            }
            _ => Err(refusal()),
        }
    }
    fn names(&mut self) -> Result<(), String> {
        self.require('(')?;
        loop {
            self.name()?;
            if self.symbol('(') {
                self.number()?;
                self.require(')')?;
            }
            if self.eat("ASC") {
            } else {
                self.eat("DESC");
            }
            if !self.symbol(',') {
                break;
            }
        }
        self.require(')')
    }
    fn timestamp(&mut self) -> Result<(), String> {
        self.word("CURRENT_TIMESTAMP")?;
        if self.symbol('(') && !self.symbol(')') {
            self.number()?;
            self.require(')')?;
        }
        Ok(())
    }
    fn column(&mut self) -> Result<(), String> {
        self.name()?;
        let kind = match self.take()? {
            Token::Word(value) => value.as_str(),
            _ => return Err(refusal()),
        };
        if ![
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "INT",
            "INTEGER",
            "BIGINT",
            "DECIMAL",
            "NUMERIC",
            "FLOAT",
            "DOUBLE",
            "REAL",
            "BIT",
            "BOOL",
            "BOOLEAN",
            "CHAR",
            "VARCHAR",
            "BINARY",
            "VARBINARY",
            "TINYBLOB",
            "BLOB",
            "MEDIUMBLOB",
            "LONGBLOB",
            "TINYTEXT",
            "TEXT",
            "MEDIUMTEXT",
            "LONGTEXT",
            "DATE",
            "DATETIME",
            "TIMESTAMP",
            "TIME",
            "YEAR",
            "ENUM",
            "SET",
            "JSON",
        ]
        .contains(&kind)
        {
            return Err(refusal());
        }
        if self.symbol('(') {
            loop {
                if ["ENUM", "SET"].contains(&kind) {
                    self.text()?;
                } else {
                    self.number()?;
                }
                if !self.symbol(',') {
                    break;
                }
            }
            self.require(')')?;
        }
        loop {
            if self.eat("NOT") {
                self.word("NULL")?;
            } else if self.eat("NULL")
                || self.eat("UNSIGNED")
                || self.eat("ZEROFILL")
                || self.eat("AUTO_INCREMENT")
            {
            } else if self.eat("DEFAULT") {
                if self.input.get(self.pos) == Some(&Token::Word("CURRENT_TIMESTAMP".into())) {
                    self.timestamp()?;
                } else {
                    self.literal()?;
                }
            } else if self.eat("ON") {
                self.word("UPDATE")?;
                self.timestamp()?;
            } else if self.eat("CHARACTER") {
                self.word("SET")?;
                self.charset()?;
            } else if self.eat("COLLATE") {
                self.collation()?;
            } else if self.eat("COMMENT") {
                self.text()?;
            } else {
                break;
            }
        }
        Ok(())
    }
    fn key(&mut self) -> Result<(), String> {
        if self.eat("PRIMARY") {
            self.word("KEY")?;
        } else {
            self.eat("UNIQUE");
            if !self.eat("KEY") {
                self.word("INDEX")?;
            }
            self.name()?;
        }
        self.names()?;
        if self.eat("USING") && !self.eat("BTREE") && !self.eat("HASH") {
            return Err(refusal());
        }
        Ok(())
    }
    fn create(&mut self) -> Result<String, String> {
        self.word("TABLE")?;
        let name = self.name()?;
        self.require('(')?;
        loop {
            if matches!(self.input.get(self.pos), Some(Token::Name(_))) {
                self.column()?;
            } else {
                self.key()?;
            }
            if !self.symbol(',') {
                break;
            }
        }
        self.require(')')?;
        let mut innodb = false;
        loop {
            if self.eat("ENGINE") {
                self.symbol('=');
                self.word("INNODB")?;
                innodb = true;
            } else if self.eat("AUTO_INCREMENT") {
                self.symbol('=');
                self.number()?;
            } else if self.eat("DEFAULT") {
                self.word("CHARSET")?;
                self.symbol('=');
                self.charset()?;
            } else if self.eat("CHARSET") {
                self.symbol('=');
                self.charset()?;
            } else if self.eat("COLLATE") {
                self.symbol('=');
                self.collation()?;
            } else if self.eat("COMMENT") {
                self.symbol('=');
                self.text()?;
            } else if self.eat("ROW_FORMAT") {
                self.symbol('=');
                if !self.eat("DYNAMIC")
                    && !self.eat("COMPACT")
                    && !self.eat("REDUNDANT")
                    && !self.eat("COMPRESSED")
                {
                    return Err(refusal());
                }
            } else {
                break;
            }
        }
        if !innodb {
            return Err(refusal());
        }
        Ok(name)
    }
}

fn safe_set(input: &[Token]) -> bool {
    let mut allowed = vec![
        "SET NAMES utf8mb4".to_string(),
        "SET NAMES utf8".into(),
        "SET TIME_ZONE='+00:00'".into(),
        "SET character_set_client=utf8mb4".into(),
        "SET character_set_client=utf8".into(),
        "SET @saved_cs_client=@@character_set_client".into(),
        "SET character_set_client=@saved_cs_client".into(),
    ];
    for key in [
        "CHARACTER_SET_CLIENT",
        "CHARACTER_SET_RESULTS",
        "COLLATION_CONNECTION",
        "TIME_ZONE",
        "UNIQUE_CHECKS",
        "FOREIGN_KEY_CHECKS",
        "SQL_MODE",
        "SQL_NOTES",
    ] {
        allowed.push(format!("SET @OLD_{key}=@@{key}"));
        allowed.push(format!("SET {key}=@OLD_{key}"));
    }
    for (key, value) in [
        ("UNIQUE_CHECKS", "0"),
        ("FOREIGN_KEY_CHECKS", "0"),
        ("SQL_MODE", "'NO_AUTO_VALUE_ON_ZERO'"),
        ("SQL_NOTES", "0"),
    ] {
        allowed.push(format!("SET @OLD_{key}=@@{key},{key}={value}"));
    }
    allowed
        .iter()
        .any(|sql| tokens(sql).is_ok_and(|value| value == input))
}

pub fn preflight(sql: &str) -> Result<DumpPlan, String> {
    if sql.len() > 32 * 1024 * 1024 {
        return Err("Restore test supports SQL snapshots up to 32 MiB. No SQL was sent.".into());
    }
    let input = tokens(sql)?;
    let mut created = BTreeSet::new();
    let mut touched = BTreeSet::new();
    let mut statements = 0;
    for (index, statement) in input.split(|t| *t == Token::Symbol(';')).enumerate() {
        if statement.is_empty() {
            continue;
        }
        let expanded;
        let statement = if let [Token::Executable(body)] = statement {
            if index == 0 && body == "M!999999\\- enable the sandbox mode " {
                continue;
            }
            let body = body
                .strip_prefix("M!")
                .or_else(|| body.strip_prefix('!'))
                .ok_or_else(refusal)?;
            let count = body.bytes().take_while(u8::is_ascii_digit).count();
            if !(5..=6).contains(&count)
                || !body
                    .as_bytes()
                    .get(count)
                    .is_some_and(u8::is_ascii_whitespace)
            {
                return Err(refusal());
            }
            expanded = tokens(body[count..].trim())?;
            expanded.as_slice()
        } else {
            statement
        };
        if statement.iter().any(|t| matches!(t, Token::Executable(_))) {
            return Err(refusal());
        }
        let mut parser = Parser {
            input: statement,
            pos: 0,
        };
        if parser.eat("CREATE") {
            let table = parser.create()?;
            if !created.insert(table.to_ascii_lowercase()) {
                return Err(refusal());
            }
            touched.insert(table.to_ascii_lowercase());
        } else if parser.eat("INSERT") {
            parser.word("INTO")?;
            touched.insert(parser.name()?.to_ascii_lowercase());
            if parser.input.get(parser.pos) == Some(&Token::Symbol('(')) {
                parser.names()?;
            }
            parser.word("VALUES")?;
            loop {
                parser.require('(')?;
                loop {
                    parser.literal()?;
                    if !parser.symbol(',') {
                        break;
                    }
                }
                parser.require(')')?;
                if !parser.symbol(',') {
                    break;
                }
            }
        } else if parser.eat("DROP") {
            parser.word("TABLE")?;
            parser.word("IF")?;
            parser.word("EXISTS")?;
            touched.insert(parser.name()?.to_ascii_lowercase());
        } else if parser.eat("LOCK") {
            parser.word("TABLES")?;
            loop {
                touched.insert(parser.name()?.to_ascii_lowercase());
                parser.word("WRITE")?;
                if !parser.symbol(',') {
                    break;
                }
            }
        } else if parser.eat("UNLOCK") {
            parser.word("TABLES")?;
        } else if parser.eat("ALTER") {
            parser.word("TABLE")?;
            touched.insert(parser.name()?.to_ascii_lowercase());
            if !parser.eat("DISABLE") {
                parser.word("ENABLE")?;
            }
            parser.word("KEYS")?;
        } else if safe_set(statement) {
            parser.pos = statement.len();
        } else {
            return Err(refusal());
        }
        if parser.pos != statement.len() {
            return Err(refusal());
        }
        statements += 1;
        if created.len() > 256 {
            return Err("Restore test supports at most 256 tables.".into());
        }
    }
    if created.is_empty() || !touched.is_subset(&created) {
        return Err(refusal());
    }
    Ok(DumpPlan {
        tables: created.into_iter().collect(),
        statements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAFE: &str = include_str!("../../../tests/fixtures/restore-safe.sql");
    #[test]
    fn ordinary_dump_is_accepted_without_rewriting() {
        let plan = preflight(SAFE).unwrap();
        assert_eq!(plan.tables, ["players"]);
        assert!(plan.statements > 5);
    }
    #[test]
    fn schema_and_execution_escape_fixtures_are_refused() {
        for extra in include_str!("../../../tests/fixtures/restore-unsafe.txt")
            .lines()
            .filter(|line| !line.is_empty())
        {
            assert!(
                preflight(&format!("{SAFE}\n{extra}")).is_err(),
                "accepted {extra}"
            );
        }
    }
    #[test]
    fn ambiguous_modes_comments_and_malformed_literals_are_refused() {
        for value in [
            "SET SQL_MODE='NO_BACKSLASH_ESCAPES';",
            "SET NAMES gbk;",
            "/*!50000 DROP DATABASE production */;",
            "INSERT INTO `players` VALUES (sleep(10));",
            "INSERT INTO `players` VALUES ('unterminated);",
            "INSERT INTO `players` VALUES (1e);",
            "CREATE TABLE `x` (`a` int DEFAULT (evil())) ENGINE=InnoDB;",
            "CREATE TABLE `x` (`a` int) ENGINE=CONNECT;",
            "INSERT INTO `other` VALUES(1);",
            "/*!40101 SET NAMES utf8mb4 */ SELECT 1;",
        ] {
            assert!(
                preflight(&format!("{SAFE}\n{value}")).is_err(),
                "accepted {value}"
            );
        }
    }
}
