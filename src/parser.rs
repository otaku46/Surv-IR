use crate::ast::{
    FuncSection, ImportDecl, MetaSection, ModSection, ModuleStatus, RequireDecl, SchemaSection,
    Section, StatusSection, SurvFile,
};
use crate::simple_toml::{parse_toml, TomlTable, TomlValue};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

#[derive(Debug)]
pub enum ParseError {
    Io(io::Error),
    InvalidHeader(String),
}

impl From<io::Error> for ParseError {
    fn from(err: io::Error) -> Self {
        ParseError::Io(err)
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Io(err) => write!(f, "{}", err),
            ParseError::InvalidHeader(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::Io(err) => Some(err),
            ParseError::InvalidHeader(_) => None,
        }
    }
}

pub fn parse_surv_file<R: Read>(reader: R) -> Result<SurvFile, ParseError> {
    let raw = parse_toml(BufReader::new(reader))?;

    let package = parse_optional_header_string(&raw, "package")?;
    let namespace = parse_optional_header_string(&raw, "namespace")?;
    let imports = parse_imports(&raw)?;
    let requires = parse_requires(&raw)?;
    let sections = parse_sections(&raw)?;

    Ok(SurvFile {
        package,
        namespace,
        imports,
        requires,
        sections,
    })
}

pub fn parse_surv_ir<R: Read>(reader: R) -> Result<SurvFile, ParseError> {
    parse_surv_file(reader)
}

pub fn parse_file(path: &Path) -> Result<SurvFile, ParseError> {
    let file = File::open(path)?;
    parse_surv_file(file)
}

fn parse_optional_header_string(raw: &TomlTable, key: &str) -> Result<Option<String>, ParseError> {
    match raw.get(key) {
        None => Ok(None),
        Some(TomlValue::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        _ => Err(ParseError::InvalidHeader(format!("{key} must be a string"))),
    }
}

fn parse_imports(raw: &TomlTable) -> Result<Vec<ImportDecl>, ParseError> {
    match raw.get("import") {
        None => Ok(Vec::new()),
        Some(value) => parse_string_array("import", value).map(|items| {
            items
                .into_iter()
                .map(|target| ImportDecl {
                    target,
                    alias: None,
                })
                .collect()
        }),
    }
}

fn parse_requires(raw: &TomlTable) -> Result<Vec<RequireDecl>, ParseError> {
    let mut result = Vec::new();
    if let Some(value) = raw.get("require") {
        result.extend(parse_string_array("require", value)?);
    }
    if let Some(value) = raw.get("requires") {
        result.extend(parse_string_array("requires", value)?);
    }
    for entry in &result {
        if !entry.starts_with("mod.") {
            return Err(ParseError::InvalidHeader(format!(
                "require entry '{}' must start with 'mod.'",
                entry
            )));
        }
    }
    Ok(result
        .into_iter()
        .map(|target| RequireDecl { target })
        .collect())
}

fn parse_string_array(label: &str, value: &TomlValue) -> Result<Vec<String>, ParseError> {
    match value {
        TomlValue::Array(items) => {
            let mut result = Vec::new();
            for item in items {
                match item {
                    TomlValue::String(s) => result.push(s.trim().to_string()),
                    _ => {
                        return Err(ParseError::InvalidHeader(format!(
                            "{} entries must be strings",
                            label
                        )))
                    }
                }
            }
            Ok(result)
        }
        _ => Err(ParseError::InvalidHeader(format!(
            "{} must be an array",
            label
        ))),
    }
}

fn parse_sections(raw: &TomlTable) -> Result<Vec<Section>, ParseError> {
    let mut sections = Vec::new();

    if let Some(meta_table) = get_table(raw, "meta") {
        sections.push(Section::Meta(parse_meta(meta_table)));
    }

    if let Some(schema_table) = get_table(raw, "schema") {
        for (name, value) in schema_table {
            if let TomlValue::Table(section) = value {
                sections.push(Section::Schema(parse_schema_section(name, section)));
            }
        }
    }

    if let Some(func_table) = get_table(raw, "func") {
        for (name, value) in func_table {
            if let TomlValue::Table(section) = value {
                sections.push(Section::Func(parse_func_section(name, section)));
            }
        }
    }

    if let Some(mod_table) = get_table(raw, "mod") {
        for (name, value) in mod_table {
            if let TomlValue::Table(section) = value {
                sections.push(Section::Mod(parse_mod_section(name, section)));
            }
        }
    }

    if let Some(status_table) = get_table(raw, "status") {
        sections.push(Section::Status(parse_status_section(status_table)));
    }

    Ok(sections)
}

fn get_table<'a>(table: &'a TomlTable, key: &str) -> Option<&'a TomlTable> {
    table.get(key)?.as_table()
}

fn parse_meta(table: &TomlTable) -> MetaSection {
    MetaSection {
        name: get_string(table, "name"),
        version: get_string(table, "version"),
        description: get_string(table, "description"),
    }
}

fn parse_schema_section(name: &str, table: &TomlTable) -> SchemaSection {
    let mut fields = BTreeMap::new();
    if let Some(TomlValue::Table(field_table)) = table.get("fields") {
        for (k, v) in field_table {
            if let Some(value) = v.as_str() {
                fields.insert(k.clone(), value.to_string());
            }
        }
    }

    SchemaSection {
        name: name.to_string(),
        kind: get_string(table, "kind"),
        role: get_string(table, "role"),
        r#type: get_string(table, "type"),
        from: get_string(table, "from"),
        to: get_string(table, "to"),
        base: get_string(table, "base"),
        label: get_string(table, "label"),
        fields,
        over: parse_string_set(table, "over"),
        impl_bind: get_optional_string(table, "impl.bind"),
        impl_lang: get_optional_string(table, "impl.lang"),
        impl_path: get_optional_string(table, "impl.path"),
    }
}

fn parse_func_section(name: &str, table: &TomlTable) -> FuncSection {
    FuncSection {
        name: name.to_string(),
        intent: get_string(table, "intent"),
        input: parse_string_set(table, "input"),
        output: parse_string_set(table, "output"),
        design_notes: get_string(table, "design_notes"),
        impl_bind: get_optional_string(table, "impl.bind"),
        impl_lang: get_optional_string(table, "impl.lang"),
        impl_path: get_optional_string(table, "impl.path"),
    }
}

fn parse_mod_section(name: &str, table: &TomlTable) -> ModSection {
    ModSection {
        name: name.to_string(),
        purpose: get_string(table, "purpose"),
        schemas: parse_string_set(table, "schemas"),
        funcs: parse_string_set(table, "funcs"),
        pipeline: parse_pipeline(table, "pipeline"),
    }
}

fn get_string(table: &TomlTable, key: &str) -> String {
    table
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string()
}

fn get_optional_string(table: &TomlTable, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn parse_string_set(table: &TomlTable, key: &str) -> Vec<String> {
    match table.get(key) {
        Some(TomlValue::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|s| s.trim().to_string())
            .collect(),
        Some(TomlValue::String(s)) => parse_inline_brace_set(s),
        _ => Vec::new(),
    }
}

fn parse_pipeline(table: &TomlTable, key: &str) -> Vec<String> {
    match table.get(key) {
        Some(TomlValue::Table(map)) => {
            let mut result = Vec::new();
            for chain in map.keys() {
                result.extend(parse_pipeline_chain(chain));
            }
            result
        }
        Some(TomlValue::Array(items)) => {
            let mut result = Vec::new();
            for item in items {
                if let Some(s) = item.as_str() {
                    result.extend(parse_pipeline_chain(s));
                }
            }
            result
        }
        Some(TomlValue::String(s)) => parse_pipeline_chain(s),
        _ => Vec::new(),
    }
}

fn parse_inline_brace_set(input: &str) -> Vec<String> {
    let mut s = input.trim();
    if let Some(stripped) = s.strip_prefix('{') {
        s = stripped;
    }
    if let Some(stripped) = s.strip_suffix('}') {
        s = stripped;
    }
    s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',')
        .map(|part| part.trim().trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_pipeline_chain(input: &str) -> Vec<String> {
    let mut s = input.trim();
    if let Some(stripped) = s.strip_prefix('{') {
        s = stripped;
    }
    if let Some(stripped) = s.strip_suffix('}') {
        s = stripped;
    }
    s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    s.split("->")
        .map(|part| part.trim().to_string())
        .filter(|step| !step.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SAMPLE_IR: &str = r#"
namespace = "example.user"
import = ["std.schema"]
require = ["mod.shared"]

[meta]
name        = "user_crud_example"
version     = "0.1.0"
description = "シンプルな User CRUD API を Surv IR で表現した例"

[schema.user]
kind   = "node"
role   = "data"
type   = "User"
fields = { user_id = "string", name = "string", email = "string" }

[schema.create_user_req]
kind   = "node"
role   = "query"
type   = "CreateUserRequest"
fields = { name = "string", email = "string" }

[schema.users_snapshot]
kind  = "boundary"
role  = "context"
over  = ["schema.user"]
label = "現在DBに存在するユーザー集合のビュー"

[func.create_user]
intent = "CreateUserRequest から User を構成する"
input  = ["schema.create_user_req"]
output = ["schema.user"]

[func.save_user]
intent = "User を永続化して users_snapshot に反映する"
input  = ["schema.user", "schema.users_snapshot"]
output = ["schema.users_snapshot"]

[func.get_user]
intent = "user_id から1件の User を取得する"
input  = ["schema.user_id_query", "schema.users_snapshot"]
output = ["schema.user"]

[mod.user_http_api]
purpose = "HTTP 経由で User の CRUD を提供するモジュール"
schemas = ["schema.user", "schema.create_user_req", "schema.users_snapshot"]
funcs   = ["func.create_user", "func.save_user", "func.get_user"]
"#;

    #[test]
    fn parses_sample_ir() {
        let file = parse_surv_file(Cursor::new(SAMPLE_IR)).expect("parse");
        assert_eq!(file.namespace.as_deref(), Some("example.user"));
        assert_eq!(file.imports.len(), 1);
        assert_eq!(file.requires.len(), 1);
        assert_eq!(file.sections.len(), 8);
    }

    #[test]
    fn parses_inline_brace_set() {
        let cases = vec![
            (
                r#"{ "a", "b", "c" }"#,
                vec!["a".to_string(), "b".to_string(), "c".to_string()],
            ),
            (r#"{ a, b }"#, vec!["a".to_string(), "b".to_string()]),
            (r#"{ }"#, Vec::<String>::new()),
            (r#"{ "schema.user" }"#, vec!["schema.user".to_string()]),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_inline_brace_set(input), expected);
        }
    }

    #[test]
    fn parses_pipeline_chain() {
        let cases = vec![
            (
                "func.a -> func.b",
                vec!["func.a".to_string(), "func.b".to_string()],
            ),
            (
                "func.a -> func.b -> func.c",
                vec![
                    "func.a".to_string(),
                    "func.b".to_string(),
                    "func.c".to_string(),
                ],
            ),
            ("func.single", vec!["func.single".to_string()]),
            (
                "{ func.a -> func.b }",
                vec!["func.a".to_string(), "func.b".to_string()],
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_pipeline_chain(input), expected);
        }
    }
}

fn parse_status_section(table: &TomlTable) -> StatusSection {
    let updated_at = get_string(table, "updated_at");
    let mut modules = BTreeMap::new();

    // Parse [status.mod] table which contains module statuses
    if let Some(TomlValue::Table(mod_table)) = table.get("mod") {
        for (module_name, value) in mod_table {
            if let TomlValue::Table(mod_status_table) = value {
                let state = get_string(mod_status_table, "state");
                let coverage = get_f64(mod_status_table, "coverage");
                let notes = get_string(mod_status_table, "notes");

                modules.insert(
                    module_name.clone(),
                    ModuleStatus {
                        state,
                        coverage,
                        notes,
                    },
                );
            }
        }
    }

    StatusSection {
        name: "status".to_string(),
        updated_at,
        modules,
    }
}

fn get_f64(table: &TomlTable, key: &str) -> f64 {
    match table.get(key) {
        Some(TomlValue::String(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}
