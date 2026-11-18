use crate::ast::{
    FuncSection, ImportDecl, MetaSection, ModSection, ModuleStatus, PipelineStep, RequireDecl,
    SchemaSection, Section, StatusSection, SurvFile,
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
    parse_surv_table(parse_toml(BufReader::new(reader))?)
}

pub(crate) fn parse_surv_table(raw: TomlTable) -> Result<SurvFile, ParseError> {
    validate_surv_table(&raw)?;
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

fn invalid(msg: impl Into<String>) -> ParseError {
    ParseError::InvalidHeader(msg.into())
}

fn expect_table<'a>(value: &'a TomlValue, label: &str) -> Result<&'a TomlTable, ParseError> {
    value
        .as_table()
        .ok_or_else(|| invalid(format!("{label} must be a table")))
}

fn expect_string(value: &TomlValue, label: &str) -> Result<String, ParseError> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| invalid(format!("{label} must be a string")))
}

fn expect_string_array(value: &TomlValue, label: &str) -> Result<(), ParseError> {
    match value {
        TomlValue::Array(items) => {
            if items.iter().all(|item| item.as_str().is_some()) {
                Ok(())
            } else {
                Err(invalid(format!("{label} entries must be strings")))
            }
        }
        TomlValue::String(_) => Ok(()),
        _ => Err(invalid(format!("{label} must be a string array"))),
    }
}

fn expect_pipeline(value: &TomlValue, label: &str) -> Result<(), ParseError> {
    match value {
        TomlValue::String(_) => Ok(()),
        TomlValue::Table(_) => Ok(()),
        TomlValue::Array(items) => {
            for item in items {
                expect_pipeline(item, label)?;
            }
            Ok(())
        }
        _ => Err(invalid(format!("{label} must be a pipeline array"))),
    }
}

fn validate_surv_table(raw: &TomlTable) -> Result<(), ParseError> {
    for (key, value) in raw {
        match key.as_str() {
            "package" | "namespace" => {
                expect_string(value, key)?;
            }
            "import" | "require" | "requires" => {
                expect_string_array(value, key)?;
            }
            "meta" => validate_meta(expect_table(value, "meta")?)?,
            "schema" => {
                let table = expect_table(value, "schema")?;
                for (name, value) in table {
                    validate_schema(name, expect_table(value, &format!("schema.{name}"))?)?;
                }
            }
            "func" => {
                let table = expect_table(value, "func")?;
                for (name, value) in table {
                    validate_func(name, expect_table(value, &format!("func.{name}"))?)?;
                }
            }
            "mod" => {
                let table = expect_table(value, "mod")?;
                for (name, value) in table {
                    validate_mod(name, expect_table(value, &format!("mod.{name}"))?)?;
                }
            }
            "status" => validate_status(expect_table(value, "status")?)?,
            other => return Err(invalid(format!("unknown top-level key '{other}'"))),
        }
    }
    Ok(())
}

fn validate_meta(table: &TomlTable) -> Result<(), ParseError> {
    for (key, value) in table {
        match key.as_str() {
            "name" | "version" | "description" => {
                expect_string(value, &format!("meta.{key}"))?;
            }
            other => return Err(invalid(format!("unknown meta key '{other}'"))),
        }
    }
    Ok(())
}

fn validate_impl(table: &TomlTable, label: &str) -> Result<(), ParseError> {
    for (key, value) in table {
        match key.as_str() {
            "bind" | "lang" | "path" => {
                expect_string(value, &format!("{label}.impl.{key}"))?;
            }
            other => return Err(invalid(format!("unknown impl key '{other}'"))),
        }
    }
    Ok(())
}

fn validate_schema(name: &str, table: &TomlTable) -> Result<(), ParseError> {
    let label = format!("schema.{name}");
    for (key, value) in table {
        match key.as_str() {
            "kind" | "role" | "type" | "from" | "to" | "base" | "label" => {
                expect_string(value, &format!("{label}.{key}"))?;
            }
            "fields" => {
                let fields = expect_table(value, &format!("{label}.fields"))?;
                for (field, field_type) in fields {
                    expect_string(field_type, &format!("{label}.fields.{field}"))?;
                }
            }
            "over" => expect_string_array(value, &format!("{label}.over"))?,
            "impl" => validate_impl(expect_table(value, &format!("{label}.impl"))?, &label)?,
            key if key.starts_with("impl.") => {
                expect_string(value, &format!("{label}.{key}"))?;
            }
            other => return Err(invalid(format!("unknown schema key '{other}' in {label}"))),
        }
    }

    let kind = table
        .get("kind")
        .and_then(|value| value.as_str())
        .ok_or_else(|| invalid(format!("{label} requires a string 'kind'")))?;
    match kind {
        "node" | "edge" | "boundary" | "value" | "space" => {}
        other => return Err(invalid(format!("{label}: unknown kind '{other}'"))),
    }
    match kind {
        "edge" => {
            for required in ["from", "to"] {
                let value = table
                    .get(required)
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                if value.trim().is_empty() {
                    return Err(invalid(format!("{label}: edge requires '{required}'")));
                }
            }
        }
        "boundary" => {
            let has_over = match table.get("over") {
                Some(TomlValue::Array(items)) => !items.is_empty(),
                Some(TomlValue::String(value)) => !value.trim().is_empty(),
                _ => false,
            };
            if !has_over {
                return Err(invalid(format!("{label}: boundary requires 'over'")));
            }
        }
        "value" => {
            let has_type = table
                .get("type")
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.trim().is_empty());
            let has_fields = table
                .get("fields")
                .and_then(|value| value.as_table())
                .is_some_and(|fields| !fields.is_empty());
            if !has_type && !has_fields {
                return Err(invalid(format!(
                    "{label}: value requires 'type' or 'fields'"
                )));
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_func(name: &str, table: &TomlTable) -> Result<(), ParseError> {
    let label = format!("func.{name}");
    for (key, value) in table {
        match key.as_str() {
            "intent" | "design_notes" => {
                expect_string(value, &format!("{label}.{key}"))?;
            }
            "input" | "output" => expect_string_array(value, &format!("{label}.{key}"))?,
            "impl" => validate_impl(expect_table(value, &format!("{label}.impl"))?, &label)?,
            key if key.starts_with("impl.") => {
                expect_string(value, &format!("{label}.{key}"))?;
            }
            other => return Err(invalid(format!("unknown func key '{other}' in {label}"))),
        }
    }
    Ok(())
}

fn validate_mod(name: &str, table: &TomlTable) -> Result<(), ParseError> {
    let label = format!("mod.{name}");
    for (key, value) in table {
        match key.as_str() {
            "purpose" => {
                expect_string(value, &format!("{label}.purpose"))?;
            }
            "pipeline" => {
                expect_pipeline(value, &format!("{label}.pipeline"))?;
            }
            "schemas" | "funcs" | "submods" => {
                expect_string_array(value, &format!("{label}.{key}"))?;
            }
            "boundary" => {
                let boundary = expect_table(value, &format!("{label}.boundary"))?;
                for (boundary_key, entries) in boundary {
                    expect_string_array(entries, &format!("{label}.boundary.{boundary_key}"))?;
                }
            }
            other => {
                if let Some(child) = value.as_table() {
                    if other == "impl" {
                        return Err(invalid(format!("unknown mod key 'impl' in {label}")));
                    }
                    validate_mod(&format!("{name}.{other}"), child)?;
                } else {
                    return Err(invalid(format!("unknown mod key '{other}' in {label}")));
                }
            }
        }
    }
    Ok(())
}

fn validate_status(table: &TomlTable) -> Result<(), ParseError> {
    for (key, value) in table {
        match key.as_str() {
            "updated_at" => {
                expect_string(value, "status.updated_at")?;
            }
            "mod" => validate_status_modules("", expect_table(value, "status.mod")?)?,
            other => return Err(invalid(format!("unknown status key '{other}'"))),
        }
    }
    Ok(())
}

fn validate_status_modules(parent: &str, table: &TomlTable) -> Result<(), ParseError> {
    for (name, value) in table {
        let status = expect_table(value, &format!("status.mod.{name}"))?;
        let full_name = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}.{name}")
        };
        validate_status_module(&full_name, status)?;
    }
    Ok(())
}

fn validate_status_module(full_name: &str, status: &TomlTable) -> Result<(), ParseError> {
    for (key, value) in status {
        if let Some(child) = value.as_table() {
            validate_status_module(&format!("{full_name}.{key}"), child)?;
            continue;
        }
        match key.as_str() {
            "state" => {
                let state = expect_string(value, &format!("status.mod.{full_name}.state"))?;
                if !matches!(
                    state.as_str(),
                    "todo" | "skeleton" | "partial" | "done" | "blocked"
                ) {
                    return Err(invalid(format!(
                        "status.mod.{full_name}: unknown state '{state}'"
                    )));
                }
            }
            "coverage" => {
                let raw = expect_string(value, &format!("status.mod.{full_name}.coverage"))?;
                let coverage = raw.parse::<f64>().map_err(|_| {
                    invalid(format!("status.mod.{full_name}: coverage must be a number"))
                })?;
                if !coverage.is_finite() || !(0.0..=1.0).contains(&coverage) {
                    return Err(invalid(format!(
                        "status.mod.{full_name}: coverage must be between 0.0 and 1.0"
                    )));
                }
            }
            "notes" => {
                expect_string(value, &format!("status.mod.{full_name}.notes"))?;
            }
            other => {
                return Err(invalid(format!(
                    "unknown status key '{other}' in status.mod.{full_name}"
                )));
            }
        }
    }
    Ok(())
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
        if !entry.starts_with("mod.") && !entry.contains(".mod.") {
            return Err(ParseError::InvalidHeader(format!(
                "require entry '{}' must reference a module",
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
        parse_mod_tree("", mod_table, &mut sections);
    }

    if let Some(status_table) = get_table(raw, "status") {
        sections.push(Section::Status(parse_status_section(status_table)));
    }

    Ok(sections)
}

fn parse_mod_tree(parent_path: &str, table: &TomlTable, sections: &mut Vec<Section>) {
    for (key, value) in table {
        if key == "purpose"
            || key == "schemas"
            || key == "funcs"
            || key == "pipeline"
            || key == "submods"
            || key == "boundary"
        {
            continue;
        }
        if let TomlValue::Table(sub_table) = value {
            let full_name = if parent_path.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", parent_path, key)
            };
            sections.push(Section::Mod(parse_mod_section(&full_name, sub_table)));
            parse_mod_tree(&full_name, sub_table, sections);
        }
    }
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
        impl_bind: get_impl_optional_string(table, "bind"),
        impl_lang: get_impl_optional_string(table, "lang"),
        impl_path: get_impl_optional_string(table, "path"),
    }
}

fn parse_func_section(name: &str, table: &TomlTable) -> FuncSection {
    FuncSection {
        name: name.to_string(),
        intent: get_string(table, "intent"),
        input: parse_string_set(table, "input"),
        output: parse_string_set(table, "output"),
        design_notes: get_string(table, "design_notes"),
        impl_bind: get_impl_optional_string(table, "bind"),
        impl_lang: get_impl_optional_string(table, "lang"),
        impl_path: get_impl_optional_string(table, "path"),
    }
}

fn parse_mod_section(name: &str, table: &TomlTable) -> ModSection {
    ModSection {
        name: name.to_string(),
        purpose: get_string(table, "purpose"),
        schemas: parse_string_set(table, "schemas"),
        funcs: parse_string_set(table, "funcs"),
        pipeline: parse_pipeline(table, "pipeline"),
        submods: parse_string_set(table, "submods"),
        boundary: get_table(table, "boundary")
            .into_iter()
            .flat_map(|boundary| {
                boundary
                    .keys()
                    .map(|key| (key.clone(), parse_string_set(boundary, key)))
            })
            .collect(),
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

fn get_impl_optional_string(table: &TomlTable, key: &str) -> Option<String> {
    get_optional_string(table, &format!("impl.{key}")).or_else(|| {
        get_table(table, "impl").and_then(|impl_table| get_optional_string(impl_table, key))
    })
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

fn parse_pipeline(table: &TomlTable, key: &str) -> Vec<PipelineStep> {
    if let Some(value) = table.get(key) {
        parse_pipeline_item_array(value)
    } else {
        Vec::new()
    }
}

fn parse_pipeline_item_array(value: &TomlValue) -> Vec<PipelineStep> {
    match value {
        TomlValue::Array(items) => {
            let mut result = Vec::new();
            for item in items {
                result.extend(parse_pipeline_item(item));
            }
            result
        }
        _ => parse_pipeline_item(value),
    }
}

fn parse_pipeline_item(value: &TomlValue) -> Vec<PipelineStep> {
    match value {
        TomlValue::String(s) => parse_pipeline_chain(s),
        TomlValue::Table(map) => {
            if let Some(parallel_val) = map.get("parallel") {
                let steps = parse_pipeline_item_array(parallel_val);
                vec![PipelineStep::Parallel(steps)]
            } else if let Some(branch_val) = map.get("branch") {
                if let Some(branch_table) = branch_val.as_table() {
                    let condition = get_string(branch_table, "condition");
                    let on_true = match branch_table.get("on_true") {
                        Some(val) => parse_pipeline_item_single(val),
                        None => PipelineStep::Call(String::new()),
                    };
                    let on_false = match branch_table.get("on_false") {
                        Some(val) => parse_pipeline_item_single(val),
                        None => PipelineStep::Call(String::new()),
                    };
                    vec![PipelineStep::Branch {
                        condition,
                        on_true: Box::new(on_true),
                        on_false: Box::new(on_false),
                    }]
                } else {
                    vec![]
                }
            } else {
                let mut result = Vec::new();
                for chain in map.keys() {
                    result.extend(parse_pipeline_chain(chain));
                }
                result
            }
        }
        TomlValue::Array(_) => parse_pipeline_item_array(value),
        _ => Vec::new(),
    }
}

fn parse_pipeline_item_single(value: &TomlValue) -> PipelineStep {
    let mut items = parse_pipeline_item(value);
    if items.is_empty() {
        PipelineStep::Call(String::new())
    } else if items.len() == 1 {
        items.pop().unwrap()
    } else {
        PipelineStep::Sequential(items)
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

fn parse_pipeline_chain(input: &str) -> Vec<PipelineStep> {
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
        .map(PipelineStep::Call)
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
                vec![
                    PipelineStep::Call("func.a".to_string()),
                    PipelineStep::Call("func.b".to_string()),
                ],
            ),
            (
                "func.a -> func.b -> func.c",
                vec![
                    PipelineStep::Call("func.a".to_string()),
                    PipelineStep::Call("func.b".to_string()),
                    PipelineStep::Call("func.c".to_string()),
                ],
            ),
            (
                "func.single",
                vec![PipelineStep::Call("func.single".to_string())],
            ),
            (
                "{ func.a -> func.b }",
                vec![
                    PipelineStep::Call("func.a".to_string()),
                    PipelineStep::Call("func.b".to_string()),
                ],
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(super::parse_pipeline_chain(input), expected);
        }
    }
}

fn parse_status_section(table: &TomlTable) -> StatusSection {
    let updated_at = get_string(table, "updated_at");
    let mut modules = BTreeMap::new();

    if let Some(mod_table) = get_table(table, "mod") {
        parse_module_statuses("", mod_table, &mut modules);
    }

    StatusSection {
        name: "status".to_string(),
        updated_at,
        modules,
    }
}

fn parse_module_statuses(
    parent: &str,
    table: &TomlTable,
    modules: &mut BTreeMap<String, ModuleStatus>,
) {
    for (name, value) in table {
        let Some(status) = value.as_table() else {
            continue;
        };
        let name = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}.{name}")
        };
        let has_leaf_value = ["state", "coverage", "notes"].iter().any(|key| {
            status
                .get(*key)
                .is_some_and(|value| !matches!(value, TomlValue::Table(_)))
        });
        if status.is_empty() || has_leaf_value {
            modules.insert(
                name.clone(),
                ModuleStatus {
                    state: get_string(status, "state"),
                    coverage: get_f64(status, "coverage"),
                    notes: get_string(status, "notes"),
                },
            );
        }
        parse_module_statuses(&name, status, modules);
    }
}

fn get_f64(table: &TomlTable, key: &str) -> f64 {
    match table.get(key) {
        Some(TomlValue::String(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}
