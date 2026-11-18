use std::collections::{BTreeMap, HashSet};
use std::io::{self, BufRead};
use std::ops::Range;

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

    #[allow(dead_code)]
    pub fn as_table_mut(&mut self) -> Option<&mut TomlTable> {
        match self {
            TomlValue::Table(table) => Some(table),
            _ => None,
        }
    }
}

pub fn parse_toml(mut reader: impl BufRead) -> io::Result<TomlTable> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;

    if let Ok(std_table) = toml::from_str::<toml::Table>(&content) {
        return Ok(convert_std_toml_table(std_table));
    }

    let mut parser = TomlParser::new();
    parser.parse_str(&content)?;
    Ok(parser.result)
}

/// Locate explicit status tables in files that use legacy Surv IR syntax.
#[doc(hidden)]
pub fn legacy_status_ranges(content: &str) -> io::Result<Vec<Range<usize>>> {
    let mut parser = TomlParser::new();
    parser.parse_str(content)
}

pub(crate) fn convert_std_toml_table(table: toml::Table) -> TomlTable {
    let mut result = TomlTable::new();
    for (key, value) in table {
        let converted = convert_std_toml_value(value);
        if key == "impl" {
            if let TomlValue::Table(ref impl_table) = converted {
                for (sub_key, sub_val) in impl_table {
                    result.insert(format!("impl.{}", sub_key), sub_val.clone());
                }
            }
        }
        result.insert(key, converted);
    }
    result
}

fn convert_std_toml_value(value: toml::Value) -> TomlValue {
    match value {
        toml::Value::String(s) => TomlValue::String(s),
        toml::Value::Integer(n) => TomlValue::String(n.to_string()),
        toml::Value::Float(f) => TomlValue::String(f.to_string()),
        toml::Value::Boolean(b) => TomlValue::Bool(b),
        toml::Value::Datetime(dt) => TomlValue::String(dt.to_string()),
        toml::Value::Array(items) => {
            TomlValue::Array(items.into_iter().map(convert_std_toml_value).collect())
        }
        toml::Value::Table(table) => TomlValue::Table(convert_std_toml_table(table)),
    }
}

struct TomlParser {
    result: TomlTable,
    current_table: Vec<String>,
    explicit_tables: HashSet<String>,
}

impl TomlParser {
    fn new() -> Self {
        Self {
            result: TomlTable::new(),
            current_table: Vec::new(),
            explicit_tables: HashSet::new(),
        }
    }

    fn parse_str(&mut self, content: &str) -> io::Result<Vec<Range<usize>>> {
        let mut multiline_key = String::new();
        let mut multiline_value = String::new();
        let mut delimiter: Option<&str> = None;
        let mut bracket_key = String::new();
        let mut bracket_value = String::new();
        let mut brackets = Vec::new();
        let mut bracket_quote: Option<char> = None;
        let mut status_ranges = Vec::new();
        let mut status_start = None;
        let mut offset = 0;

        for line in content.split_inclusive('\n') {
            let line_start = offset;
            offset += line.len();
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(end) = delimiter {
                multiline_value.push('\n');
                multiline_value.push_str(line);
                if line.contains(end) {
                    if let Some(value) = parse_toml_string(&multiline_value) {
                        self.set_value(&multiline_key, value)?;
                        delimiter = None;
                        multiline_value.clear();
                    }
                }
                continue;
            }

            if !brackets.is_empty() {
                let cleaned = strip_inline_comment(line).trim();
                if !cleaned.is_empty() {
                    if !bracket_value.is_empty() {
                        bracket_value.push(' ');
                    }
                    bracket_value.push_str(cleaned);
                    scan_value(cleaned, &mut brackets, &mut bracket_quote, &bracket_key)?;
                    if brackets.is_empty() {
                        let parsed = self.parse_value(&bracket_value);
                        self.set_value(&bracket_key, parsed)?;
                        bracket_key.clear();
                        bracket_value.clear();
                        bracket_quote = None;
                    }
                }
                continue;
            }

            let trimmed = strip_inline_comment(line).trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let table_name = &trimmed[1..trimmed.len() - 1];
                self.current_table = split_table_path(table_name);
                if self
                    .current_table
                    .first()
                    .is_some_and(|item| item == "status")
                {
                    status_start.get_or_insert(line_start);
                } else if let Some(start) = status_start.take() {
                    status_ranges.push(start..line_start);
                }
                let table_path = self.current_table.join("\u{1f}");
                if !self.explicit_tables.insert(table_path) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("duplicate table [{}]", table_name),
                    ));
                }
                self.ensure_table()?;
                continue;
            }

            if let Some(idx) = trimmed.find('=') {
                let key = trimmed[..idx].trim();
                let raw_after_eq = trimmed[idx + 1..].trim_start();
                if let Some(start) = ["\"\"\"", "'''"]
                    .into_iter()
                    .find(|delimiter| raw_after_eq.starts_with(delimiter))
                {
                    let raw_after_eq = line[line.find('=').unwrap() + 1..].trim_start();
                    if let Some(value) = parse_toml_string(raw_after_eq) {
                        self.set_value(key, value)?;
                    } else {
                        multiline_key = key.to_string();
                        multiline_value.push_str(raw_after_eq);
                        delimiter = Some(start);
                    }
                    continue;
                }

                let value = trimmed[idx + 1..].trim().to_string();
                scan_value(&value, &mut brackets, &mut bracket_quote, key)?;
                if !brackets.is_empty() {
                    bracket_key = key.to_string();
                    bracket_value = value;
                    continue;
                }
                bracket_quote = None;

                let parsed = self.parse_value(&value);
                self.set_value(key, parsed)?;
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected a table header or assignment at byte {line_start}"),
                ));
            }
        }

        if delimiter.is_some() || !brackets.is_empty() {
            let key = if delimiter.is_some() {
                multiline_key
            } else {
                bracket_key
            };
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unterminated value for {key}"),
            ));
        }

        if let Some(start) = status_start {
            status_ranges.push(start..content.len());
        }
        Ok(status_ranges)
    }

    fn ensure_table(&mut self) -> io::Result<()> {
        let mut current = &mut self.result;
        for part in &self.current_table {
            let entry = current
                .entry(part.clone())
                .or_insert_with(|| TomlValue::Table(TomlTable::new()));
            if !matches!(entry, TomlValue::Table(_)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("key '{}' is both a value and a table", part),
                ));
            }
            current = match entry {
                TomlValue::Table(table) => table,
                _ => unreachable!(),
            };
        }
        Ok(())
    }

    fn set_value(&mut self, key: &str, value: TomlValue) -> io::Result<()> {
        let parts = split_key_path(key);
        if parts.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "empty key in assignment",
            ));
        }

        let mut current = &mut self.result;
        for part in self
            .current_table
            .iter()
            .chain(parts.iter().take(parts.len() - 1))
        {
            let entry = current
                .entry(part.clone())
                .or_insert_with(|| TomlValue::Table(TomlTable::new()));
            if !matches!(entry, TomlValue::Table(_)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("key '{}' is both a value and a table", part),
                ));
            }
            current = match entry {
                TomlValue::Table(table) => table,
                _ => unreachable!(),
            };
        }

        let leaf = parts.last().expect("non-empty key path");
        if current.contains_key(leaf) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("duplicate key '{}'", parts.join(".")),
            ));
        }
        current.insert(leaf.clone(), value);
        Ok(())
    }

    fn parse_value(&self, s: &str) -> TomlValue {
        let trimmed = s.trim();

        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            let content = &trimmed[1..trimmed.len() - 1];
            return parse_toml_string(trimmed)
                .unwrap_or_else(|| TomlValue::String(unquote_string(content)));
        }

        if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
            let content = &trimmed[1..trimmed.len() - 1];
            return TomlValue::String(content.to_string());
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return TomlValue::Array(self.parse_array(trimmed));
        }

        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            let inner = trimmed[1..trimmed.len() - 1].trim();
            let pairs = split_respecting_brackets(inner, ',');
            let has_key_value = pairs.iter().any(|p| p.contains('='));
            if !has_key_value && pairs.iter().any(|p| !p.trim().contains("->")) {
                return TomlValue::String(trimmed.to_string());
            }
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
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .map(|item| self.parse_value(&item))
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
            if trimmed.is_empty() {
                continue;
            }
            if let Some(idx) = trimmed.find('=') {
                let key = trimmed[..idx].trim().to_string();
                let value = trimmed[idx + 1..].trim();
                result.insert(key, self.parse_value(value));
            } else {
                result.insert(trimmed.to_string(), TomlValue::String(String::new()));
            }
        }

        result
    }
}

fn strip_inline_comment(line: &str) -> &str {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;

    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_double {
            escaped = true;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '#' && !in_double && !in_single {
            return &line[..idx];
        }
    }
    line
}

fn scan_value(
    s: &str,
    brackets: &mut Vec<char>,
    quote: &mut Option<char>,
    key: &str,
) -> io::Result<()> {
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        match *quote {
            Some('"') if ch == '\\' => {
                chars.next();
            }
            Some(open) if ch == open => *quote = None,
            Some(_) => {}
            None => match ch {
                '"' | '\'' => *quote = Some(ch),
                '[' => brackets.push(']'),
                '{' => brackets.push('}'),
                ']' | '}' if brackets.pop() != Some(ch) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unmatched {ch} in {key}"),
                    ));
                }
                _ => {}
            },
        }
    }
    if quote.is_some() && brackets.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unterminated string in {key}"),
        ));
    }
    Ok(())
}

fn split_table_path(table_name: &str) -> Vec<String> {
    split_respecting_brackets(table_name, '.')
        .into_iter()
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_key_path(key: &str) -> Vec<String> {
    let trimmed = key.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return vec![trimmed.trim_matches('"').trim_matches('\'').to_string()];
    }
    split_respecting_brackets(trimmed, '.')
        .into_iter()
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
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

fn parse_toml_string(raw: &str) -> Option<TomlValue> {
    let mut table = toml::from_str::<toml::Table>(&format!("value = {raw}")).ok()?;
    match table.remove("value")? {
        toml::Value::String(value) => Some(TomlValue::String(value)),
        _ => None,
    }
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
        s = &s[escaped.len_utf8()..];
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_multiline_arrays_and_inline_comments() {
        let input = r#"
[deploy.job.build]
runs = [
    "npm ci", # install deps
    "npm run build"
]
impl.lang = "ts" # Only in TypeScript
"#;
        let table = parse_toml(Cursor::new(input)).expect("parse");
        let build = table["deploy"].as_table().unwrap()["job"]
            .as_table()
            .unwrap()["build"]
            .as_table()
            .unwrap();
        let runs = build["runs"].as_array().expect("runs array");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].as_str(), Some("npm ci"));
        assert_eq!(runs[1].as_str(), Some("npm run build"));
        assert_eq!(build["impl.lang"].as_str(), Some("ts"));
    }

    #[test]
    fn fallback_parses_multiline_strings_inside_arrays() {
        let input = r#"
[deploy.job.build]
runs = ["""line1
  line2""", "next"]
legacy = bare
"#;
        let table = parse_toml(Cursor::new(input)).expect("fallback parse");
        let build = table["deploy"].as_table().unwrap()["job"]
            .as_table()
            .unwrap()["build"]
            .as_table()
            .unwrap();
        let runs = build["runs"].as_array().expect("runs array");
        assert_eq!(runs.len(), 2);
        let first = runs[0].as_str().expect("multiline string");
        assert!(first.contains("line1"), "{first}");
        assert!(first.contains("line2"), "{first}");
        assert_eq!(runs[1].as_str(), Some("next"));
    }

    #[test]
    fn fallback_rejects_table_scalar_collision() {
        let input = r#"
foo = "scalar"
[foo.bar]
items = { "a", "b" }
"#;
        let err = parse_toml(Cursor::new(input)).expect_err("collision must be rejected");
        assert!(err.to_string().contains("both a value and a table"));
    }
}
