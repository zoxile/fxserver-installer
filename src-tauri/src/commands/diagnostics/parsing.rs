#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Literal(String),
    Symbol(char),
}

#[derive(Default, Debug)]
pub(super) struct Manifest {
    pub dependencies: Vec<String>,
    pub provides: Vec<String>,
    pub dynamic: bool,
}

pub(super) fn manifest(source: &str) -> Manifest {
    let tokens = lua_tokens(source);
    let mut result = Manifest::default();
    let mut index = 0;
    while index < tokens.len() {
        let (plural, provides) = match &tokens[index] {
            Token::Word(word) if word == "dependency" => (false, false),
            Token::Word(word) if word == "dependencies" => (true, false),
            Token::Word(word) if word == "provide" => (false, true),
            _ => {
                index += 1;
                continue;
            }
        };
        index += 1;
        if tokens.get(index) == Some(&Token::Symbol('(')) {
            index += 1;
        }
        let values = if plural && tokens.get(index) == Some(&Token::Symbol('{')) {
            index += 1;
            let mut values = Vec::new();
            while index < tokens.len() && tokens[index] != Token::Symbol('}') {
                let start = index;
                let mut nesting = 0_usize;
                while index < tokens.len() {
                    match tokens[index] {
                        Token::Symbol(',' | ';' | '}') if nesting == 0 => break,
                        Token::Symbol('{' | '(' | '[') => nesting += 1,
                        Token::Symbol('}' | ')' | ']') => nesting = nesting.saturating_sub(1),
                        _ => {}
                    }
                    index += 1;
                }
                if let [Token::Literal(value)] = &tokens[start..index] {
                    values.push(value.clone());
                } else if index > start {
                    result.dynamic = true;
                }
                if matches!(tokens.get(index), Some(Token::Symbol(',' | ';'))) {
                    index += 1;
                }
            }
            values
        } else if let Some(Token::Literal(value)) = tokens.get(index) {
            if matches!(
                tokens.get(index + 1),
                Some(Token::Symbol('.' | '+' | '-' | '*' | '/' | '[' | '('))
            ) {
                result.dynamic = true;
                Vec::new()
            } else {
                vec![value.clone()]
            }
        } else {
            result.dynamic = true;
            Vec::new()
        };
        let destination = if provides {
            &mut result.provides
        } else {
            &mut result.dependencies
        };
        destination.extend(values.into_iter().filter(|value| !value.trim().is_empty()));
        index += 1;
    }
    result.dependencies.sort();
    result.dependencies.dedup();
    result.provides.sort();
    result.provides.dedup();
    for values in [&mut result.dependencies, &mut result.provides] {
        if values.len() > 256 || values.iter().any(|value| value.len() > 255) {
            result.dynamic = true;
            values.retain(|value| value.len() <= 255);
            values.truncate(256);
        }
    }
    result
}

fn long_string(source: &[char], start: usize) -> Option<(String, usize)> {
    if source.get(start) != Some(&'[') {
        return None;
    }
    let mut cursor = start + 1;
    while source.get(cursor) == Some(&'=') {
        cursor += 1;
    }
    if source.get(cursor) != Some(&'[') {
        return None;
    }
    let equals = cursor - start - 1;
    let content_start = cursor + 1;
    cursor = content_start;
    while cursor < source.len() {
        if source[cursor] == ']'
            && (0..equals).all(|offset| source.get(cursor + offset + 1) == Some(&'='))
            && source.get(cursor + equals + 1) == Some(&']')
        {
            return Some((
                source[content_start..cursor].iter().collect(),
                cursor + equals + 2,
            ));
        }
        cursor += 1;
    }
    Some((source[content_start..].iter().collect(), source.len()))
}

fn lua_tokens(source: &str) -> Vec<Token> {
    let source: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < source.len() {
        let character = source[index];
        if character == '-' && source.get(index + 1) == Some(&'-') {
            index += 2;
            if let Some((_, end)) = long_string(&source, index) {
                index = end;
            } else {
                while index < source.len() && source[index] != '\n' {
                    index += 1;
                }
            }
        } else if let Some((value, end)) = long_string(&source, index) {
            tokens.push(Token::Literal(value));
            index = end;
        } else if matches!(character, '\'' | '"') {
            index += 1;
            let mut value = String::new();
            while index < source.len() && source[index] != character {
                if source[index] == '\\' && index + 1 < source.len() {
                    index += 1;
                }
                value.push(source[index]);
                index += 1;
            }
            tokens.push(Token::Literal(value));
            index += 1;
        } else if character.is_ascii_alphabetic() || character == '_' {
            let start = index;
            while index < source.len()
                && (source[index].is_ascii_alphanumeric() || source[index] == '_')
            {
                index += 1;
            }
            tokens.push(Token::Word(source[start..index].iter().collect()));
        } else {
            if !character.is_whitespace() {
                tokens.push(Token::Symbol(character));
            }
            index += 1;
        }
    }
    tokens
}

pub(super) fn config_commands(source: &str) -> Vec<(usize, Vec<String>)> {
    let mut commands = Vec::new();
    for (line, source) in source.lines().enumerate() {
        let mut words = Vec::new();
        let mut word = String::new();
        let mut quote = None;
        let mut chars = source.trim_start_matches('\u{feff}').chars().peekable();
        while let Some(character) = chars.next() {
            if let Some(delimiter) = quote {
                if character == delimiter {
                    quote = None;
                } else if character == '\\' && chars.peek() == Some(&delimiter) {
                    word.push(chars.next().unwrap_or(delimiter));
                } else {
                    word.push(character);
                }
            } else if character == '#' || (character == '/' && chars.peek() == Some(&'/')) {
                break;
            } else if matches!(character, '\'' | '"') {
                quote = Some(character);
            } else if character == ';' {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                if !words.is_empty() {
                    commands.push((line + 1, std::mem::take(&mut words)));
                }
            } else if character.is_whitespace() {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            } else {
                word.push(character);
            }
        }
        if !word.is_empty() {
            words.push(word);
        }
        if !words.is_empty() {
            commands.push((line + 1, words));
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_literal_dependencies_without_comments_or_string_content() {
        let result = manifest(
            r#"
            -- dependency 'not-a-resource'
            --[=[ dependencies { 'also-not-a-resource' } ]=]
            description "dependency 'just-text'"
            dependency ('ox_lib')
            dependencies { 'qbx_core', '/server:7290', '/onesync', [=[custom]=] }
            provide 'qb-core'
        "#,
        );
        assert_eq!(
            result.dependencies,
            vec!["/onesync", "/server:7290", "custom", "ox_lib", "qbx_core"]
        );
        assert_eq!(result.provides, vec!["qb-core"]);
        assert!(!result.dynamic);
    }

    #[test]
    fn notes_dynamic_lua_without_executing_it() {
        assert!(manifest("dependencies(get_dependencies())").dynamic);
        let result = manifest("dependency ('ox_' .. name)\ndependencies { 'real', 'qbx_' .. suffix, get_value('not-one', 'not-two') }");
        assert!(result.dynamic);
        assert_eq!(result.dependencies, vec!["real"]);
    }

    #[test]
    fn cfg_preserves_quoted_values_and_splits_commands() {
        let commands = config_commands("# ensure ignored\nexec \"folder/misc.cfg\"; ensure [core] # note\nset rcon_password \"pass#word;still-password\"\nendpoint_add_tcp \"0.0.0.0:30120\" // note");
        assert_eq!(commands.len(), 4);
        assert_eq!(
            commands[0],
            (2, vec!["exec".into(), "folder/misc.cfg".into()])
        );
        assert_eq!(commands[1].1, vec!["ensure", "[core]"]);
        assert_eq!(commands[2].1[2], "pass#word;still-password");
    }
}
