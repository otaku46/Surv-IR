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

    #[allow(dead_code)]
    pub fn as_table_mut(&mut self) -> Option<&mut TomlTable> {
        match self {
            TomlValue::Table(table) => Some(table),
            _ => None,
        }
    }
}

/// Parse strictly as TOML. Surv IR input is defined as valid TOML (Spec v1.1);
/// there is no legacy grammar fallback.
pub fn parse_toml(mut reader: impl BufRead) -> io::Result<TomlTable> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;
    let table: toml::Table =
        toml::from_str(&content).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(convert_std_toml_table(table))
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
    fn rejects_invalid_toml() {
        let input = "legacy = bare\n";
        let err = parse_toml(Cursor::new(input)).expect_err("bare values are not TOML");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_table_scalar_collision() {
        let input = r#"
foo = "scalar"
[foo.bar]
items = ["a", "b"]
"#;
        let err = parse_toml(Cursor::new(input)).expect_err("collision must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
