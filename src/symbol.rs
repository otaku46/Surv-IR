use crate::ast::{Section, SurvFile};
use crate::diagnostic::Diagnostic;
use crate::imports::{FileImportContext, ImportEntry};
use crate::project::ProjectAST;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Schema,
    Func,
    Mod,
}

#[derive(Debug, Clone)]
pub struct SymbolEntry {
    pub kind: SymbolKind,
    pub package: String,
    pub fq_name: String,
    pub local_name: String,
    pub namespace: Option<String>,
    pub file: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    pub entries: Vec<SymbolEntry>,
}

impl SymbolTable {
    pub fn new(entries: Vec<SymbolEntry>) -> Self {
        Self { entries }
    }
}

pub fn build_symbol_table(project: &ProjectAST) -> (SymbolTable, Vec<Diagnostic>) {
    let mut package_map = HashMap::new();
    for (path, file) in &project.files {
        let package = file
            .package
            .clone()
            .unwrap_or_else(|| "default".to_string());
        package_map.insert(path.clone(), package);
    }
    build_symbol_table_with_packages(&project.files, &package_map)
}

pub fn build_symbol_table_with_packages(
    files: &[(PathBuf, SurvFile)],
    assignments: &HashMap<PathBuf, String>,
) -> (SymbolTable, Vec<Diagnostic>) {
    let mut entries = Vec::new();
    let mut diags = Vec::new();
    let mut defined: HashMap<(SymbolKind, String, Option<String>, String), (PathBuf, String)> =
        HashMap::new();

    for (path, file) in files {
        let namespace = file.namespace.clone();
        let package = assignments
            .get(path)
            .cloned()
            .or_else(|| file.package.clone())
            .unwrap_or_else(|| "default".to_string());

        for section in &file.sections {
            match section {
                Section::Schema(schema) => {
                    let entry = make_entry(
                        SymbolKind::Schema,
                        &package,
                        &namespace,
                        schema.name.clone(),
                        path,
                    );
                    report_duplicates(
                        &mut defined,
                        &mut diags,
                        &entry,
                        path,
                        &format!("{section:?}"),
                    );
                    entries.push(entry);
                }
                Section::Func(func) => {
                    let entry = make_entry(
                        SymbolKind::Func,
                        &package,
                        &namespace,
                        func.name.clone(),
                        path,
                    );
                    report_duplicates(
                        &mut defined,
                        &mut diags,
                        &entry,
                        path,
                        &format!("{section:?}"),
                    );
                    entries.push(entry);
                }
                Section::Mod(module) => {
                    let entry = make_entry(
                        SymbolKind::Mod,
                        &package,
                        &namespace,
                        module.name.clone(),
                        path,
                    );
                    report_duplicates(
                        &mut defined,
                        &mut diags,
                        &entry,
                        path,
                        &format!("{section:?}"),
                    );
                    entries.push(entry);
                }
                Section::Meta(_) => {}
                Section::Status(_) => {}
            }
        }
    }

    (SymbolTable::new(entries), diags)
}

fn make_entry(
    kind: SymbolKind,
    package: &str,
    namespace: &Option<String>,
    local_name: String,
    path: &Path,
) -> SymbolEntry {
    let fq_name = build_fq_name(kind, package, namespace.as_deref(), &local_name);
    SymbolEntry {
        kind,
        package: package.to_string(),
        fq_name,
        local_name,
        namespace: namespace.clone(),
        file: path.to_path_buf(),
    }
}

fn report_duplicates(
    defined: &mut HashMap<(SymbolKind, String, Option<String>, String), (PathBuf, String)>,
    diags: &mut Vec<Diagnostic>,
    entry: &SymbolEntry,
    current_path: &Path,
    definition: &str,
) {
    let key = (
        entry.kind,
        entry.package.clone(),
        entry.namespace.clone(),
        entry.local_name.clone(),
    );
    if let Some((existing, existing_definition)) = defined.get(&key) {
        if existing_definition != definition {
            diags.push(Diagnostic {
                severity: "warning".into(),
                kind: "W_AMBIGUOUS_NAME".into(),
                message: format!(
                    "Symbol '{}' has conflicting definitions in {} and {}",
                    entry.fq_name,
                    existing.display(),
                    current_path.display()
                ),
                location: current_path.display().to_string(),
            });
        }
    } else {
        defined.insert(key, (current_path.to_path_buf(), definition.to_string()));
    }
}

fn build_fq_name(
    kind: SymbolKind,
    package: &str,
    namespace: Option<&str>,
    local_name: &str,
) -> String {
    let prefix = match kind {
        SymbolKind::Schema => "schema",
        SymbolKind::Func => "func",
        SymbolKind::Mod => "mod",
    };
    let ns_segment = namespace.unwrap_or("global");
    format!("pkg.{package}.{prefix}.{ns_segment}.{local_name}")
}

fn emit_undefined(
    kind: SymbolKind,
    reference: &str,
    path: &Path,
    context: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let code = match kind {
        SymbolKind::Schema => "E_UNDEFINED_SCHEMA",
        SymbolKind::Func => "E_UNDEFINED_FUNC",
        SymbolKind::Mod => "E_UNDEFINED_MOD",
    };
    diags.push(Diagnostic {
        severity: "error".into(),
        kind: code.into(),
        message: format!("Reference '{}' is undefined", reference),
        location: format!("{}: {}", path.display(), context),
    });
}

fn emit_ambiguous(
    kind: SymbolKind,
    reference: &str,
    matches: Vec<&SymbolEntry>,
    path: &Path,
    context: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let code = "E_AMBIGUOUS_NAME";
    let candidates: Vec<String> = matches.iter().map(|entry| entry.fq_name.clone()).collect();
    let kind_str = match kind {
        SymbolKind::Schema => "schema",
        SymbolKind::Func => "func",
        SymbolKind::Mod => "mod",
    };
    diags.push(Diagnostic {
        severity: "error".into(),
        kind: code.into(),
        message: format!(
            "Ambiguous {kind_str} reference '{}'; candidates: {}",
            reference,
            candidates.join(", ")
        ),
        location: format!("{}: {}", path.display(), context),
    });
}

#[cfg(test)]
mod symbol_tests {
    use super::*;
    use crate::parser::parse_surv_file;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn file(path: &str, text: &str) -> (PathBuf, crate::ast::SurvFile) {
        let parsed = parse_surv_file(Cursor::new(text)).expect("parse");
        (PathBuf::from(path), parsed)
    }

    #[test]
    fn build_symbol_table_reports_duplicates() {
        let files = vec![
            file(
                "a.toml",
                r#"
[schema.user]
kind = "node"
type = "User"
"#,
            ),
            file(
                "b.toml",
                r#"
[schema.user]
kind = "node"
type = "User"
role = "event"
"#,
            ),
        ];

        let project = ProjectAST::from_files(files);
        let (_table, diags) = build_symbol_table(&project);
        assert!(diags.iter().any(|d| d.kind == "W_AMBIGUOUS_NAME"));
    }

    #[test]
    fn resolve_schema_reference_success() {
        let files = vec![file(
            "a.toml",
            r#"
[schema.user]
kind = "node"
type = "User"

[func.create]
intent = "test"
input = ["schema.user"]
output = ["schema.user"]
"#,
        )];

        let project = ProjectAST::from_files(files);
        let (symbols, _) = build_symbol_table(&project);
        let diags = resolve_schema_and_func_references(&project, &symbols);
        assert!(diags.is_empty());
    }

    #[test]
    fn resolve_schema_reference_missing() {
        let files = vec![file(
            "a.toml",
            r#"
[func.create]
intent = "test"
input = ["schema.unknown"]
output = []
"#,
        )];

        let project = ProjectAST::from_files(files);
        let (symbols, _) = build_symbol_table(&project);
        let diags = resolve_schema_and_func_references(&project, &symbols);
        assert!(diags
            .iter()
            .any(|d| d.kind == "E_UNDEFINED_SCHEMA" && d.location.contains("schema.unknown")));
    }
    #[test]
    fn package_specific_symbol_table() {
        let files = vec![
            file(
                "pkg/users/a.toml",
                r#"
[schema.user]
kind = "node"
type = "User"
"#,
            ),
            file(
                "pkg/auth/a.toml",
                r#"
[schema.user]
kind = "node"
type = "User"
"#,
            ),
        ];
        let mut assignments = HashMap::new();
        assignments.insert(PathBuf::from("pkg/users/a.toml"), "users".to_string());
        assignments.insert(PathBuf::from("pkg/auth/a.toml"), "auth".to_string());

        let (symbols, diags) = build_symbol_table_with_packages(&files, &assignments);
        assert!(diags.is_empty());
        assert_eq!(symbols.entries.len(), 2);
        let packages: Vec<_> = symbols.entries.iter().map(|e| e.package.clone()).collect();
        assert!(packages.contains(&"users".to_string()));
        assert!(packages.contains(&"auth".to_string()));
    }
}
pub fn resolve_names_with_packages(
    files: &[(PathBuf, SurvFile)],
    symbols: &SymbolTable,
    import_contexts: &[FileImportContext],
) -> Vec<Diagnostic> {
    resolve_project_references(files, symbols, import_contexts, None)
}

pub(crate) fn resolve_project_references(
    files: &[(PathBuf, SurvFile)],
    symbols: &SymbolTable,
    import_contexts: &[FileImportContext],
    manifest: Option<&crate::manifest::Manifest>,
) -> Vec<Diagnostic> {
    let ctx_map: HashMap<_, _> = import_contexts
        .iter()
        .map(|ctx| (ctx.file_path.clone(), ctx))
        .collect();
    let mut diags = Vec::new();

    for (path, file) in files {
        let Some(ctx) = ctx_map.get(path) else {
            continue;
        };
        let resolve = |kind, reference: &str, context: &str, diags: &mut Vec<_>| {
            resolve_reference(
                symbols, kind, reference, path, ctx, context, manifest, diags,
            );
        };
        for section in &file.sections {
            match section {
                Section::Func(func) => {
                    for (field, references) in [("input", &func.input), ("output", &func.output)] {
                        for reference in references {
                            resolve(
                                SymbolKind::Schema,
                                reference,
                                &format!("func.{}.{field}", func.name),
                                &mut diags,
                            );
                        }
                    }
                }
                Section::Schema(schema) if schema.kind == "edge" => {
                    for (field, reference) in [("from", &schema.from), ("to", &schema.to)] {
                        resolve(
                            SymbolKind::Schema,
                            reference,
                            &format!("schema.{}.{field}", schema.name),
                            &mut diags,
                        );
                    }
                }
                Section::Schema(schema) if schema.kind == "boundary" => {
                    for reference in &schema.over {
                        resolve(
                            SymbolKind::Schema,
                            reference,
                            &format!("schema.{}.over", schema.name),
                            &mut diags,
                        );
                    }
                }
                Section::Mod(module) => {
                    let context = format!("mod.{}", module.name);
                    for schema in &module.schemas {
                        resolve(SymbolKind::Schema, schema, &context, &mut diags);
                    }
                    for func in &module.funcs {
                        resolve(SymbolKind::Func, func, &context, &mut diags);
                    }
                    for step in module.get_pipeline_calls() {
                        resolve(SymbolKind::Func, &step, &context, &mut diags);
                        if !module.funcs.contains(&step) {
                            let step_matches =
                                reference_candidates(symbols, SymbolKind::Func, &step, ctx);
                            let listed = matches!(step_matches.as_slice(), [target] if module.funcs.iter().any(|func| {
                                matches!(reference_candidates(symbols, SymbolKind::Func, func, ctx).as_slice(), [candidate] if candidate.fq_name == target.fq_name)
                            }));
                            if !listed {
                                diags.push(Diagnostic {
                                    severity: "error".into(),
                                    kind: "PipelineFuncNotInModule".into(),
                                    message: format!(
                                        "{context}: pipeline step {step} is absent from funcs"
                                    ),
                                    location: path.display().to_string(),
                                });
                            }
                        }
                    }
                    for submod in &module.submods {
                        let reference = format!("mod.{}.{submod}", module.name);
                        resolve(SymbolKind::Mod, &reference, &context, &mut diags);
                    }
                }
                _ => {}
            }
        }
    }

    diags
}

fn resolve_reference(
    symbols: &SymbolTable,
    kind: SymbolKind,
    reference: &str,
    path: &Path,
    ctx: &FileImportContext,
    context: &str,
    manifest: Option<&crate::manifest::Manifest>,
    diags: &mut Vec<Diagnostic>,
) {
    if reference.is_empty() {
        return;
    }
    match reference_candidates(symbols, kind, reference, ctx).as_slice() {
        [] => emit_undefined(
            kind,
            reference,
            path,
            &format!("{context}({reference})"),
            diags,
        ),
        [entry] => {
            if let Some(diag) = check_package_dependency(manifest, ctx, &entry.package) {
                diags.push(diag);
            }
        }
        matches => emit_ambiguous(kind, reference, matches.to_vec(), path, context, diags),
    }
}

pub(crate) fn check_package_dependency(
    manifest: Option<&crate::manifest::Manifest>,
    ctx: &FileImportContext,
    target: &str,
) -> Option<Diagnostic> {
    let package = manifest?.packages.get(&ctx.self_package)?;
    if target == ctx.self_package || package.depends.iter().any(|dep| dep == target) {
        return None;
    }
    Some(Diagnostic {
        severity: "error".into(),
        kind: "E_UNDECLARED_DEPENDENCY".into(),
        message: format!(
            "Package '{}' does not declare a dependency on '{target}'",
            ctx.self_package
        ),
        location: ctx.file_path.display().to_string(),
    })
}

pub fn reference_candidates<'a>(
    symbols: &'a SymbolTable,
    kind: SymbolKind,
    reference: &str,
    ctx: &FileImportContext,
) -> Vec<&'a SymbolEntry> {
    let category = match kind {
        SymbolKind::Schema => "schema",
        SymbolKind::Func => "func",
        SymbolKind::Mod => "mod",
    };
    let local_prefix = format!("{category}.");
    let marker = format!(".{category}.");
    let reference = reference.trim();
    let (prefix, name) = if let Some(name) = reference.strip_prefix(&local_prefix) {
        (None, name)
    } else if let Some((prefix, name)) = reference.split_once(&marker) {
        (Some(prefix), name)
    } else {
        (None, reference)
    };
    let candidates = || {
        symbols
            .entries
            .iter()
            .filter(|e| e.kind == kind && e.local_name == name)
    };
    if let Some(prefix) = prefix {
        let alias = ctx
            .imports
            .iter()
            .find(|import| import.alias.as_deref() == Some(prefix));
        return candidates()
            .filter(|e| match alias {
                Some(import) => e.package == import.package,
                None => e.package == prefix || e.namespace.as_deref() == Some(prefix),
            })
            .collect();
    }

    let same_file: Vec<_> = candidates().filter(|e| e.file == ctx.file_path).collect();
    if !same_file.is_empty() {
        return same_file;
    }
    let same_package: Vec<_> = candidates()
        .filter(|e| e.package == ctx.self_package)
        .collect();
    let same_namespace: Vec<_> = same_package
        .iter()
        .copied()
        .filter(|e| e.namespace == ctx.namespace)
        .collect();
    if !same_namespace.is_empty() {
        return same_namespace;
    }
    if !same_package.is_empty() {
        return same_package;
    }
    let imported: Vec<_> = candidates()
        .filter(|e| ctx.imports.iter().any(|i| i.package == e.package))
        .collect();
    if !imported.is_empty() {
        return imported;
    }
    candidates().collect()
}

pub fn resolve_schema_and_func_references(
    project: &ProjectAST,
    symbols: &SymbolTable,
) -> Vec<Diagnostic> {
    let contexts: Vec<FileImportContext> = project
        .files
        .iter()
        .map(|(path, file)| FileImportContext {
            file_path: path.clone(),
            self_package: file.package.clone().unwrap_or_else(|| "default".into()),
            namespace: file.namespace.clone(),
            imports: file
                .imports
                .iter()
                .map(|imp| ImportEntry {
                    package: imp.target.clone(),
                    alias: imp.alias.clone(),
                })
                .collect(),
        })
        .collect();
    resolve_names_with_packages(&project.files, symbols, &contexts)
}

#[cfg(test)]
mod resolve_tests {
    use super::*;
    use crate::parser::parse_surv_file;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn file(path: &str, text: &str) -> (PathBuf, crate::ast::SurvFile) {
        let parsed = parse_surv_file(Cursor::new(text)).expect("parse");
        (PathBuf::from(path), parsed)
    }

    #[test]
    fn build_symbol_table_reports_duplicates() {
        let files = vec![
            file(
                "a.toml",
                r#"
[schema.user]
kind = "node"
type = "User"
"#,
            ),
            file(
                "b.toml",
                r#"
[schema.user]
kind = "node"
type = "User"
role = "event"
"#,
            ),
        ];

        let project = ProjectAST::from_files(files);
        let (_table, diags) = build_symbol_table(&project);
        assert!(diags.iter().any(|d| d.kind == "W_AMBIGUOUS_NAME"));
    }

    #[test]
    fn resolve_schema_reference_success() {
        let files = vec![file(
            "a.toml",
            r#"
[schema.user]
kind = "node"
type = "User"

[func.create]
intent = "test"
input = ["schema.user"]
output = ["schema.user"]
"#,
        )];

        let project = ProjectAST::from_files(files.clone());
        let (symbols, _) = build_symbol_table(&project);
        let contexts = vec![FileImportContext {
            file_path: PathBuf::from("a.toml"),
            self_package: "default".into(),
            namespace: None,
            imports: Vec::new(),
        }];
        let diags = resolve_names_with_packages(&project.files, &symbols, &contexts);
        assert!(diags.is_empty());
    }

    #[test]
    fn resolve_schema_reference_missing() {
        let files = vec![file(
            "a.toml",
            r#"
[func.create]
intent = "test"
input = ["schema.unknown"]
output = []
"#,
        )];

        let project = ProjectAST::from_files(files.clone());
        let (symbols, _) = build_symbol_table(&project);
        let contexts = vec![FileImportContext {
            file_path: PathBuf::from("a.toml"),
            self_package: "default".into(),
            namespace: None,
            imports: Vec::new(),
        }];
        let diags = resolve_names_with_packages(&project.files, &symbols, &contexts);
        assert!(diags
            .iter()
            .any(|d| d.kind == "E_UNDEFINED_SCHEMA" && d.location.contains("schema.unknown")));
    }

    #[test]
    fn package_specific_symbol_table() {
        let files = vec![
            file(
                "pkg/users/a.toml",
                r#"
[schema.user]
kind = "node"
type = "User"
"#,
            ),
            file(
                "pkg/auth/a.toml",
                r#"
[schema.user]
kind = "node"
type = "User"
"#,
            ),
        ];
        let mut assignments = HashMap::new();
        assignments.insert(PathBuf::from("pkg/users/a.toml"), "users".to_string());
        assignments.insert(PathBuf::from("pkg/auth/a.toml"), "auth".to_string());

        let (symbols, diags) = build_symbol_table_with_packages(&files, &assignments);
        assert!(diags.is_empty());
        assert_eq!(symbols.entries.len(), 2);
        let packages: Vec<_> = symbols.entries.iter().map(|e| e.package.clone()).collect();
        assert!(packages.contains(&"users".to_string()));
        assert!(packages.contains(&"auth".to_string()));
    }
}
