use crate::ast::{FuncSection, ModSection, SchemaSection, Section, SurvFile};
use crate::diagnostic::Diagnostic;
use std::collections::{BTreeMap, BTreeSet};

pub fn check_surv_file(file: &SurvFile) -> Vec<Diagnostic> {
    let index = FileIndex::new(file);
    let mut diags = Vec::new();
    check_func_schemas(&index, &mut diags);
    check_mod_references(&index, &mut diags);
    check_schema_links(&index, &mut diags);
    check_pipeline_semantics(&index, &mut diags);
    check_unused_definitions(&index, &mut diags);
    diags
}

pub fn check_surv_ast(file: &SurvFile) -> Vec<Diagnostic> {
    check_surv_file(file)
}

struct FileIndex<'a> {
    schemas: BTreeMap<String, &'a SchemaSection>,
    funcs: BTreeMap<String, &'a FuncSection>,
    mods: BTreeMap<String, &'a ModSection>,
}

impl<'a> FileIndex<'a> {
    fn new(file: &'a SurvFile) -> Self {
        let mut schemas = BTreeMap::new();
        let mut funcs = BTreeMap::new();
        let mut mods = BTreeMap::new();

        for section in &file.sections {
            match section {
                Section::Schema(schema) => {
                    schemas.insert(schema_id(schema), schema);
                }
                Section::Func(func) => {
                    funcs.insert(func_id(func), func);
                }
                Section::Mod(module) => {
                    mods.insert(mod_id(module), module);
                }
                Section::Meta(_) => {}
                Section::Status(_) => {}
            }
        }

        Self {
            schemas,
            funcs,
            mods,
        }
    }
}

fn schema_id(schema: &SchemaSection) -> String {
    format!("schema.{}", schema.name)
}

fn func_id(func: &FuncSection) -> String {
    format!("func.{}", func.name)
}

fn mod_id(module: &ModSection) -> String {
    format!("mod.{}", module.name)
}

fn check_func_schemas(index: &FileIndex<'_>, diags: &mut Vec<Diagnostic>) {
    for func in index.funcs.values() {
        for schema in &func.input {
            if !index.schemas.contains_key(schema) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedSchema".into(),
                    message: format!(
                        "func {}: input schema {} is not defined",
                        func_id(func),
                        schema
                    ),
                    location: format!("{}.input({})", func_id(func), schema),
                });
            }
        }

        for schema in &func.output {
            if !index.schemas.contains_key(schema) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedSchema".into(),
                    message: format!(
                        "func {}: output schema {} is not defined",
                        func_id(func),
                        schema
                    ),
                    location: format!("{}.output({})", func_id(func), schema),
                });
            }
        }
    }
}

fn check_mod_references(index: &FileIndex<'_>, diags: &mut Vec<Diagnostic>) {
    for module in index.mods.values() {
        for schema in &module.schemas {
            if !index.schemas.contains_key(schema) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedSchemaInMod".into(),
                    message: format!("mod {}: schema {} is not defined", mod_id(module), schema),
                    location: format!("{}.schemas({})", mod_id(module), schema),
                });
            }
        }

        for func in &module.funcs {
            if !index.funcs.contains_key(func) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedFuncInMod".into(),
                    message: format!("mod {}: func {} is not defined", mod_id(module), func),
                    location: format!("{}.funcs({})", mod_id(module), func),
                });
            }
        }

        for step in &module.pipeline {
            if !index.funcs.contains_key(step) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedFuncInPipeline".into(),
                    message: format!(
                        "mod {}: pipeline step {} is not defined",
                        mod_id(module),
                        step
                    ),
                    location: format!("{}.pipeline({})", mod_id(module), step),
                });
            }
        }
    }
}

fn check_schema_links(index: &FileIndex<'_>, diags: &mut Vec<Diagnostic>) {
    for schema in index.schemas.values() {
        match schema.kind.as_str() {
            "edge" => {
                if !schema.from.is_empty() && !index.schemas.contains_key(&schema.from) {
                    diags.push(Diagnostic {
                        severity: "error".into(),
                        kind: "UndefinedSchemaInEdgeFrom".into(),
                        message: format!(
                            "schema {}: edge.from {} is not defined",
                            schema_id(schema),
                            schema.from
                        ),
                        location: format!("{}.from({})", schema_id(schema), schema.from),
                    });
                }
                if !schema.to.is_empty() && !index.schemas.contains_key(&schema.to) {
                    diags.push(Diagnostic {
                        severity: "error".into(),
                        kind: "UndefinedSchemaInEdgeTo".into(),
                        message: format!(
                            "schema {}: edge.to {} is not defined",
                            schema_id(schema),
                            schema.to
                        ),
                        location: format!("{}.to({})", schema_id(schema), schema.to),
                    });
                }
            }
            "boundary" => {
                for over in &schema.over {
                    if !index.schemas.contains_key(over) {
                        diags.push(Diagnostic {
                            severity: "error".into(),
                            kind: "UndefinedSchemaInBoundary".into(),
                            message: format!(
                                "schema {}: boundary.over {} is not defined",
                                schema_id(schema),
                                over
                            ),
                            location: format!("{}.over({})", schema_id(schema), over),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_pipeline_semantics(index: &FileIndex<'_>, diags: &mut Vec<Diagnostic>) {
    for module in index.mods.values() {
        if module.pipeline.is_empty() {
            continue;
        }

        let mut seen = BTreeSet::new();
        for step in &module.pipeline {
            if !seen.insert(step.clone()) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "PipelineCycle".into(),
                    message: format!(
                        "mod {}: pipeline has a cycle involving {} (appears multiple times)",
                        mod_id(module),
                        step
                    ),
                    location: format!("{}.pipeline", mod_id(module)),
                });
            }
        }

        for pair in module.pipeline.windows(2) {
            let f1 = &pair[0];
            let f2 = &pair[1];

            let Some(func1) = index.funcs.get(f1) else {
                continue;
            };
            let Some(func2) = index.funcs.get(f2) else {
                continue;
            };

            if !has_common_schema(&func1.output, &func2.input) {
                diags.push(Diagnostic {
                    severity: "warning".into(),
                    kind: "PipelineTypeMismatch".into(),
                    message: format!(
                        "mod {}: pipeline step {} -> {} has no shared schema between output and input",
                        mod_id(module),
                        f1,
                        f2
                    ),
                    location: format!("{}.pipeline({}->{})", mod_id(module), f1, f2),
                });
            }
        }
    }
}

fn has_common_schema(a: &[String], b: &[String]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let mut set: BTreeSet<String> = BTreeSet::new();
    for item in a {
        set.insert(item.clone());
    }
    b.iter().any(|schema| set.contains(schema))
}

fn check_unused_definitions(index: &FileIndex<'_>, diags: &mut Vec<Diagnostic>) {
    let mut used_schemas = BTreeSet::new();

    for func in index.funcs.values() {
        for schema in func.input.iter().chain(func.output.iter()) {
            used_schemas.insert(schema.clone());
        }
    }

    for module in index.mods.values() {
        for schema in &module.schemas {
            used_schemas.insert(schema.clone());
        }
    }

    for schema in index.schemas.values() {
        if !schema.from.is_empty() {
            used_schemas.insert(schema.from.clone());
        }
        if !schema.to.is_empty() {
            used_schemas.insert(schema.to.clone());
        }
        for over in &schema.over {
            used_schemas.insert(over.clone());
        }
    }

    for name in index.schemas.keys() {
        if !used_schemas.contains(name) {
            diags.push(Diagnostic {
                severity: "warning".into(),
                kind: "UnusedSchema".into(),
                message: format!("schema {} is defined but never referenced", name),
                location: name.clone(),
            });
        }
    }

    let mut used_funcs = BTreeSet::new();

    for module in index.mods.values() {
        for func in &module.funcs {
            used_funcs.insert(func.clone());
        }
        for step in &module.pipeline {
            used_funcs.insert(step.clone());
        }
    }

    for name in index.funcs.keys() {
        if !used_funcs.contains(name) {
            diags.push(Diagnostic {
                severity: "warning".into(),
                kind: "UnusedFunc".into(),
                message: format!("func {} is defined but never referenced in any mod", name),
                location: name.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_surv_file;
    use std::io::Cursor;

    fn parse(input: &str) -> SurvFile {
        parse_surv_file(Cursor::new(input)).expect("parse")
    }

    #[test]
    fn detects_undefined_schema_in_func() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[func.create_user]
intent = "test"
input  = ["schema.undefined_input"]
output = ["schema.user"]

[func.get_user]
intent = "test"
input  = ["schema.user"]
output = ["schema.undefined_output"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        let count = diags
            .iter()
            .filter(|d| d.kind == "UndefinedSchema" && d.severity == "error")
            .count();
        assert_eq!(count, 2);
    }

    #[test]
    fn detects_undefined_func_in_pipeline() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[func.create_user]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[mod.api]
purpose = "test"
schemas = ["schema.user"]
funcs   = ["func.create_user"]
pipeline = ["func.create_user", "func.nonexistent"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(
            diags
                .iter()
                .any(|d| d.kind == "UndefinedFuncInPipeline"
                    && d.message.contains("func.nonexistent"))
        );
    }

    #[test]
    fn detects_undefined_schema_in_boundary() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[schema.snapshot]
kind  = "boundary"
role  = "context"
over  = ["schema.user", "schema.nonexistent"]
label = "test"
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags
            .iter()
            .any(|d| d.kind == "UndefinedSchemaInBoundary"
                && d.message.contains("schema.nonexistent")));
    }

    #[test]
    fn detects_undefined_schema_in_edge() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[schema.follows]
kind = "edge"
from = "schema.user"
to   = "schema.nonexistent"
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags.iter().any(|d| d.kind == "UndefinedSchemaInEdgeTo"));
    }

    #[test]
    fn detects_undefined_schema_in_mod() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[func.create_user]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[mod.api]
purpose = "test"
schemas = ["schema.user", "schema.nonexistent"]
funcs   = ["func.create_user"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags.iter().any(|d| d.kind == "UndefinedSchemaInMod"));
    }

    #[test]
    fn detects_undefined_func_in_mod() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[func.create_user]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[mod.api]
purpose = "test"
schemas = ["schema.user"]
funcs   = ["func.create_user", "func.nonexistent"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags.iter().any(|d| d.kind == "UndefinedFuncInMod"));
    }

    #[test]
    fn detects_unused_schema() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[schema.unused]
kind = "node"
type = "Unused"

[func.create_user]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[mod.api]
purpose = "test"
schemas = ["schema.user"]
funcs   = ["func.create_user"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags
            .iter()
            .any(|d| d.kind == "UnusedSchema" && d.message.contains("schema.unused")));
    }

    #[test]
    fn detects_unused_func() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[func.create_user]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[func.unused_func]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[mod.api]
purpose = "test"
schemas = ["schema.user"]
funcs   = ["func.create_user"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags
            .iter()
            .any(|d| d.kind == "UnusedFunc" && d.message.contains("func.unused_func")));
    }

    #[test]
    fn valid_file_has_no_errors() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[func.create_user]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[mod.api]
purpose = "test"
schemas = ["schema.user"]
funcs   = ["func.create_user"]
pipeline = ["func.create_user"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags.iter().all(|d| d.severity != "error"));
    }

    #[test]
    fn detects_pipeline_cycle() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[func.step_a]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[func.step_b]
intent = "test"
input  = ["schema.user"]
output = ["schema.user"]

[mod.api]
purpose = "test"
schemas = ["schema.user"]
funcs   = ["func.step_a", "func.step_b"]
pipeline = ["func.step_a", "func.step_b", "func.step_a"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags
            .iter()
            .any(|d| d.kind == "PipelineCycle" && d.message.contains("func.step_a")));
    }

    #[test]
    fn detects_pipeline_type_mismatch() {
        let ir = r#"
[meta]
name = "test"

[schema.user]
kind = "node"
type = "User"

[schema.product]
kind = "node"
type = "Product"

[func.create_user]
intent = "create user"
input  = ["schema.user"]
output = ["schema.user"]

[func.create_product]
intent = "create product"
input  = ["schema.product"]
output = ["schema.product"]

[mod.api]
purpose = "test"
schemas = ["schema.user", "schema.product"]
funcs   = ["func.create_user", "func.create_product"]
pipeline = ["func.create_user", "func.create_product"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags.iter().any(|d| d.kind == "PipelineTypeMismatch"));
    }

    #[test]
    fn valid_pipeline_composition() {
        let ir = r#"
[meta]
name = "test"

[schema.request]
kind = "node"
type = "Request"

[schema.user]
kind = "node"
type = "User"

[schema.response]
kind = "node"
type = "Response"

[func.parse_request]
intent = "parse request"
input  = ["schema.request"]
output = ["schema.user"]

[func.process_user]
intent = "process user"
input  = ["schema.user"]
output = ["schema.user", "schema.response"]

[func.send_response]
intent = "send response"
input  = ["schema.response"]
output = ["schema.response"]

[mod.api]
purpose = "test"
schemas = ["schema.request", "schema.user", "schema.response"]
funcs   = ["func.parse_request", "func.process_user", "func.send_response"]
pipeline = ["func.parse_request", "func.process_user", "func.send_response"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(!diags
            .iter()
            .any(|d| d.kind == "PipelineTypeMismatch" || d.severity == "error"));
    }
}
