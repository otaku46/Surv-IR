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
    let mut defined: HashMap<(SymbolKind, String, Option<String>, String), PathBuf> =
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
                    report_duplicates(&mut defined, &mut diags, &entry, path);
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
                    report_duplicates(&mut defined, &mut diags, &entry, path);
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
                    report_duplicates(&mut defined, &mut diags, &entry, path);
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
    defined: &mut HashMap<(SymbolKind, String, Option<String>, String), PathBuf>,
    diags: &mut Vec<Diagnostic>,
    entry: &SymbolEntry,
    current_path: &Path,
) {
    let key = (
        entry.kind,
        entry.package.clone(),
        entry.namespace.clone(),
        entry.local_name.clone(),
    );
    if let Some(existing) = defined.get(&key) {
        diags.push(Diagnostic {
            severity: "warning".into(),
            kind: "W_AMBIGUOUS_NAME".into(),
            message: format!(
                "Symbol '{}' is defined in both {} and {}",
                entry.fq_name,
                existing.display(),
                current_path.display()
            ),
            location: current_path.display().to_string(),
        });
    } else {
        defined.insert(key, current_path.to_path_buf());
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

fn extract_local_name(reference: &str) -> &str {
    reference
        .split_once('.')
        .map(|(_, rest)| rest)
        .unwrap_or(reference)
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
    let code = "W_AMBIGUOUS_NAME";
    let candidates: Vec<String> = matches.iter().map(|entry| entry.fq_name.clone()).collect();
    let kind_str = match kind {
        SymbolKind::Schema => "schema",
        SymbolKind::Func => "func",
        SymbolKind::Mod => "mod",
    };
    diags.push(Diagnostic {
        severity: "warning".into(),
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
    let ctx_map: HashMap<_, _> = import_contexts
        .iter()
        .map(|ctx| (ctx.file_path.clone(), ctx))
        .collect();
    let mut diags = Vec::new();

    for (path, file) in files {
        let Some(ctx) = ctx_map.get(path) else {
            continue;
        };
        for section in &file.sections {
            match section {
                Section::Func(func) => {
                    for schema in &func.input {
                        resolve_reference(
                            symbols,
                            SymbolKind::Schema,
                            schema,
                            path,
                            ctx,
                            &format!("func.{}.input({})", func.name, schema),
                            &mut diags,
                        );
                    }
                    for schema in &func.output {
                        resolve_reference(
                            symbols,
                            SymbolKind::Schema,
                            schema,
                            path,
                            ctx,
                            &format!("func.{}.output({})", func.name, schema),
                            &mut diags,
                        );
                    }
                }
                Section::Schema(schema) => match schema.kind.as_str() {
                    "edge" => {
                        if !schema.from.is_empty() {
                            resolve_reference(
                                symbols,
                                SymbolKind::Schema,
                                &schema.from,
                                path,
                                ctx,
                                &format!("schema.{}.from({})", schema.name, schema.from),
                                &mut diags,
                            );
                        }
                        if !schema.to.is_empty() {
                            resolve_reference(
                                symbols,
                                SymbolKind::Schema,
                                &schema.to,
                                path,
                                ctx,
                                &format!("schema.{}.to({})", schema.name, schema.to),
                                &mut diags,
                            );
                        }
                    }
                    "boundary" => {
                        for over in &schema.over {
                            resolve_reference(
                                symbols,
                                SymbolKind::Schema,
                                over,
                                path,
                                ctx,
                                &format!("schema.{}.over({})", schema.name, over),
                                &mut diags,
                            );
                        }
                    }
                    _ => {}
                },
                Section::Mod(module) => {
                    for s in &module.schemas {
                        resolve_reference(
                            symbols,
                            SymbolKind::Schema,
                            s,
                            path,
                            ctx,
                            &format!("mod.{}.schemas({})", module.name, s),
                            &mut diags,
                        );
                    }
                    for func in &module.funcs {
                        resolve_reference(
                            symbols,
                            SymbolKind::Func,
                            func,
                            path,
                            ctx,
                            &format!("mod.{}.funcs({})", module.name, func),
                            &mut diags,
                        );
                    }
                    for step in &module.pipeline {
                        resolve_reference(
                            symbols,
                            SymbolKind::Func,
                            step,
                            path,
                            ctx,
                            &format!("mod.{}.pipeline({})", module.name, step),
                            &mut diags,
                        );
                    }
                }
                Section::Meta(_) => {}
                Section::Status(_) => {}
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
    diags: &mut Vec<Diagnostic>,
) {
    if reference.is_empty() {
        return;
    }

    let reference = reference.trim();
    let (prefix, base) = split_reference(reference);

    match prefix {
        Some(prefix) if prefix != "schema" && prefix != "func" => {
            let Some(package) = resolve_prefix(prefix, ctx) else {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "E_UNDEFINED_PREFIX".into(),
                    message: format!("Unknown reference prefix '{}'", prefix),
                    location: format!("{}: {}", path.display(), context),
                });
                return;
            };
            resolve_package_reference(
                symbols,
                kind,
                &package,
                extract_local_name(base),
                reference,
                path,
                context,
                diags,
            );
        }
        _ => {
            let local = if prefix.is_some() {
                extract_local_name(base)
            } else {
                extract_local_name(reference)
            };

            if resolve_self_package(symbols, kind, ctx, local, reference, path, context, diags) {
                return;
            }
            if resolve_import_packages(symbols, kind, ctx, local, reference, path, context, diags) {
                return;
            }
            resolve_global_reference(symbols, kind, local, reference, path, context, diags);
        }
    }
}

fn resolve_self_package(
    symbols: &SymbolTable,
    kind: SymbolKind,
    ctx: &FileImportContext,
    local_name: &str,
    reference: &str,
    path: &Path,
    context: &str,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let matches = lookup_in_package(
        symbols,
        kind,
        &ctx.self_package,
        ctx.namespace.as_deref(),
        local_name,
    );
    match matches.len() {
        0 => false,
        1 => true,
        _ => {
            emit_ambiguous(kind, reference, matches, path, context, diags);
            true
        }
    }
}

fn resolve_import_packages(
    symbols: &SymbolTable,
    kind: SymbolKind,
    ctx: &FileImportContext,
    local_name: &str,
    reference: &str,
    path: &Path,
    context: &str,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let mut matches = Vec::new();
    for import in &ctx.imports {
        let found = lookup_in_package(symbols, kind, &import.package, None, local_name);
        if found.len() == 1 {
            matches.push(found[0]);
        } else if found.len() > 1 {
            emit_ambiguous(kind, reference, found, path, context, diags);
            return true;
        }
    }
    match matches.len() {
        0 => false,
        1 => true,
        _ => {
            emit_ambiguous(kind, reference, matches, path, context, diags);
            true
        }
    }
}

fn resolve_package_reference(
    symbols: &SymbolTable,
    kind: SymbolKind,
    package: &str,
    local_name: &str,
    reference: &str,
    path: &Path,
    context: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let matches = lookup_in_package(symbols, kind, package, None, local_name);
    match matches.len() {
        0 => emit_undefined(kind, reference, path, context, diags),
        1 => {}
        _ => emit_ambiguous(kind, reference, matches, path, context, diags),
    }
}

fn resolve_global_reference(
    symbols: &SymbolTable,
    kind: SymbolKind,
    local_name: &str,
    reference: &str,
    path: &Path,
    context: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let matches = lookup_any_namespace(symbols, kind, local_name);
    match matches.len() {
        0 => emit_undefined(kind, reference, path, context, diags),
        1 => {}
        _ => emit_ambiguous(kind, reference, matches, path, context, diags),
    }
}

fn resolve_prefix(prefix: &str, ctx: &FileImportContext) -> Option<String> {
    if prefix == ctx.self_package {
        return Some(prefix.to_string());
    }
    if ctx.namespace.as_deref() == Some(prefix) {
        return Some(ctx.self_package.clone());
    }
    for import in &ctx.imports {
        if import.alias.as_deref() == Some(prefix) || prefix == import.package {
            return Some(import.package.clone());
        }
    }
    None
}

fn split_reference(reference: &str) -> (Option<&str>, &str) {
    if let Some(idx) = reference.find('.') {
        let (head, tail) = reference.split_at(idx);
        (Some(head), &tail[1..])
    } else {
        (None, reference)
    }
}

fn lookup_in_package<'a>(
    symbols: &'a SymbolTable,
    kind: SymbolKind,
    package: &str,
    namespace: Option<&str>,
    local_name: &str,
) -> Vec<&'a SymbolEntry> {
    let matches: Vec<_> = symbols
        .entries
        .iter()
        .filter(|entry| {
            entry.kind == kind && entry.package == package && entry.local_name == local_name
        })
        .collect();
    if let Some(ns) = namespace {
        let exact: Vec<_> = matches
            .iter()
            .copied()
            .filter(|entry| entry.namespace.as_deref() == Some(ns))
            .collect();
        if !exact.is_empty() {
            return exact;
        }
    }
    matches
}

fn lookup_any_namespace<'a>(
    symbols: &'a SymbolTable,
    kind: SymbolKind,
    local_name: &str,
) -> Vec<&'a SymbolEntry> {
    symbols
        .entries
        .iter()
        .filter(|entry| entry.kind == kind && entry.local_name == local_name)
        .collect()
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
