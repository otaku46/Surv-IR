use std::collections::BTreeMap;
use std::io::{self, BufRead};

pub type TomlTable = BTreeMap<String, TomlValue>;

#[derive(Clone, Debug)]
pub enum TomlValue {
    String(String),
    Array(Vec<TomlValue>),
    Table(TomlTable),
    #[allow(dead_code)]
    Bool(bool),
}

impl TomlValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TomlValue::String(s) => Some(s),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_array(&self) -> Option<&[TomlValue]> {
        match self {
            TomlValue::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&TomlTable> {
        match self {
            TomlValue::Table(table) => Some(table),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut TomlTable> {
        match self {
            TomlValue::Table(table) => Some(table),
            _ => None,
        }
    }
}

pub fn parse_toml(reader: impl BufRead) -> io::Result<TomlTable> {
    let mut parser = TomlParser::new();
    parser.parse(reader)?;
    Ok(parser.result)
}

struct TomlParser {
    result: TomlTable,
    current_table: Vec<String>,
}

impl TomlParser {
    fn new() -> Self {
        Self {
            result: TomlTable::new(),
            current_table: Vec::new(),
        }
    }

    fn parse(&mut self, reader: impl BufRead) -> io::Result<()> {
        let mut multiline_key = String::new();
        let mut multiline_value = String::new();
        let mut in_multiline = false;

        for line in reader.lines() {
            let line = line?;

            if in_multiline {
                if let Some(idx) = line.find(r#"""""#) {
                    multiline_value.push_str(&line[..idx]);
                    self.set_value(&multiline_key, TomlValue::String(multiline_value.clone()));
                    in_multiline = false;
                    multiline_key.clear();
                    multiline_value.clear();
                } else {
                    multiline_value.push_str(&line);
                    multiline_value.push('\n');
                }
                continue;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let table_name = &trimmed[1..trimmed.len() - 1];
                self.current_table = table_name
                    .split('.')
                    .map(|s| s.trim().to_string())
                    .collect();
                self.ensure_table();
                continue;
            }

            if let Some(idx) = trimmed.find('=') {
                let key = trimmed[..idx].trim();
                let value = trimmed[idx + 1..].trim().to_string();

                if value.starts_with(r#"""""#) {
                    multiline_key = key.to_string();
                    let rest = &value[3..];
                    if let Some(end_idx) = rest.find(r#"""""#) {
                        let segment = &rest[..end_idx];
                        self.set_value(key, TomlValue::String(segment.to_string()));
                    } else {
                        in_multiline = true;
                        multiline_value.push_str(rest);
                        multiline_value.push('\n');
                    }
                    continue;
                }

                let parsed = self.parse_value(&value);
                self.set_value(key, parsed);
            }
        }

        Ok(())
    }

    fn ensure_table(&mut self) {
        let mut current = &mut self.result;
        for part in &self.current_table {
            current = current
                .entry(part.clone())
                .or_insert_with(|| TomlValue::Table(TomlTable::new()))
                .as_table_mut()
                .unwrap();
        }
    }

    fn set_value(&mut self, key: &str, value: TomlValue) {
        let mut current = &mut self.result;

        for part in &self.current_table {
            current = current
                .entry(part.clone())
                .or_insert_with(|| TomlValue::Table(TomlTable::new()))
                .as_table_mut()
                .unwrap();
        }

        current.insert(key.to_string(), value);
    }

    fn parse_value(&self, s: &str) -> TomlValue {
        let trimmed = s.trim();

        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            let content = &trimmed[1..trimmed.len() - 1];
            return TomlValue::String(unquote_string(content));
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return TomlValue::Array(self.parse_array(trimmed));
        }

        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            return TomlValue::Table(self.parse_inline_table(trimmed));
        }

        if is_number(trimmed) {
            return TomlValue::String(trimmed.to_string());
        }

        if trimmed == "true" || trimmed == "false" {
            return TomlValue::Bool(trimmed == "true");
        }

        TomlValue::String(trimmed.to_string())
    }

    fn parse_array(&self, s: &str) -> Vec<TomlValue> {
        let inner = &s[1..s.len() - 1];
        let inner = inner.trim();
        if inner.is_empty() {
            return Vec::new();
        }

        split_respecting_brackets(inner, ',')
            .into_iter()
            .map(|item| self.parse_value(item.trim()))
            .collect()
    }

    fn parse_inline_table(&self, s: &str) -> TomlTable {
        let inner = &s[1..s.len() - 1];
        let inner = inner.trim();
        let mut result = TomlTable::new();

        if inner.is_empty() {
            return result;
        }

        for pair in split_respecting_brackets(inner, ',') {
            let trimmed = pair.trim();
            if let Some(idx) = trimmed.find('=') {
                let key = trimmed[..idx].trim().to_string();
                let value = trimmed[idx + 1..].trim();
                result.insert(key, self.parse_value(value));
            }
        }

        result
    }
}

fn split_respecting_brackets(s: &str, sep: char) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut prev_char = '\0';

    for ch in s.chars() {
        if ch == '"' && prev_char != '\\' {
            in_string = !in_string;
        }

        if !in_string {
            if ch == '[' || ch == '{' {
                depth += 1;
            } else if ch == ']' || ch == '}' {
                depth -= 1;
            }
        }

        if ch == sep && depth == 0 && !in_string {
            result.push(current);
            current = String::new();
        } else {
            current.push(ch);
        }

        prev_char = ch;
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

fn unquote_string(mut s: &str) -> String {
    let mut result = String::new();
    while let Some(idx) = s.find('\\') {
        result.push_str(&s[..idx]);
        s = &s[idx + 1..];
        if s.is_empty() {
            break;
        }
        let escaped = s.chars().next().unwrap();
        match escaped {
            '"' => result.push('"'),
            '\\' => result.push('\\'),
            'n' => result.push('\n'),
            't' => result.push('\t'),
            other => {
                result.push('\\');
                result.push(other);
            }
        }
        s = &s[1..];
    }
    result.push_str(s);
    result
}

fn is_number(s: &str) -> bool {
    let mut chars = s.chars();
    let mut first = true;
    while let Some(ch) = chars.next() {
        if first && (ch == '-' || ch == '+') {
            first = false;
            continue;
        }
        first = false;
        if !ch.is_ascii_digit() && ch != '.' {
            return false;
        }
    }
    !s.is_empty()
}
