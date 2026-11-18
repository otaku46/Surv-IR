use crate::ast::{FuncSection, ModSection, PipelineStep, SchemaSection, Section, SurvFile};
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
    namespace: Option<String>,
    package: Option<String>,
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
            namespace: file.namespace.clone(),
            package: file.package.clone(),
        }
    }

    fn normalize(&self, reference: &str) -> String {
        let reference = reference.trim();
        for prefix in [self.namespace.as_deref(), self.package.as_deref()]
            .into_iter()
            .flatten()
        {
            let marker = format!("{prefix}.");
            if let Some(rest) = reference.strip_prefix(&marker) {
                if rest.starts_with("schema.")
                    || rest.starts_with("func.")
                    || rest.starts_with("mod.")
                {
                    return rest.to_string();
                }
            }
        }
        reference.to_string()
    }

    fn schema_key(&self, reference: &str) -> String {
        let reference = self.normalize(reference);
        if reference.starts_with("schema.") {
            reference
        } else {
            format!("schema.{reference}")
        }
    }

    fn func_key(&self, reference: &str) -> String {
        let reference = self.normalize(reference);
        if reference.starts_with("func.") {
            reference
        } else {
            format!("func.{reference}")
        }
    }

    fn mod_key(&self, reference: &str) -> String {
        let reference = self.normalize(reference);
        if reference.starts_with("mod.") {
            reference
        } else {
            format!("mod.{reference}")
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
            if !index.schemas.contains_key(&index.schema_key(schema)) {
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
            if !index.schemas.contains_key(&index.schema_key(schema)) {
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
        for submod in &module.submods {
            let expected_full_name = index.mod_key(&format!("{}.{}", mod_id(module), submod));
            if !index.mods.contains_key(&expected_full_name) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedSubmod".into(),
                    message: format!("mod {}: submod {} is not defined", mod_id(module), submod),
                    location: format!("{}.submods({})", mod_id(module), submod),
                });
            }
        }

        for schema in &module.schemas {
            if !index.schemas.contains_key(&index.schema_key(schema)) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedSchemaInMod".into(),
                    message: format!("mod {}: schema {} is not defined", mod_id(module), schema),
                    location: format!("{}.schemas({})", mod_id(module), schema),
                });
            }
        }

        for func in &module.funcs {
            if !index.funcs.contains_key(&index.func_key(func)) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedFuncInMod".into(),
                    message: format!("mod {}: func {} is not defined", mod_id(module), func),
                    location: format!("{}.funcs({})", mod_id(module), func),
                });
            }
        }

        let pipeline_calls = module.get_pipeline_calls();
        for step in &pipeline_calls {
            if !index.funcs.contains_key(&index.func_key(step)) {
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
        if !schema.from.is_empty() && !index.schemas.contains_key(&index.schema_key(&schema.from)) {
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
        if !schema.to.is_empty() && !index.schemas.contains_key(&index.schema_key(&schema.to)) {
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
        if !schema.base.is_empty() && !index.schemas.contains_key(&index.schema_key(&schema.base)) {
            diags.push(Diagnostic {
                severity: "error".into(),
                kind: "UndefinedSchemaInBase".into(),
                message: format!(
                    "schema {}: base {} is not defined",
                    schema_id(schema),
                    schema.base
                ),
                location: format!("{}.base({})", schema_id(schema), schema.base),
            });
        }
        for over in &schema.over {
            if !index.schemas.contains_key(&index.schema_key(over)) {
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
}

fn check_pipeline_semantics(index: &FileIndex<'_>, diags: &mut Vec<Diagnostic>) {
    for module in index.mods.values() {
        check_pipeline(
            module,
            index,
            &module.pipeline,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
            diags,
        );
    }
}

fn check_pipeline(
    module: &ModSection,
    index: &FileIndex<'_>,
    steps: &[PipelineStep],
    seen: &mut BTreeSet<String>,
    previous: &mut BTreeSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    for step in steps {
        match step {
            PipelineStep::Sequential(steps) => {
                check_pipeline(module, index, steps, seen, previous, diags);
            }
            PipelineStep::Parallel(steps) => {
                check_pipeline_alternatives(module, index, steps.iter(), seen, previous, diags);
            }
            PipelineStep::Branch {
                on_true, on_false, ..
            } => {
                check_pipeline_alternatives(
                    module,
                    index,
                    [on_true.as_ref(), on_false.as_ref()].into_iter(),
                    seen,
                    previous,
                    diags,
                );
            }
            PipelineStep::Call(name) => {
                let key = index.func_key(name);
                if !module
                    .funcs
                    .iter()
                    .any(|listed| index.func_key(listed) == key)
                    && index.funcs.contains_key(&key)
                {
                    diags.push(Diagnostic {
                        severity: "error".into(),
                        kind: "PipelineFuncNotInModule".into(),
                        message: format!(
                            "mod {}: pipeline step {name} is absent from funcs",
                            mod_id(module)
                        ),
                        location: format!("{}.pipeline({name})", mod_id(module)),
                    });
                }
                if !seen.insert(key.clone()) {
                    diags.push(Diagnostic {
                        severity: "error".into(),
                        kind: "PipelineCycle".into(),
                        message: format!(
                            "mod {}: {name} appears more than once along a pipeline path",
                            mod_id(module)
                        ),
                        location: format!("{}.pipeline", mod_id(module)),
                    });
                }
                if let Some(next) = index.funcs.get(&key) {
                    for before in previous.iter() {
                        if let Some(prior) = index.funcs.get(before) {
                            check_pipeline_flow(module, index, before, &key, prior, next, diags);
                        }
                    }
                }
                previous.clear();
                previous.insert(key);
            }
        }
    }
}

fn check_pipeline_alternatives<'a>(
    module: &'a ModSection,
    index: &FileIndex<'_>,
    alternatives: impl Iterator<Item = &'a PipelineStep>,
    seen: &mut BTreeSet<String>,
    previous: &mut BTreeSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut exits = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for alternative in alternatives {
        let mut path_seen = seen.clone();
        let mut path_previous = previous.clone();
        check_pipeline(
            module,
            index,
            std::slice::from_ref(alternative),
            &mut path_seen,
            &mut path_previous,
            diags,
        );
        visited.extend(path_seen);
        exits.extend(path_previous);
    }
    seen.extend(visited);
    if !exits.is_empty() {
        *previous = exits;
    }
}

fn check_pipeline_flow(
    module: &ModSection,
    index: &FileIndex<'_>,
    before: &str,
    after: &str,
    prior: &FuncSection,
    next: &FuncSection,
    diags: &mut Vec<Diagnostic>,
) {
    let outputs: BTreeSet<String> = prior
        .output
        .iter()
        .map(|schema| index.schema_key(schema))
        .collect();
    let inputs: BTreeSet<String> = next
        .input
        .iter()
        .map(|schema| index.schema_key(schema))
        .collect();
    if !outputs.is_empty() && !inputs.is_empty() && outputs.is_disjoint(&inputs) {
        diags.push(Diagnostic {
            severity: "warning".into(),
            kind: "PipelineTypeMismatch".into(),
            message: format!(
                "mod {}: pipeline step {before} -> {after} has no shared schema between output and input",
                mod_id(module),
            ),
            location: format!("{}.pipeline({before}->{after})", mod_id(module)),
        });
    }
}

fn check_unused_definitions(index: &FileIndex<'_>, diags: &mut Vec<Diagnostic>) {
    let mut used_schemas = BTreeSet::new();

    for func in index.funcs.values() {
        for schema in func.input.iter().chain(func.output.iter()) {
            used_schemas.insert(index.schema_key(schema));
        }
    }

    for module in index.mods.values() {
        for schema in &module.schemas {
            used_schemas.insert(index.schema_key(schema));
        }
    }

    for schema in index.schemas.values() {
        for reference in [&schema.from, &schema.to, &schema.base]
            .into_iter()
            .chain(&schema.over)
        {
            if !reference.is_empty() {
                used_schemas.insert(index.schema_key(reference));
            }
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
            used_funcs.insert(index.func_key(func));
        }
        let pipeline_calls = module.get_pipeline_calls();
        for step in &pipeline_calls {
            used_funcs.insert(index.func_key(step));
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
    fn detects_undefined_submod() {
        let ir = r#"
[meta]
name = "test"

[mod.api]
purpose = "test"
submods = ["nonexistent"]
"#;

        let file = parse(ir);
        let diags = check_surv_file(&file);
        assert!(diags.iter().any(|d| d.kind == "UndefinedSubmod"));
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
