use crate::ast::{FuncSection, ModSection, PipelineStep, Section};
use crate::diagnostic::Diagnostic;
use crate::imports::{parse_imports_with_alias, FileImportContext, ImportEntry};
use crate::loader::{load_manifest, load_project_with_manifest};
use crate::manifest::Manifest;
use crate::package::assign_packages_to_files;
use crate::parser::parse_file;
use crate::project::{NormalizedRequire, ProjectAST};
use crate::symbol::{
    build_symbol_table_with_packages, check_package_dependency, reference_candidates,
    resolve_project_references, SymbolKind, SymbolTable,
};
use std::collections::HashMap;
use std::path::Path;

pub struct ProjectContext {
    pub project: ProjectAST,
    pub manifest: Option<Manifest>,
    pub contexts: Vec<FileImportContext>,
    pub symbols: SymbolTable,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn load_project_context(
    manifest_path: &Path,
) -> Result<ProjectContext, Box<dyn std::error::Error>> {
    let manifest = load_manifest(manifest_path)?;
    let mut project = load_project_with_manifest(manifest_path, &manifest)?;
    let root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let (contexts, symbols, diagnostics) = prepare_project(&mut project, Some(&manifest), root);
    ensure_context_ok(&diagnostics)?;
    Ok(ProjectContext {
        project,
        manifest: Some(manifest),
        contexts,
        symbols,
        diagnostics,
    })
}

pub fn load_ir_context(path: &Path) -> Result<ProjectContext, Box<dyn std::error::Error>> {
    let file = parse_file(path)?;
    let mut project = ProjectAST::from_files(vec![(path.to_path_buf(), file)]);
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let (contexts, symbols, diagnostics) = prepare_project(&mut project, None, root);
    ensure_context_ok(&diagnostics)?;
    Ok(ProjectContext {
        project,
        manifest: None,
        contexts,
        symbols,
        diagnostics,
    })
}

fn ensure_context_ok(diagnostics: &[Diagnostic]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(diagnostic) = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == "error")
    {
        return Err(format!(
            "{}: [{}] {}",
            diagnostic.location, diagnostic.kind, diagnostic.message
        )
        .into());
    }
    Ok(())
}

pub fn check_project(project: &ProjectAST) -> Vec<Diagnostic> {
    let contexts = standalone_contexts(project);
    let (symbols, mut diagnostics) = symbols_for_project(project, &contexts);
    diagnostics.extend(check_with_contexts(project, &contexts, &symbols, None));
    diagnostics
}

pub fn check_project_manifest(path: &Path) -> Result<Vec<Diagnostic>, Box<dyn std::error::Error>> {
    let manifest = load_manifest(path)?;
    let project = load_project_with_manifest(path, &manifest)?;
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    Ok(check_project_with_manifest(project, &manifest, root))
}

fn check_project_with_manifest(
    mut project: ProjectAST,
    manifest: &Manifest,
    root: &Path,
) -> Vec<Diagnostic> {
    let (contexts, symbols, mut diagnostics) = prepare_project(&mut project, Some(manifest), root);
    diagnostics.extend(check_with_contexts(
        &project,
        &contexts,
        &symbols,
        Some(manifest),
    ));
    diagnostics
}

pub(crate) fn prepare_project(
    project: &mut ProjectAST,
    manifest: Option<&Manifest>,
    root: &Path,
) -> (Vec<FileImportContext>, SymbolTable, Vec<Diagnostic>) {
    let (contexts, mut diagnostics) = if let Some(manifest) = manifest {
        let (assignments, mut diagnostics) =
            assign_packages_to_files(manifest, root, &project.files);
        let assigned: HashMap<_, _> = assignments
            .iter()
            .map(|assignment| (&assignment.file_path, &assignment.package))
            .collect();
        for (path, file) in &mut project.files {
            if file.namespace.is_none() {
                file.namespace = assigned
                    .get(path)
                    .and_then(|package| manifest.packages.get(*package))
                    .and_then(|package| package.namespace.clone());
            }
        }
        let (contexts, imports) = parse_imports_with_alias(manifest, &assignments, &project.files);
        diagnostics.extend(imports);
        (contexts, diagnostics)
    } else {
        (standalone_contexts(project), Vec::new())
    };
    let (symbols, duplicates) = symbols_for_project(project, &contexts);
    diagnostics.extend(duplicates);
    (contexts, symbols, diagnostics)
}

fn standalone_contexts(project: &ProjectAST) -> Vec<FileImportContext> {
    project
        .files
        .iter()
        .map(|(path, file)| FileImportContext {
            file_path: path.clone(),
            self_package: file.package.clone().unwrap_or_else(|| "default".into()),
            namespace: file.namespace.clone(),
            imports: file
                .imports
                .iter()
                .map(|import| {
                    let (package, alias) = crate::imports::parse_import_entry(&import.target)
                        .unwrap_or_else(|_| (import.target.clone(), None));
                    ImportEntry {
                        package,
                        alias: alias.or_else(|| import.alias.clone()),
                    }
                })
                .collect(),
        })
        .collect()
}

fn symbols_for_project(
    project: &ProjectAST,
    contexts: &[FileImportContext],
) -> (SymbolTable, Vec<Diagnostic>) {
    let packages = contexts
        .iter()
        .map(|ctx| (ctx.file_path.clone(), ctx.self_package.clone()))
        .collect();
    build_symbol_table_with_packages(&project.files, &packages)
}

pub(crate) fn check_with_contexts(
    project: &ProjectAST,
    contexts: &[FileImportContext],
    symbols: &SymbolTable,
    manifest: Option<&Manifest>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(resolve_project_references(
        &project.files,
        symbols,
        contexts,
        manifest,
    ));

    let by_file: HashMap<_, _> = contexts.iter().map(|ctx| (&ctx.file_path, ctx)).collect();
    let mut requires = Vec::new();
    for (path, file) in &project.files {
        let ctx = by_file[path];
        for import in &ctx.imports {
            diagnostics.extend(check_package_dependency(manifest, ctx, &import.package));
        }

        for requirement in &file.requires {
            let target = &requirement.target;
            let matches = reference_candidates(symbols, SymbolKind::Mod, target, ctx);
            let [entry] = matches.as_slice() else {
                let (kind, message) = if matches.is_empty() {
                    (
                        "UnresolvedRequire",
                        format!("Required module '{target}' does not exist"),
                    )
                } else {
                    (
                        "AmbiguousRequire",
                        format!("Required module '{target}' has multiple definitions"),
                    )
                };
                diagnostics.push(Diagnostic {
                    severity: "error".into(),
                    kind: kind.into(),
                    message,
                    location: path.display().to_string(),
                });
                continue;
            };
            diagnostics.extend(check_package_dependency(manifest, ctx, &entry.package));
            let file_module_count = file
                .sections
                .iter()
                .filter(|section| matches!(section, Section::Mod(_)))
                .count();
            for section in &file.sections {
                if let Section::Mod(module) = section {
                    let reference = format!("mod.{}", module.name);
                    if let [source] =
                        reference_candidates(symbols, SymbolKind::Mod, &reference, ctx).as_slice()
                    {
                        if source.fq_name != entry.fq_name || file_module_count == 1 {
                            requires.push(NormalizedRequire {
                                from_mod: source.fq_name.clone(),
                                to_mod: entry.fq_name.clone(),
                                raw: target.clone(),
                                file: path.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    check_cycles(&requires, &mut diagnostics);
    diagnostics.extend(check_project_semantics(project, contexts, symbols));
    diagnostics
}

fn check_project_semantics(
    project: &ProjectAST,
    contexts: &[FileImportContext],
    symbols: &SymbolTable,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut funcs: HashMap<String, &FuncSection> = HashMap::new();
    let mut sections: HashMap<(std::path::PathBuf, SymbolKind, String), &Section> = HashMap::new();
    for (path, file) in &project.files {
        for section in &file.sections {
            let (kind, name) = match section {
                Section::Schema(schema) => (SymbolKind::Schema, &schema.name),
                Section::Func(func) => (SymbolKind::Func, &func.name),
                Section::Mod(module) => (SymbolKind::Mod, &module.name),
                _ => continue,
            };
            sections.insert((path.clone(), kind, name.clone()), section);
        }
    }
    for entry in &symbols.entries {
        let Some(section) =
            sections.get(&(entry.file.clone(), entry.kind, entry.local_name.clone()))
        else {
            continue;
        };
        if let Section::Func(func) = section {
            funcs.insert(entry.fq_name.clone(), func);
        }
    }

    let contexts_by_file: HashMap<_, _> =
        contexts.iter().map(|ctx| (&ctx.file_path, ctx)).collect();
    let mut used_schemas = std::collections::BTreeSet::new();
    let mut used_funcs = std::collections::BTreeSet::new();

    for (path, file) in &project.files {
        let Some(ctx) = contexts_by_file.get(path) else {
            continue;
        };
        for section in &file.sections {
            let Section::Mod(module) = section else {
                continue;
            };
            for reference in &module.schemas {
                used_schemas.extend(canonical_names(symbols, SymbolKind::Schema, reference, ctx));
            }
            for reference in &module.funcs {
                used_funcs.extend(canonical_names(symbols, SymbolKind::Func, reference, ctx));
            }
            for reference in module.get_pipeline_calls() {
                used_funcs.extend(canonical_names(symbols, SymbolKind::Func, &reference, ctx));
            }
            check_pipeline_semantics_project(module, symbols, &funcs, ctx, &mut diags);
        }
        for section in &file.sections {
            match section {
                Section::Func(func) => {
                    for reference in func.input.iter().chain(&func.output) {
                        used_schemas.extend(canonical_names(
                            symbols,
                            SymbolKind::Schema,
                            reference,
                            ctx,
                        ));
                    }
                }
                Section::Schema(schema) => {
                    for reference in [&schema.from, &schema.to, &schema.base]
                        .into_iter()
                        .chain(&schema.over)
                    {
                        if !reference.is_empty() {
                            used_schemas.extend(canonical_names(
                                symbols,
                                SymbolKind::Schema,
                                reference,
                                ctx,
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    for entry in &symbols.entries {
        match entry.kind {
            SymbolKind::Schema if !used_schemas.contains(&entry.fq_name) => {
                diags.push(Diagnostic {
                    severity: "warning".into(),
                    kind: "UnusedSchema".into(),
                    message: format!(
                        "schema {} is defined but never referenced",
                        entry.local_name
                    ),
                    location: entry.fq_name.clone(),
                });
            }
            SymbolKind::Func if !used_funcs.contains(&entry.fq_name) => {
                diags.push(Diagnostic {
                    severity: "warning".into(),
                    kind: "UnusedFunc".into(),
                    message: format!(
                        "func {} is defined but never referenced in any mod",
                        entry.local_name
                    ),
                    location: entry.fq_name.clone(),
                });
            }
            _ => {}
        }
    }
    diags
}

fn canonical_names(
    symbols: &SymbolTable,
    kind: SymbolKind,
    reference: &str,
    ctx: &FileImportContext,
) -> Vec<String> {
    let candidates = reference_candidates(symbols, kind, reference, ctx);
    if candidates.is_empty() {
        Vec::new()
    } else {
        candidates
            .into_iter()
            .map(|entry| entry.fq_name.clone())
            .collect()
    }
}

fn canonical_name(
    symbols: &SymbolTable,
    kind: SymbolKind,
    reference: &str,
    ctx: &FileImportContext,
) -> String {
    let candidates = reference_candidates(symbols, kind, reference, ctx);
    match candidates.as_slice() {
        [entry] => entry.fq_name.clone(),
        _ => format!("raw:{reference}"),
    }
}

fn check_pipeline_semantics_project(
    module: &ModSection,
    symbols: &SymbolTable,
    funcs: &HashMap<String, &FuncSection>,
    ctx: &FileImportContext,
    diags: &mut Vec<Diagnostic>,
) {
    walk_pipeline_project(
        module,
        &module.pipeline,
        symbols,
        funcs,
        ctx,
        &mut std::collections::BTreeSet::new(),
        &mut std::collections::BTreeSet::new(),
        diags,
    );
}

#[allow(clippy::too_many_arguments)]
fn walk_pipeline_project(
    module: &ModSection,
    steps: &[PipelineStep],
    symbols: &SymbolTable,
    funcs: &HashMap<String, &FuncSection>,
    ctx: &FileImportContext,
    seen: &mut std::collections::BTreeSet<String>,
    previous: &mut std::collections::BTreeSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    for step in steps {
        match step {
            PipelineStep::Sequential(inner) => {
                walk_pipeline_project(module, inner, symbols, funcs, ctx, seen, previous, diags)
            }
            PipelineStep::Parallel(inner) => walk_pipeline_alternatives(
                module,
                inner.iter(),
                symbols,
                funcs,
                ctx,
                seen,
                previous,
                diags,
            ),
            PipelineStep::Branch {
                on_true, on_false, ..
            } => walk_pipeline_alternatives(
                module,
                [on_true.as_ref(), on_false.as_ref()].into_iter(),
                symbols,
                funcs,
                ctx,
                seen,
                previous,
                diags,
            ),
            PipelineStep::Call(name) => {
                let key = canonical_name(symbols, SymbolKind::Func, name, ctx);
                // Membership is validated by resolve_project_references.
                if !seen.insert(key.clone()) {
                    diags.push(Diagnostic {
                        severity: "error".into(),
                        kind: "PipelineCycle".into(),
                        message: format!(
                            "mod {}: {name} appears more than once along a pipeline path",
                            module.name
                        ),
                        location: format!("mod.{}.pipeline", module.name),
                    });
                }
                if let Some(next) = funcs.get(&key) {
                    for before in previous.iter() {
                        if let Some(prior) = funcs.get(before) {
                            let outputs: std::collections::BTreeSet<_> = prior
                                .output
                                .iter()
                                .flat_map(|reference| {
                                    canonical_names(symbols, SymbolKind::Schema, reference, ctx)
                                })
                                .collect();
                            let inputs: std::collections::BTreeSet<_> = next
                                .input
                                .iter()
                                .flat_map(|reference| {
                                    canonical_names(symbols, SymbolKind::Schema, reference, ctx)
                                })
                                .collect();
                            if !outputs.is_empty()
                                && !inputs.is_empty()
                                && outputs.is_disjoint(&inputs)
                            {
                                diags.push(Diagnostic {
                                    severity: "warning".into(),
                                    kind: "PipelineTypeMismatch".into(),
                                    message: format!(
                                        "mod {}: pipeline step {before} -> {name} has no shared schema between output and input",
                                        module.name
                                    ),
                                    location: format!(
                                        "mod.{}.pipeline({}->{name})",
                                        module.name, before
                                    ),
                                });
                            }
                        }
                    }
                }
                previous.clear();
                previous.insert(key);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_pipeline_alternatives<'a>(
    module: &'a ModSection,
    alternatives: impl Iterator<Item = &'a PipelineStep>,
    symbols: &SymbolTable,
    funcs: &HashMap<String, &FuncSection>,
    ctx: &FileImportContext,
    seen: &mut std::collections::BTreeSet<String>,
    previous: &mut std::collections::BTreeSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut exits = std::collections::BTreeSet::new();
    let mut visited = std::collections::BTreeSet::new();
    for alternative in alternatives {
        let mut path_seen = seen.clone();
        let mut path_previous = previous.clone();
        walk_pipeline_project(
            module,
            std::slice::from_ref(alternative),
            symbols,
            funcs,
            ctx,
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

fn check_cycles(requires: &[NormalizedRequire], diagnostics: &mut Vec<Diagnostic>) {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut edge_map: HashMap<(String, String), &NormalizedRequire> = HashMap::new();

    for edge in requires {
        graph.entry(edge.from_mod.clone()).or_default();
        graph.entry(edge.to_mod.clone()).or_default();
        graph
            .entry(edge.from_mod.clone())
            .or_default()
            .push(edge.to_mod.clone());
        edge_map.insert((edge.from_mod.clone(), edge.to_mod.clone()), edge);
    }

    #[derive(Copy, Clone, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    fn dfs(
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        color: &mut HashMap<String, Color>,
        stack: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(node.to_string(), Color::Gray);
        stack.push(node.to_string());

        if let Some(neighbors) = graph.get(node) {
            for next in neighbors {
                match color.get(next).copied().unwrap_or(Color::White) {
                    Color::White => dfs(next, graph, color, stack, cycles),
                    Color::Gray => {
                        if let Some(pos) = stack.iter().position(|s| s == next) {
                            cycles.push(stack[pos..].to_vec());
                        }
                    }
                    Color::Black => {}
                }
            }
        }

        color.insert(node.to_string(), Color::Black);
        stack.pop();
    }

    let mut color: HashMap<String, Color> =
        graph.keys().map(|k| (k.clone(), Color::White)).collect();
    let mut cycles = Vec::new();

    for node in graph.keys() {
        if color.get(node) == Some(&Color::White) {
            let mut stack = Vec::new();
            dfs(node, &graph, &mut color, &mut stack, &mut cycles);
        }
    }

    for cycle in cycles {
        let mut path = cycle.clone();
        path.push(cycle[0].clone());
        let msg = format!("Require cycle detected: {}", path.join(" -> "));
        let file = path
            .windows(2)
            .filter_map(|pair| {
                if let [from, to] = pair {
                    edge_map
                        .get(&(from.clone(), to.clone()))
                        .map(|edge| edge.file.display().to_string())
                } else {
                    None
                }
            })
            .next()
            .unwrap_or_else(|| "require graph".into());
        diagnostics.push(Diagnostic {
            severity: "error".into(),
            kind: "RequireCycle".into(),
            message: msg,
            location: file,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_surv_file;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn file(path: &str, text: &str) -> (PathBuf, crate::ast::SurvFile) {
        let parsed = parse_surv_file(Cursor::new(text)).expect("parse");
        (PathBuf::from(path), parsed)
    }

    #[test]
    fn reports_unresolved_require() {
        let files = vec![file(
            "a.toml",
            r#"
require = ["mod.missing"]

[mod.alpha]
purpose = "test"
schemas = []
funcs = []
"#,
        )];

        let project = ProjectAST::from_files(files);
        let diags = check_project(&project);
        assert!(diags
            .iter()
            .any(|d| d.kind == "UnresolvedRequire" && d.location.contains("a.toml")));
    }

    #[test]
    fn detects_require_cycles() {
        let files = vec![
            file(
                "a.toml",
                r#"
require = ["mod.beta"]

[mod.alpha]
purpose = "test"
schemas = []
funcs = []
"#,
            ),
            file(
                "b.toml",
                r#"
require = ["mod.alpha"]

[mod.beta]
purpose = "test"
schemas = []
funcs = []
"#,
            ),
        ];

        let project = ProjectAST::from_files(files);
        let diags = check_project(&project);
        assert!(diags.iter().any(|d| d.kind == "RequireCycle"));
    }
}
