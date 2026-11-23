use super::lsp_client::LspClient;
use super::mapping::{find_by_surv_ref, load_mapping};
use super::static_analysis::StaticAnalyzer;
use super::types::{
    DiffResult, ExpectedSymbol, FoundSymbol, FunctionSignature, MappingFile, SchemaFieldMismatch,
    SignatureMismatch, SymbolKind, TypeVerification,
};
use crate::ast::{ModSection, Section};
use crate::loader::load_manifest;
use crate::project::ProjectAST;
use crate::project_checker::{load_ir_context, load_project_context, ProjectContext};
use crate::symbol::{reference_candidates, SymbolEntry, SymbolKind as ResolveKind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type TypeMap = BTreeMap<String, Vec<String>>;

/// Main entry point for diff-impl
pub fn diff_impl(
    ir_file: &Path,
    workspace_root: &Path,
    filter_mod: Option<&str>,
    language: &str,
    strategy: &str, // "lsp" or "tree-sitter"
    mapping_path: Option<&Path>,
) -> Result<DiffResult, Box<dyn Error>> {
    // 1. Parse IR file and extract expected symbols
    let (expected, type_map) = extract_expected_symbols(ir_file, filter_mod)?;
    let (result, _) = diff_expected_symbols(
        &expected,
        workspace_root,
        language,
        strategy,
        mapping_path,
        &type_map,
        true,
    )?;
    Ok(result)
}

/// Run diff analysis on a pre-built list of expected symbols, also returning matched FoundSymbols keyed by stable_id (or surv_name).
pub(crate) fn diff_expected_symbols(
    expected: &[ExpectedSymbol],
    workspace_root: &Path,
    language: &str,
    strategy: &str,
    mapping_path: Option<&Path>,
    type_map: &TypeMap,
    strict: bool,
) -> Result<(DiffResult, HashMap<String, FoundSymbol>), Box<dyn Error>> {
    if strict && !workspace_root.is_dir() {
        return Err(format!("Workspace is not a directory: {}", workspace_root.display()).into());
    }
    let mapping = if let Some(path) = mapping_path {
        if strict && !path.is_file() {
            return Err(format!("Mapping file is not a file: {}", path.display()).into());
        }
        Some(load_mapping(path)?)
    } else {
        None
    };

    let found = if strategy == "lsp" {
        let mut symbols =
            query_workspace_symbols_lsp(workspace_root, language, Some(expected), strict)?;
        if strict && (!symbols.is_empty() || language == "all") {
            // workspace/symbol only supplies identities. Read local declarations
            // for the field and signature checks used by the plan overlay.
            let declarations = query_workspace_symbols_static(workspace_root, language)?;
            enrich_lsp_declarations(&mut symbols, &declarations);
            if language == "all" {
                symbols.extend(
                    declarations
                        .into_iter()
                        .filter(|symbol| matches!(symbol.language.as_deref(), Some("c" | "zig"))),
                );
            }
        }
        symbols
    } else {
        query_workspace_symbols_static(workspace_root, language)?
    };

    Ok(match_symbols_with_found(
        expected,
        &found,
        language,
        mapping.as_ref(),
        type_map,
        strict,
    ))
}

fn enrich_lsp_declarations(symbols: &mut [FoundSymbol], declarations: &[FoundSymbol]) {
    let mut by_file_and_name = HashMap::<_, Vec<_>>::new();
    for declaration in declarations {
        if let Some(path) = declaration.relative_path.as_deref() {
            by_file_and_name
                .entry((path, declaration.name.as_str()))
                .or_default()
                .push(declaration);
        }
    }
    for symbol in symbols {
        let Some(path) = symbol.relative_path.as_deref() else {
            continue;
        };
        let Some(candidates) = by_file_and_name.get(&(path, symbol.name.as_str())) else {
            continue;
        };
        let nearby: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|candidate| {
                candidate.range.start_line <= symbol.range.start_line
                    && symbol.range.start_line <= candidate.range.end_line
            })
            .collect();
        let declaration = match (nearby.as_slice(), candidates.as_slice()) {
            ([declaration], _) | ([], [declaration]) => *declaration,
            _ => continue,
        };
        symbol.signature = declaration.signature.clone();
        symbol.fields.clone_from(&declaration.fields);
        symbol.fields_known = declaration.fields_known;
        symbol.impl_path.clone_from(&declaration.impl_path);
        symbol.module_path.clone_from(&declaration.module_path);
    }
}

/// Scan workspace symbols directly for Code -> IR skeleton generation without requiring an existing IR file.
pub fn scan_workspace_for_skeleton(
    workspace_root: &Path,
    language: &str,
    strategy: &str,
) -> Result<DiffResult, Box<dyn Error>> {
    let found = if strategy == "lsp" {
        query_workspace_symbols_lsp(workspace_root, language, None, false)?
    } else {
        query_workspace_symbols_static(workspace_root, language)?
    };

    let extra = found
        .into_iter()
        .filter(is_relevant_symbol)
        .collect::<Vec<_>>();

    Ok(DiffResult {
        extra,
        ..Default::default()
    })
}

/// Extract expected symbols from an IR file/manifest for IR -> Code stub generation without scanning a workspace.
pub fn extract_ir_for_code_skeleton(
    ir_file: &Path,
    filter_mod: Option<&str>,
    language: &str,
) -> Result<DiffResult, Box<dyn Error>> {
    let (expected, _) = extract_expected_symbols(ir_file, filter_mod)?;
    let missing = expected
        .into_iter()
        .filter(|exp| matches!(language, "both" | "all") || exp.matches_language(language))
        .collect::<Vec<_>>();

    Ok(DiffResult {
        missing,
        ..Default::default()
    })
}

/// Query symbols using static analysis (Tree-sitter)
fn query_workspace_symbols_static(
    workspace_root: &Path,
    language: &str,
) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let mut analyzer = StaticAnalyzer::new()?;
    analyzer.scan_workspace(workspace_root, language)
}

/// Extract expected symbols from IR file
fn extract_expected_symbols(
    ir_file: &Path,
    filter_mod: Option<&str>,
) -> Result<(Vec<ExpectedSymbol>, TypeMap), Box<dyn Error>> {
    let context = if should_load_as_manifest(ir_file)? {
        load_project_context(ir_file)?
    } else {
        load_ir_context(ir_file)?
    };
    if let Some(diagnostic) = context
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == "error")
    {
        return Err(format!(
            "{}: [{}] {}",
            diagnostic.location, diagnostic.kind, diagnostic.message
        )
        .into());
    }
    let type_map = context
        .manifest
        .as_ref()
        .map(|manifest| manifest.diff.type_map.clone())
        .unwrap_or_default();
    let sections = section_map(&context.project);
    let filter = filter_mod
        .map(|module| compute_filter_closure(&context, &sections, module))
        .transpose()?;
    let module_refs = collect_module_refs_context(&context);

    let mut expected = Vec::new();
    for entry in &context.symbols.entries {
        if let Some(filter) = &filter {
            if !filter.contains(&entry.fq_name) {
                continue;
            }
        }
        let Some(section) =
            sections.get(&(entry.file.clone(), entry.kind, entry.local_name.clone()))
        else {
            continue;
        };
        match (entry.kind, section) {
            (ResolveKind::Schema, Section::Schema(schema)) => {
                expected.push(ExpectedSymbol {
                    stable_id: Some(entry.fq_name.clone()),
                    surv_name: schema.name.clone(),
                    surv_path: Some(format!("schema.{}", schema.name)),
                    impl_bind: schema.impl_bind.clone(),
                    impl_lang: schema.impl_lang.clone(),
                    impl_path: schema.impl_path.clone(),
                    module_refs: module_refs.get(&entry.fq_name).cloned().unwrap_or_default(),
                    kind: SymbolKind::Schema,
                    input: Vec::new(),
                    output: Vec::new(),
                    fields: schema.fields.clone(),
                });
            }
            (ResolveKind::Func, Section::Func(func)) => {
                expected.push(ExpectedSymbol {
                    stable_id: Some(entry.fq_name.clone()),
                    surv_name: func.name.clone(),
                    surv_path: Some(format!("func.{}", func.name)),
                    impl_bind: func.impl_bind.clone(),
                    impl_lang: func.impl_lang.clone(),
                    impl_path: func.impl_path.clone(),
                    module_refs: module_refs.get(&entry.fq_name).cloned().unwrap_or_default(),
                    kind: SymbolKind::Func,
                    input: func.input.clone(),
                    output: func.output.clone(),
                    fields: Default::default(),
                });
            }
            _ => {}
        }
    }
    Ok((expected, type_map))
}

type SectionMap<'a> = HashMap<(PathBuf, ResolveKind, String), &'a Section>;

fn section_map(project: &ProjectAST) -> SectionMap<'_> {
    let mut map = HashMap::new();
    for (path, file) in &project.files {
        for section in &file.sections {
            let Some((kind, name)) = section_kind_name(section) else {
                continue;
            };
            map.insert((path.clone(), kind, name.to_string()), section);
        }
    }
    map
}

fn section_kind_name(section: &Section) -> Option<(ResolveKind, &str)> {
    match section {
        Section::Schema(schema) => Some((ResolveKind::Schema, &schema.name)),
        Section::Func(func) => Some((ResolveKind::Func, &func.name)),
        Section::Mod(module) => Some((ResolveKind::Mod, &module.name)),
        _ => None,
    }
}

fn module_section<'a>(
    context: &'a ProjectContext,
    sections: &SectionMap<'a>,
    module: &str,
) -> Result<(&'a ModSection, &'a crate::imports::FileImportContext), Box<dyn Error>> {
    let raw = module.strip_prefix("mod.").unwrap_or(module);
    let matches: Vec<&SymbolEntry> = context
        .symbols
        .entries
        .iter()
        .filter(|entry| entry.kind == ResolveKind::Mod)
        .filter(|entry| {
            entry.local_name == raw
                || entry.fq_name == module
                || format!("mod.{}", entry.local_name) == module
                || format!("{}.mod.{}", entry.package, entry.local_name) == module
                || entry.namespace.as_ref().is_some_and(|namespace| {
                    format!("{namespace}.mod.{}", entry.local_name) == module
                })
        })
        .collect();
    let entry = match matches.as_slice() {
        [entry] => *entry,
        [] => return Err(format!("Module '{module}' not found").into()),
        _ => {
            let names = matches
                .iter()
                .map(|entry| entry.fq_name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!("Ambiguous module '{module}'; candidates: {names}").into());
        }
    };
    let section = sections
        .get(&(
            entry.file.clone(),
            ResolveKind::Mod,
            entry.local_name.clone(),
        ))
        .and_then(|section| match section {
            Section::Mod(module) => Some(module),
            _ => None,
        })
        .ok_or_else(|| format!("Module '{module}' has no section"))?;
    let context = context
        .contexts
        .iter()
        .find(|ctx| ctx.file_path == entry.file)
        .ok_or_else(|| format!("Module '{module}' has no import context"))?;
    Ok((section, context))
}

fn compute_filter_closure(
    context: &ProjectContext,
    sections: &SectionMap<'_>,
    module_name: &str,
) -> Result<HashSet<String>, Box<dyn Error>> {
    let (module, import_context) = module_section(context, sections, module_name)?;
    let mut closure = HashSet::new();
    closure.insert(
        context
            .symbols
            .entries
            .iter()
            .find(|entry| {
                entry.kind == ResolveKind::Mod
                    && entry.local_name == module.name
                    && entry.file
                        == context
                            .contexts
                            .iter()
                            .find(|ctx| std::ptr::eq(*ctx, import_context))
                            .map(|ctx| ctx.file_path.clone())
                            .unwrap_or_default()
            })
            .map(|entry| entry.fq_name.clone())
            .unwrap_or_else(|| format!("mod.{}", module.name)),
    );
    for reference in &module.schemas {
        add_candidates(
            &mut closure,
            &context.symbols,
            ResolveKind::Schema,
            reference,
            import_context,
        );
    }
    for reference in &module.funcs {
        add_candidates(
            &mut closure,
            &context.symbols,
            ResolveKind::Func,
            reference,
            import_context,
        );
    }
    for reference in &module.submods {
        add_candidates(
            &mut closure,
            &context.symbols,
            ResolveKind::Mod,
            &format!("mod.{}.{reference}", module.name),
            import_context,
        );
    }
    for reference in module.get_pipeline_calls() {
        if let Some(func) = resolve_one(
            &context.symbols,
            ResolveKind::Func,
            &reference,
            import_context,
        ) {
            add_candidates(
                &mut closure,
                &context.symbols,
                ResolveKind::Func,
                &reference,
                import_context,
            );
            let func_name = func.local_name.as_str();
            if let Some((_, file)) = context
                .project
                .files
                .iter()
                .find(|(path, _)| path == &func.file)
            {
                for section in &file.sections {
                    if let Section::Func(section) = section {
                        if section.name == func_name {
                            for schema in section.input.iter().chain(&section.output) {
                                add_candidates(
                                    &mut closure,
                                    &context.symbols,
                                    ResolveKind::Schema,
                                    schema,
                                    import_context,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(closure)
}

fn add_candidates(
    closure: &mut HashSet<String>,
    symbols: &crate::symbol::SymbolTable,
    kind: ResolveKind,
    reference: &str,
    context: &crate::imports::FileImportContext,
) {
    for candidate in reference_candidates(symbols, kind, reference, context) {
        closure.insert(candidate.fq_name.clone());
    }
}

fn resolve_one<'a>(
    symbols: &'a crate::symbol::SymbolTable,
    kind: ResolveKind,
    reference: &str,
    context: &crate::imports::FileImportContext,
) -> Option<&'a SymbolEntry> {
    match reference_candidates(symbols, kind, reference, context).as_slice() {
        [entry] => Some(*entry),
        _ => None,
    }
}

fn collect_module_refs_context(context: &ProjectContext) -> HashMap<String, Vec<String>> {
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    for (path, file) in &context.project.files {
        let Some(import_context) = context.contexts.iter().find(|ctx| &ctx.file_path == path)
        else {
            continue;
        };
        for section in &file.sections {
            let Section::Mod(module) = section else {
                continue;
            };
            let module_ref = format!("mod.{}", module.name);
            for reference in &module.schemas {
                for candidate in reference_candidates(
                    &context.symbols,
                    ResolveKind::Schema,
                    reference,
                    import_context,
                ) {
                    refs.entry(candidate.fq_name.clone())
                        .or_default()
                        .push(module_ref.clone());
                }
            }
            for reference in &module.funcs {
                for candidate in reference_candidates(
                    &context.symbols,
                    ResolveKind::Func,
                    reference,
                    import_context,
                ) {
                    refs.entry(candidate.fq_name.clone())
                        .or_default()
                        .push(module_ref.clone());
                }
            }
        }
    }
    for modules in refs.values_mut() {
        modules.sort();
        modules.dedup();
    }
    refs
}

fn is_manifest_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == "surv.toml")
        .unwrap_or(false)
}

fn should_load_as_manifest(ir_file: &Path) -> Result<bool, Box<dyn Error>> {
    if load_manifest(ir_file).is_ok() {
        return Ok(true);
    }
    if is_manifest_path(ir_file) {
        let raw = fs::read_to_string(ir_file)?;
        let table: toml::Table = toml::from_str(&raw)
            .map_err(|err| format!("{}: invalid TOML: {err}", ir_file.display()))?;
        let has_manifest_section = table.contains_key("project")
            || table.contains_key("paths")
            || table.contains_key("packages");
        if has_manifest_section {
            // Trigger manifest validation error so callers see why surv.toml failed
            load_manifest(ir_file)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Query LSP servers for workspace symbols
fn query_workspace_symbols_lsp(
    workspace_root: &Path,
    language: &str,
    expected: Option<&[ExpectedSymbol]>,
    strict: bool,
) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let mut all_symbols = Vec::new();
    let mut queried = false;
    let mut last_error = None;

    let languages = if matches!(language, "both" | "all") {
        vec!["ts", "rust"]
    } else {
        vec![language]
    };

    for lang in languages {
        // Check if any expected symbols support this language
        if matches!(language, "both" | "all")
            && expected.is_some_and(|symbols| !symbols.iter().any(|exp| exp.matches_language(lang)))
        {
            continue;
        }

        match query_language_symbols(workspace_root, lang) {
            Ok(mut symbols) => {
                queried = true;
                all_symbols.append(&mut symbols);
            }
            Err(e) if strict => return Err(format!("Failed to query {lang} symbols: {e}").into()),
            Err(e) => {
                eprintln!("Warning: Failed to query {} symbols: {}", lang, e);
                last_error = Some(e);
            }
        }
    }

    if !queried {
        if let Some(error) = last_error {
            return Err(error);
        }
    }
    Ok(all_symbols)
}

fn query_language_symbols(
    workspace_root: &Path,
    lang: &str,
) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let workspace_root = workspace_root.to_path_buf();
    let lang = lang.to_string();
    let label = lang.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result =
            query_language_symbols_inner(&workspace_root, &lang).map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    match receiver.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(Ok(symbols)) => Ok(symbols),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => Err(format!("LSP query for {label} timed out after 60s").into()),
    }
}

fn query_language_symbols_inner(
    workspace_root: &Path,
    lang: &str,
) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let mut client = LspClient::new(lang, workspace_root)?;

    // Query for all symbols (empty query returns all). Some servers return an
    // empty result while indexing; retry briefly before treating it as empty.
    let mut symbols = client.workspace_symbol("")?;
    for _ in 0..3 {
        if !symbols.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        symbols = client.workspace_symbol("")?;
    }
    let root = workspace_root.canonicalize().ok();
    for symbol in &mut symbols {
        symbol.language = Some(lang.to_string());
        symbol.relative_path = lsp_types::Url::parse(&symbol.uri)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .and_then(|path| {
                path.strip_prefix(root.as_ref()?)
                    .ok()
                    .map(|relative| relative.to_string_lossy().into_owned())
            });
    }

    client.shutdown()?;

    Ok(symbols)
}

/// Match expected symbols against found symbols
#[cfg(test)]
fn match_symbols(
    expected: &[ExpectedSymbol],
    found: &[FoundSymbol],
    language: &str,
    mapping: Option<&MappingFile>,
) -> DiffResult {
    match_symbols_with_found(expected, found, language, mapping, &BTreeMap::new(), false).0
}

fn match_symbols_with_found(
    expected: &[ExpectedSymbol],
    found: &[FoundSymbol],
    language: &str,
    mapping: Option<&MappingFile>,
    type_map: &TypeMap,
    scoped: bool,
) -> (DiffResult, HashMap<String, FoundSymbol>) {
    let mut result = DiffResult::default();
    let type_context = TypeContext::from_found(found, type_map);

    // Create a map of found symbols by name for quick lookup
    let mut found_map: HashMap<String, Vec<&FoundSymbol>> = HashMap::new();
    for symbol in found {
        found_map
            .entry(symbol.name.clone())
            .or_default()
            .push(symbol);
    }

    let mut matched_found = HashSet::new();
    let mut matches = HashMap::new();
    let mut candidates_by_expected = Vec::new();
    let mut claims = HashMap::new();
    let mut packages_by_binding = HashMap::<_, BTreeSet<_>>::new();
    if scoped {
        for symbol in expected {
            if let Some(package) = symbol.stable_id.as_deref().and_then(package_from_id) {
                packages_by_binding
                    .entry((&symbol.kind, symbol.search_name()))
                    .or_default()
                    .insert(package);
            }
        }
    }

    for exp in expected {
        if !matches!(language, "both" | "all") && !exp.matches_language(language) {
            continue;
        }
        let mut candidates = find_candidates(&found_map, found, exp, exp.search_name(), mapping);
        if scoped {
            candidates.retain(|symbol| {
                symbol
                    .language
                    .as_deref()
                    .is_none_or(|lang| exp.matches_language(lang))
            });
            if mapping_entry(mapping, exp).is_none() && exp.impl_path.is_none() {
                let package = exp.stable_id.as_deref().and_then(package_from_id);
                let packages = packages_by_binding.get(&(&exp.kind, exp.search_name()));
                if let (Some(package), Some(packages)) = (package, packages.filter(|p| p.len() > 1))
                {
                    if package != "default"
                        && candidates
                            .iter()
                            .any(|symbol| symbol_matches_expected_path(symbol, package))
                    {
                        candidates.retain(|symbol| symbol_matches_expected_path(symbol, package));
                    } else {
                        candidates.retain(|symbol| {
                            !packages.iter().any(|other| {
                                *other != package
                                    && *other != "default"
                                    && symbol_matches_expected_path(symbol, other)
                            })
                        });
                    }
                }
            }
            if let [only] = candidates.as_slice() {
                *claims.entry(found_symbol_key(only)).or_insert(0) += 1;
            }
        }
        candidates_by_expected.push((exp, candidates));
    }

    for (exp, candidates) in candidates_by_expected {
        if scoped && matches!(candidates.as_slice(), [only] if claims[&found_symbol_key(only)] > 1)
        {
            result
                .ambiguous
                .push((exp.clone(), vec![candidates[0].clone()]));
            matched_found.insert(found_symbol_key(candidates[0]));
            continue;
        }
        match candidates.len() {
            0 => {
                // Missing
                result.missing.push(exp.clone());
            }
            1 => {
                // Matched
                result.matched += 1;
                let key = exp.stable_id.as_ref().unwrap_or(&exp.surv_name).clone();
                matches.insert(key, candidates[0].clone());
                let (signature_mismatch, signature_unverified) =
                    compare_signature(exp, candidates[0], &type_context);
                let (field_mismatch, field_unverified) =
                    compare_schema_fields(exp, candidates[0], &type_context);
                if let Some(mismatch) = signature_mismatch {
                    result.signature_mismatches.push(mismatch);
                } else if !signature_unverified.is_empty() {
                    result.unverified.push(TypeVerification {
                        expected: exp.clone(),
                        found: candidates[0].clone(),
                        problems: signature_unverified,
                    });
                }
                if let Some(mismatch) = field_mismatch {
                    result.schema_field_mismatches.push(mismatch);
                } else if !field_unverified.is_empty() {
                    result.unverified.push(TypeVerification {
                        expected: exp.clone(),
                        found: candidates[0].clone(),
                        problems: field_unverified,
                    });
                }
                matched_found.insert(found_symbol_key(candidates[0]));
            }
            _ => {
                // Ambiguous
                result.ambiguous.push((
                    exp.clone(),
                    candidates.iter().map(|&s| (*s).clone()).collect(),
                ));
                for candidate in &candidates {
                    matched_found.insert(found_symbol_key(candidate));
                }
            }
        }
    }

    // Find extra symbols (in code but not in IR)
    // Note: This can be very noisy, so we'll be conservative
    // Only report symbols that look like they could be expected
    for symbol in found {
        if !matched_found.contains(&found_symbol_key(symbol)) {
            // Filter noise: only include types that match our expectations
            if is_relevant_symbol(symbol) {
                result.extra.push(symbol.clone());
            }
        }
    }

    (result, matches)
}

fn found_symbol_key(symbol: &FoundSymbol) -> (&str, &str, u32, u32, &str) {
    (
        symbol.uri.as_str(),
        symbol.kind.as_str(),
        symbol.range.start_line,
        symbol.range.start_char,
        symbol.name.as_str(),
    )
}

fn package_from_id(id: &str) -> Option<&str> {
    let id = id.strip_prefix("pkg.")?;
    [".schema.", ".func."]
        .into_iter()
        .filter_map(|marker| id.find(marker))
        .min()
        .map(|index| &id[..index])
}

fn mapping_entry<'a>(
    mapping: Option<&'a MappingFile>,
    exp: &ExpectedSymbol,
) -> Option<&'a super::types::MappingEntry> {
    let mapping = mapping?;
    exp.stable_id
        .as_deref()
        .and_then(|id| {
            mapping
                .entries
                .iter()
                .find(|entry| entry.stable_id == id || entry.surv_ref == id)
        })
        .or_else(|| {
            exp.surv_path
                .as_deref()
                .and_then(|path| find_by_surv_ref(mapping, path))
        })
}

fn find_candidates<'a>(
    found_map: &'a HashMap<String, Vec<&'a FoundSymbol>>,
    found: &'a [FoundSymbol],
    exp: &ExpectedSymbol,
    search_name: &str,
    mapping: Option<&MappingFile>,
) -> Vec<&'a FoundSymbol> {
    let mut candidates = Vec::new();

    if let Some(entry) = mapping_entry(mapping, exp) {
        for symbol in found {
            let impl_path_matches = symbol
                .impl_path
                .as_deref()
                .map(|path| path == entry.impl_path)
                .unwrap_or(false);
            let file_and_name_match = symbol.name == entry.symbol_name
                && symbol
                    .relative_path
                    .as_deref()
                    .map(|file| file == entry.source_file)
                    .unwrap_or(false);
            if symbol_kind_matches(symbol, exp) && (impl_path_matches || file_and_name_match) {
                candidates.push(symbol);
            }
        }
        return candidates;
    }

    if let Some(symbols) = found_map.get(search_name) {
        for symbol in symbols {
            // Check if symbol kind matches expectation
            if symbol_kind_matches(symbol, exp) {
                // Check impl.path if specified
                if let Some(expected_path) = &exp.impl_path {
                    if symbol_matches_expected_path(symbol, expected_path) {
                        candidates.push(*symbol);
                    }
                } else {
                    candidates.push(*symbol);
                }
            }
        }
    }

    candidates
}

fn split_path_segments(path: &str) -> Vec<&str> {
    path.split([':', '.', '/', '\\'])
        .filter(|seg| !seg.is_empty())
        .collect()
}

fn segments_contain(haystack: &[&str], needle: &[&str]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn symbol_matches_expected_path(symbol: &FoundSymbol, expected_path: &str) -> bool {
    let expected_segments = split_path_segments(expected_path);
    if expected_segments.is_empty() {
        return true;
    }

    let matches_field = |field: Option<&str>| -> bool {
        let Some(value) = field else {
            return false;
        };
        let actual_segments = split_path_segments(value);
        segments_contain(&actual_segments, &expected_segments)
    };

    if matches_field(symbol.impl_path.as_deref())
        || matches_field(symbol.module_path.as_deref())
        || matches_field(symbol.relative_path.as_deref())
    {
        return true;
    }

    if let Some(container) = symbol.container_name.as_deref() {
        let container_segments = split_path_segments(container);
        if segments_contain(&container_segments, &expected_segments) {
            return true;
        }
        // When only a short container name is available (e.g. from LSP without module_path),
        // allow matching if the container matches the trailing segment(s) of expected_path.
        if symbol.impl_path.is_none()
            && symbol.module_path.is_none()
            && !container_segments.is_empty()
            && expected_segments.ends_with(&container_segments)
        {
            return true;
        }
    }

    false
}

fn symbol_kind_matches(symbol: &FoundSymbol, expected: &ExpectedSymbol) -> bool {
    match expected.kind {
        SymbolKind::Func => matches!(
            symbol.kind.as_str(),
            "Function" | "Method" | "Variable" // Variable for TS const functions
        ),
        SymbolKind::Schema => matches!(
            symbol.kind.as_str(),
            "Interface" | "Class" | "Struct" | "Enum" | "Type"
        ),
    }
}

fn is_relevant_symbol(symbol: &FoundSymbol) -> bool {
    // Only include symbols that are likely to be user-defined.
    // Enums and type aliases are implementation details unless the IR
    // explicitly refers to them; they must not become required IR schemas.
    matches!(
        symbol.kind.as_str(),
        "Function" | "Method" | "Interface" | "Class" | "Struct"
    )
}

fn compare_signature(
    exp: &ExpectedSymbol,
    found: &FoundSymbol,
    type_context: &TypeContext,
) -> (Option<SignatureMismatch>, Vec<String>) {
    if exp.kind != SymbolKind::Func {
        return (None, Vec::new());
    }
    let Some(signature) = found.signature.as_ref() else {
        return (None, Vec::new());
    };

    let mut problems = Vec::new();
    let mut unverified = Vec::new();
    let owner = found
        .container_name
        .as_deref()
        .or(Some(found.name.as_str()));
    let language = found.language.as_deref();

    compare_inputs(
        exp,
        signature,
        owner,
        language,
        type_context,
        &mut problems,
        &mut unverified,
    );
    compare_outputs(
        exp,
        signature,
        owner,
        language,
        type_context,
        &mut problems,
        &mut unverified,
    );

    let mismatch = if problems.is_empty() {
        None
    } else {
        Some(SignatureMismatch {
            expected: exp.clone(),
            found: found.clone(),
            problems,
        })
    };
    (mismatch, unverified)
}

fn compare_schema_fields(
    exp: &ExpectedSymbol,
    found: &FoundSymbol,
    type_context: &TypeContext,
) -> (Option<SchemaFieldMismatch>, Vec<String>) {
    if exp.kind != SymbolKind::Schema
        || exp.fields.is_empty()
        || (!found.fields_known && found.fields.is_empty())
    {
        return (None, Vec::new());
    }

    let mut problems = Vec::new();
    let mut unverified = Vec::new();
    let language = found.language.as_deref();
    for (name, expected_type) in &exp.fields {
        match found.fields.get(name) {
            None => problems.push(format!("missing implementation field '{}'", name)),
            Some(actual_type) => {
                let expected = parse_ir_type(expected_type, language);
                let actual =
                    type_context.resolve(parse_impl_type(actual_type, None, language), language);
                match compare_type_expr(&expected, &actual, type_context) {
                    TypeComparison::Mismatch => problems.push(format!(
                        "field '{}' type mismatch: IR '{}' vs implementation '{}'",
                        name, expected_type, actual_type
                    )),
                    TypeComparison::Unverified => unverified.push(format!(
                        "field '{}' type could not be verified: IR '{}' vs implementation '{}'",
                        name, expected_type, actual_type
                    )),
                    TypeComparison::Matches => {}
                }
            }
        }
    }

    for name in found.fields.keys() {
        if !exp.fields.contains_key(name) {
            problems.push(format!("extra implementation field '{}'", name));
        }
    }

    let mismatch = if problems.is_empty() {
        None
    } else {
        Some(SchemaFieldMismatch {
            expected: exp.clone(),
            found: found.clone(),
            problems,
        })
    };
    (mismatch, unverified)
}

fn compare_inputs(
    exp: &ExpectedSymbol,
    signature: &FunctionSignature,
    owner: Option<&str>,
    language: Option<&str>,
    type_context: &TypeContext,
    problems: &mut Vec<String>,
    unverified: &mut Vec<String>,
) {
    let expected_inputs = normalized_schema_refs(&exp.input, language);
    if expected_inputs.is_empty() {
        return;
    }

    let actual_param_types: Vec<TypeExpr> = signature
        .parameters
        .iter()
        .filter_map(|param| param.type_name.as_ref())
        .map(|t| type_context.resolve(parse_impl_type(t, owner, language), language))
        .collect();

    if actual_param_types.len() != expected_inputs.len() {
        problems.push(format!(
            "expected {} input parameter(s) from IR, found {} typed parameter(s)",
            expected_inputs.len(),
            actual_param_types.len()
        ));
    }

    for expected in expected_inputs {
        let comparisons: Vec<TypeComparison> = actual_param_types
            .iter()
            .map(|actual| compare_type_expr(&expected, actual, type_context))
            .collect();
        if comparisons.contains(&TypeComparison::Matches) {
            continue;
        }
        if comparisons.contains(&TypeComparison::Unverified) {
            unverified.push(format!(
                "input type compatible with schema.{} could not be verified",
                type_schema_label(&expected)
            ));
        } else {
            problems.push(format!(
                "missing input type compatible with schema.{}",
                type_schema_label(&expected)
            ));
        }
    }
}

fn compare_outputs(
    exp: &ExpectedSymbol,
    signature: &FunctionSignature,
    owner: Option<&str>,
    language: Option<&str>,
    type_context: &TypeContext,
    problems: &mut Vec<String>,
    unverified: &mut Vec<String>,
) {
    let expected_outputs = normalized_schema_refs(&exp.output, language);
    if expected_outputs.is_empty() {
        return;
    }

    let Some(return_type) = &signature.return_type else {
        problems.push(format!(
            "expected output schema(s) {}, but implementation has no explicit return type",
            exp.output.join(", ")
        ));
        return;
    };

    let actual_return =
        type_context.resolve(parse_impl_type(return_type, owner, language), language);
    for expected in expected_outputs {
        match compare_type_expr(&expected, &actual_return, type_context) {
            TypeComparison::Matches => {}
            TypeComparison::Unverified => unverified.push(format!(
                "return type '{}' for schema.{} could not be verified",
                return_type,
                type_schema_label(&expected)
            )),
            TypeComparison::Mismatch => problems.push(format!(
                "return type '{}' does not mention schema.{}",
                return_type,
                type_schema_label(&expected)
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TypeExpr {
    Named(String),
    Qualified { path: String, local: String },
    Optional(Box<TypeExpr>),
    List(Box<TypeExpr>),
    Union(BTreeSet<String>),
    Unknown,
}

#[derive(Clone, Default)]
struct TypeContext {
    aliases: HashMap<String, Option<String>>,
    enums: HashMap<String, (Vec<String>, bool)>,
    any_targets: BTreeSet<String>,
}

impl TypeContext {
    fn from_found(found: &[FoundSymbol], type_map: &TypeMap) -> Self {
        let mut context = TypeContext::default();
        for symbol in found {
            if symbol.is_test {
                continue;
            }
            let name = normalize_identifier(&symbol.name);
            if let Some(target) = &symbol.alias_target {
                match context.aliases.get(&name) {
                    Some(Some(existing)) if existing != target => {
                        context.aliases.insert(name.clone(), None);
                    }
                    Some(_) => {}
                    None => {
                        context.aliases.insert(name.clone(), Some(target.clone()));
                    }
                }
            }
            if symbol.kind == "Enum" {
                let variants = symbol.enum_variants.clone();
                let known = symbol.enum_variants_known;
                match context.enums.get(&name) {
                    Some((existing, existing_known))
                        if existing != &variants || *existing_known != known =>
                    {
                        context.enums.insert(name, (Vec::new(), false));
                    }
                    Some(_) => {}
                    None => {
                        context.enums.insert(name, (variants, known));
                    }
                }
            }
        }
        for (key, targets) in type_map {
            if normalize_identifier(key) == "any" {
                for target in targets {
                    let target = target.trim();
                    let target = target.strip_prefix("dyn ").unwrap_or(target);
                    let target = target.trim_start_matches('&').trim();
                    context.any_targets.insert(normalize_type_path(target));
                }
            }
        }
        context
    }

    fn resolve(&self, expr: TypeExpr, language: Option<&str>) -> TypeExpr {
        self.resolve_with_seen(expr, language, &mut BTreeSet::new())
    }

    fn resolve_with_seen(
        &self,
        expr: TypeExpr,
        language: Option<&str>,
        seen: &mut BTreeSet<String>,
    ) -> TypeExpr {
        match expr {
            TypeExpr::Optional(inner) => {
                TypeExpr::Optional(Box::new(self.resolve_with_seen(*inner, language, seen)))
            }
            TypeExpr::List(inner) => {
                TypeExpr::List(Box::new(self.resolve_with_seen(*inner, language, seen)))
            }
            TypeExpr::Named(name) => {
                if name == "any" {
                    return TypeExpr::Named(name);
                }
                if let Some(alias) = self.aliases.get(&name) {
                    let Some(target) = alias else {
                        return TypeExpr::Unknown;
                    };
                    if !seen.insert(name.clone()) {
                        return TypeExpr::Unknown;
                    }
                    let target = parse_impl_type(target, None, language);
                    return self.resolve_with_seen(target, language, seen);
                }
                TypeExpr::Named(name)
            }
            TypeExpr::Qualified { path, local } => {
                if local == "any" {
                    return TypeExpr::Qualified { path, local };
                }
                if let Some(alias) = self.aliases.get(&local) {
                    let Some(target) = alias else {
                        return TypeExpr::Unknown;
                    };
                    if !seen.insert(local.clone()) {
                        return TypeExpr::Unknown;
                    }
                    let target = parse_impl_type(target, None, language);
                    return self.resolve_with_seen(target, language, seen);
                }
                TypeExpr::Qualified { path, local }
            }
            TypeExpr::Union(variants) => TypeExpr::Union(variants),
            TypeExpr::Unknown => TypeExpr::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeComparison {
    Matches,
    Mismatch,
    Unverified,
}

fn compare_type_expr(
    expected: &TypeExpr,
    actual: &TypeExpr,
    context: &TypeContext,
) -> TypeComparison {
    if let (Some(expected_local), Some(actual_local)) =
        (type_local_name(expected), type_local_name(actual))
    {
        if expected_local == "any" || actual_local == "any" {
            let other = if expected_local == "any" {
                actual
            } else {
                expected
            };
            let other_local = type_local_name(other).unwrap_or("");
            if other_local == "any" {
                return if context.any_targets.contains("any") {
                    TypeComparison::Matches
                } else {
                    TypeComparison::Unverified
                };
            }
            let identity = type_identity(other).unwrap_or(other_local);
            return if context.any_targets.contains(identity) {
                TypeComparison::Matches
            } else {
                TypeComparison::Unverified
            };
        }
    }
    match (expected, actual) {
        (TypeExpr::Unknown, _) | (_, TypeExpr::Unknown) => TypeComparison::Unverified,
        (TypeExpr::Optional(expected), TypeExpr::Optional(actual))
        | (TypeExpr::List(expected), TypeExpr::List(actual)) => {
            compare_type_expr(expected, actual, context)
        }
        (TypeExpr::Union(expected), TypeExpr::Union(actual)) if expected == actual => {
            TypeComparison::Matches
        }
        (TypeExpr::Union(_), TypeExpr::Union(_)) => TypeComparison::Mismatch,
        (TypeExpr::Union(expected), other) | (other, TypeExpr::Union(expected)) => {
            let Some(local) = type_local_name(other) else {
                return TypeComparison::Unverified;
            };
            let Some((variants, known)) = context.enums.get(local) else {
                return TypeComparison::Unverified;
            };
            if !known {
                return TypeComparison::Unverified;
            }
            if expected == &variants.iter().cloned().collect::<BTreeSet<_>>() {
                TypeComparison::Matches
            } else {
                TypeComparison::Mismatch
            }
        }
        (TypeExpr::Named(expected), TypeExpr::Named(actual)) if expected == actual => {
            TypeComparison::Matches
        }
        (TypeExpr::Named(expected), TypeExpr::Named(actual))
            if is_builtin_scalar(expected) != is_builtin_scalar(actual) =>
        {
            TypeComparison::Unverified
        }
        (TypeExpr::Named(expected), TypeExpr::Qualified { local, .. })
        | (TypeExpr::Qualified { local, .. }, TypeExpr::Named(expected)) => {
            compare_named_qualified(expected, local)
        }
        (
            TypeExpr::Qualified {
                path: expected_path,
                local: expected_local,
            },
            TypeExpr::Qualified {
                path: actual_path,
                local: actual_local,
            },
        ) => {
            if expected_path == actual_path || expected_local == actual_local {
                TypeComparison::Matches
            } else if is_builtin_scalar(expected_local) != is_builtin_scalar(actual_local) {
                TypeComparison::Unverified
            } else {
                TypeComparison::Mismatch
            }
        }
        _ => TypeComparison::Mismatch,
    }
}

fn compare_named_qualified(expected: &str, local: &str) -> TypeComparison {
    if expected == local {
        if is_builtin_scalar(expected) {
            // A builtin IR type must not silently match a user-defined
            // `other::i64`-style path.
            TypeComparison::Unverified
        } else {
            TypeComparison::Matches
        }
    } else if is_builtin_scalar(expected) != is_builtin_scalar(local) {
        TypeComparison::Unverified
    } else {
        TypeComparison::Mismatch
    }
}

fn type_local_name(expr: &TypeExpr) -> Option<&str> {
    match expr {
        TypeExpr::Named(name) => Some(name),
        TypeExpr::Qualified { local, .. } => Some(local),
        _ => None,
    }
}

fn type_identity(expr: &TypeExpr) -> Option<&str> {
    match expr {
        TypeExpr::Named(name) => Some(name),
        TypeExpr::Qualified { path, .. } => Some(path),
        _ => None,
    }
}

fn is_builtin_scalar(name: &str) -> bool {
    matches!(
        name,
        "string"
            | "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "isize"
            | "usize"
            | "f16"
            | "f32"
            | "f64"
            | "uuid"
            | "timestamp"
            | "bytes"
            | "unit"
            | "any"
    )
}

fn type_schema_label(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Named(name) => name.clone(),
        TypeExpr::Qualified { local, .. } => local.clone(),
        TypeExpr::Optional(inner) => format!("{}?", type_schema_label(inner)),
        TypeExpr::List(inner) => format!("{}[]", type_schema_label(inner)),
        TypeExpr::Union(variants) => variants.iter().cloned().collect::<Vec<_>>().join("|"),
        TypeExpr::Unknown => "unknown".to_string(),
    }
}

fn normalized_schema_refs(refs: &[String], language: Option<&str>) -> Vec<TypeExpr> {
    refs.iter()
        .map(|reference| parse_ir_type(reference, language))
        .collect()
}

fn parse_ir_type(input: &str, language: Option<&str>) -> TypeExpr {
    let input = input.trim();
    if input.is_empty() {
        return TypeExpr::Unknown;
    }
    if input == "any" {
        return TypeExpr::Named("any".to_string());
    }
    if input.contains('|') {
        let variants: Vec<&str> = input
            .split('|')
            .map(str::trim)
            .filter(|variant| !variant.is_empty())
            .collect();
        if variants.len() > 1 {
            return TypeExpr::Union(
                variants
                    .into_iter()
                    .map(normalize_variant_name)
                    .collect::<BTreeSet<_>>(),
            );
        }
    }
    if input.starts_with('[') && input.ends_with(']') && input.len() > 2 {
        return TypeExpr::List(Box::new(parse_ir_type(
            &input[1..input.len() - 1],
            language,
        )));
    }
    if let Some(inner) = input.strip_suffix('?') {
        return TypeExpr::Optional(Box::new(parse_ir_type(inner, language)));
    }
    if let Some(inner) = input.strip_suffix("[]") {
        return TypeExpr::List(Box::new(parse_ir_type(inner, language)));
    }
    parse_ir_base(input, language)
}

fn parse_ir_base(input: &str, language: Option<&str>) -> TypeExpr {
    let family = language_family(language);
    let trimmed = input.trim();
    let local = trimmed
        .strip_prefix("schema.")
        .or_else(|| trimmed.rsplit_once(".schema.").map(|(_, name)| name))
        .unwrap_or(trimmed);
    match local {
        "string" | "String" | "str" => TypeExpr::Named("string".to_string()),
        "bool" => TypeExpr::Named("bool".to_string()),
        "int" => match family {
            "c" | "zig" => TypeExpr::Named("i32".to_string()),
            "ts" => TypeExpr::Unknown,
            _ => TypeExpr::Named("i64".to_string()),
        },
        "float" => match family {
            "c" | "zig" => TypeExpr::Named("f32".to_string()),
            "ts" => TypeExpr::Unknown,
            _ => TypeExpr::Named("f64".to_string()),
        },
        "uuid" => TypeExpr::Named("uuid".to_string()),
        "timestamp" => TypeExpr::Named("timestamp".to_string()),
        "bytes" => TypeExpr::Named("bytes".to_string()),
        "void" | "()" => TypeExpr::Named("unit".to_string()),
        "any" => TypeExpr::Named("any".to_string()),
        _ => TypeExpr::Named(normalize_identifier(local)),
    }
}

fn language_family(language: Option<&str>) -> &str {
    match language.unwrap_or("rust").to_ascii_lowercase().as_str() {
        "c" | "cpp" | "c++" => "c",
        "zig" => "zig",
        "ts" | "typescript" => "ts",
        _ => "rust",
    }
}

fn parse_impl_type(input: &str, owner: Option<&str>, language: Option<&str>) -> TypeExpr {
    let mut value = input.trim().to_string();
    let family = language_family(language);
    if value.is_empty() {
        return TypeExpr::Unknown;
    }
    if value == "()" {
        return TypeExpr::Named("unit".to_string());
    }
    if matches!(value.as_str(), "any" | "Any") {
        return TypeExpr::Named("any".to_string());
    }

    if family == "zig" {
        if let Some(inner) = value.strip_prefix('?') {
            return TypeExpr::Optional(Box::new(parse_impl_type(inner, owner, language)));
        }
        if let Some(inner) = value.strip_prefix("[]") {
            if is_byte_scalar(inner.trim_start_matches("const ")) {
                return TypeExpr::Named("bytes".to_string());
            }
            return TypeExpr::List(Box::new(parse_impl_type(inner, owner, language)));
        }
    }
    if family == "ts" && value.ends_with("[]") {
        return TypeExpr::List(Box::new(parse_impl_type(
            &value[..value.len() - 2],
            owner,
            language,
        )));
    }
    if family == "ts" && value == "number" {
        return TypeExpr::Unknown;
    }

    while let Some(inner) = strip_outer_parens(&value) {
        value = inner.to_string();
    }
    let value = strip_rust_prefixes(&value);

    if matches!(value, "Self" | "self") {
        return owner
            .map(|owner| TypeExpr::Named(normalize_identifier(owner)))
            .unwrap_or(TypeExpr::Unknown);
    }

    if let Some((outer, inner)) = split_generic(value) {
        let inner = inner.trim();
        let outer = outer.rsplit("::").next().unwrap_or(outer);
        match (family, outer) {
            ("rust", "Option") => {
                return TypeExpr::Optional(Box::new(parse_impl_type(inner, owner, language)));
            }
            ("rust", "Vec") => {
                if is_byte_scalar(inner) {
                    return TypeExpr::Named("bytes".to_string());
                }
                return TypeExpr::List(Box::new(parse_impl_type(inner, owner, language)));
            }
            ("rust", "Result") => {
                return parse_impl_type(first_generic_argument(inner), owner, language);
            }
            ("ts", "Array") => {
                return TypeExpr::List(Box::new(parse_impl_type(inner, owner, language)));
            }
            _ => return TypeExpr::Unknown,
        }
    }

    if let Some(inner) = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        if is_byte_scalar(inner) {
            return TypeExpr::Named("bytes".to_string());
        }
        return TypeExpr::List(Box::new(parse_impl_type(inner.trim(), owner, language)));
    }

    named_or_qualified(&normalize_impl_base(value, family))
}

fn named_or_qualified(value: &str) -> TypeExpr {
    let value = value.trim();
    if let Some((_, local)) = value.rsplit_once("::") {
        TypeExpr::Qualified {
            path: value.to_string(),
            local: normalize_identifier(local),
        }
    } else if let Some((path, local)) = value.rsplit_once('.') {
        TypeExpr::Qualified {
            path: normalize_type_path(path),
            local: normalize_identifier(local),
        }
    } else {
        TypeExpr::Named(value.to_string())
    }
}

fn normalize_type_path_or_local(value: &str) -> String {
    if value.contains("::") || value.contains('.') {
        normalize_type_path(value)
    } else {
        normalize_identifier(value)
    }
}

fn normalize_type_path(value: &str) -> String {
    value
        .split("::")
        .flat_map(|part| part.split('.'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(normalize_identifier)
        .collect::<Vec<_>>()
        .join("::")
}

fn strip_rust_prefixes(mut value: &str) -> &str {
    loop {
        let trimmed = value.trim();
        if let Some(rest) = trimmed.strip_prefix('&') {
            value = if rest.trim_start().starts_with('\'') {
                rest.split_once(' ').map(|(_, rest)| rest).unwrap_or(rest)
            } else {
                rest
            };
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('*') {
            value = rest;
            continue;
        }
        let stripped = ["mut ", "const ", "volatile ", "dyn ", "impl ", "struct "]
            .into_iter()
            .find_map(|prefix| trimmed.strip_prefix(prefix));
        if let Some(rest) = stripped {
            value = rest;
            continue;
        }
        return value.trim_end_matches('*').trim();
    }
}

fn strip_outer_parens(input: &str) -> Option<&str> {
    let inner = input.strip_prefix('(')?.strip_suffix(')')?;
    if balanced_delimiters(inner) {
        Some(inner.trim())
    } else {
        None
    }
}

fn balanced_delimiters(input: &str) -> bool {
    let mut depth = 0i32;
    for ch in input.chars() {
        match ch {
            '(' | '[' | '<' | '{' => depth += 1,
            ')' | ']' | '>' | '}' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

fn split_generic(input: &str) -> Option<(&str, &str)> {
    let open = input.find('<')?;
    let inner = input.get(open + 1..input.len() - 1)?;
    let outer = input[..open].trim();
    if input.ends_with('>') && !outer.is_empty() && balanced_delimiters(inner) {
        Some((outer, inner))
    } else {
        None
    }
}

fn first_generic_argument(input: &str) -> &str {
    let mut depth = 0i32;
    for (index, ch) in input.char_indices() {
        match ch {
            '<' | '[' | '(' | '{' => depth += 1,
            '>' | ']' | ')' | '}' => depth -= 1,
            ',' if depth == 0 => return &input[..index],
            _ => {}
        }
    }
    input
}

fn is_byte_scalar(input: &str) -> bool {
    matches!(
        input.trim(),
        "u8" | "u8_t" | "uint8_t" | "byte" | "[]u8" | "[:0]u8"
    )
}

fn normalize_impl_base(input: &str, family: &str) -> String {
    match family {
        "c" => match input {
            "int" | "signed" | "signed int" => "i32".to_string(),
            "unsigned" | "unsigned int" => "u32".to_string(),
            "short" | "short int" => "i16".to_string(),
            "long" | "long int" | "long long" | "long long int" => "i64".to_string(),
            "float" => "f32".to_string(),
            "double" => "f64".to_string(),
            "char" | "signed char" => "i8".to_string(),
            "unsigned char" => "u8".to_string(),
            "bool" => "bool".to_string(),
            "size_t" => "usize".to_string(),
            "int8_t" => "i8".to_string(),
            "int16_t" => "i16".to_string(),
            "int32_t" => "i32".to_string(),
            "int64_t" => "i64".to_string(),
            "uint8_t" => "u8".to_string(),
            "uint16_t" => "u16".to_string(),
            "uint32_t" => "u32".to_string(),
            "uint64_t" => "u64".to_string(),
            _ => normalize_identifier(input),
        },
        "zig" => match input {
            "int" | "c_int" => "i32".to_string(),
            "float" => "f32".to_string(),
            "bool" => "bool".to_string(),
            "usize" => "usize".to_string(),
            "isize" => "isize".to_string(),
            _ => normalize_type_path_or_local(input),
        },
        "ts" => match input {
            "string" => "string".to_string(),
            "boolean" => "bool".to_string(),
            "any" => "any".to_string(),
            _ => normalize_type_path_or_local(input),
        },
        _ => match input {
            "String" | "str" => "string".to_string(),
            "char" => "char".to_string(),
            "bool" => "bool".to_string(),
            "()" | "void" => "unit".to_string(),
            "any" | "Any" => "any".to_string(),
            "int" => "i64".to_string(),
            "float" => "f64".to_string(),
            "Uuid" | "uuid" => "uuid".to_string(),
            "DateTime" | "datetime" | "timestamp" => "timestamp".to_string(),
            _ => {
                let local = input
                    .rsplit("::")
                    .next()
                    .unwrap_or(input)
                    .rsplit('.')
                    .next()
                    .unwrap_or(input)
                    .trim();
                if input.starts_with("std::")
                    || input.starts_with("core::")
                    || input.starts_with("alloc::")
                {
                    if let Some(name) = builtin_local(local) {
                        return name.to_string();
                    }
                }
                match local {
                    "String" | "str" => "string".to_string(),
                    "bool" => "bool".to_string(),
                    "Uuid" | "uuid" => "uuid".to_string(),
                    "DateTime" | "datetime" | "timestamp" => "timestamp".to_string(),
                    _ if input.contains("::") || input.contains('.') => normalize_type_path(input),
                    _ => normalize_identifier(input),
                }
            }
        },
    }
}

fn builtin_local(local: &str) -> Option<&'static str> {
    match local {
        "String" | "str" => Some("string"),
        "bool" => Some("bool"),
        "Uuid" | "uuid" => Some("uuid"),
        "i8" => Some("i8"),
        "i16" => Some("i16"),
        "i32" => Some("i32"),
        "i64" => Some("i64"),
        "i128" => Some("i128"),
        "u8" => Some("u8"),
        "u16" => Some("u16"),
        "u32" => Some("u32"),
        "u64" => Some("u64"),
        "u128" => Some("u128"),
        "isize" => Some("isize"),
        "usize" => Some("usize"),
        "f16" => Some("f16"),
        "f32" => Some("f32"),
        "f64" => Some("f64"),
        _ => None,
    }
}

fn normalize_identifier(value: &str) -> String {
    let mut normalized = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            normalized.push(ch.to_ascii_lowercase());
        } else if !normalized.is_empty() && !normalized.ends_with('_') {
            normalized.push('_');
        }
    }
    normalized.trim_matches('_').to_string()
}

fn normalize_variant_name(value: &str) -> String {
    normalize_identifier(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_impl::types::{ParameterSignature, SymbolRange};
    use std::collections::BTreeMap;

    fn expected_func(input: Vec<&str>, output: Vec<&str>) -> ExpectedSymbol {
        ExpectedSymbol {
            stable_id: None,
            surv_name: "processData".to_string(),
            surv_path: Some("func.processData".to_string()),
            impl_bind: None,
            impl_lang: Some("rust".to_string()),
            impl_path: None,
            module_refs: Vec::new(),
            kind: SymbolKind::Func,
            input: input.into_iter().map(str::to_string).collect(),
            output: output.into_iter().map(str::to_string).collect(),
            fields: Default::default(),
        }
    }

    fn found_func(params: Vec<&str>, return_type: Option<&str>) -> FoundSymbol {
        FoundSymbol {
            language: Some("rust".to_string()),
            name: "processData".to_string(),
            kind: "Function".to_string(),
            uri: "file:///tmp/lib.rs".to_string(),
            relative_path: Some("src/lib.rs".to_string()),
            module_path: Some("lib".to_string()),
            impl_path: Some("lib::processData".to_string()),
            range: SymbolRange {
                start_line: 0,
                start_char: 0,
                end_line: 0,
                end_char: 1,
            },
            container_name: None,
            visibility: None,
            is_test: false,
            is_method: false,
            detail: None,
            alias_target: None,
            enum_variants: Vec::new(),
            enum_variants_known: false,
            signature: Some(FunctionSignature {
                parameters: params
                    .into_iter()
                    .map(|type_name| ParameterSignature {
                        name: Some("input".to_string()),
                        type_name: Some(type_name.to_string()),
                    })
                    .collect(),
                return_type: return_type.map(str::to_string),
            }),
            fields: Default::default(),
            fields_known: false,
        }
    }

    fn expected_schema(fields: Vec<(&str, &str)>) -> ExpectedSymbol {
        ExpectedSymbol {
            stable_id: None,
            surv_name: "User".to_string(),
            surv_path: Some("schema.User".to_string()),
            impl_bind: None,
            impl_lang: Some("rust".to_string()),
            impl_path: None,
            module_refs: Vec::new(),
            kind: SymbolKind::Schema,
            input: Vec::new(),
            output: Vec::new(),
            fields: fields
                .into_iter()
                .map(|(name, type_name)| (name.to_string(), type_name.to_string()))
                .collect(),
        }
    }

    fn found_schema(fields: Vec<(&str, &str)>) -> FoundSymbol {
        FoundSymbol {
            language: Some("rust".to_string()),
            name: "User".to_string(),
            kind: "Struct".to_string(),
            uri: "file:///tmp/lib.rs".to_string(),
            relative_path: Some("src/lib.rs".to_string()),
            module_path: Some("lib".to_string()),
            impl_path: Some("lib::User".to_string()),
            range: SymbolRange {
                start_line: 0,
                start_char: 0,
                end_line: 0,
                end_char: 1,
            },
            container_name: None,
            visibility: None,
            is_test: false,
            is_method: false,
            detail: None,
            alias_target: None,
            enum_variants: Vec::new(),
            enum_variants_known: false,
            signature: None,
            fields_known: true,
            fields: fields
                .into_iter()
                .map(|(name, type_name)| (name.to_string(), type_name.to_string()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn accepts_matching_signature() {
        let expected = expected_func(vec!["schema.TestData"], vec!["schema.TestData"]);
        let found = found_func(vec!["TestData"], Some("Result<TestData, Error>"));

        assert!(
            compare_signature(&expected, &found, &TypeContext::default())
                .0
                .is_none()
        );
    }

    #[test]
    fn reports_signature_mismatch() {
        let expected = expected_func(vec!["schema.TestData"], vec!["schema.TestData"]);
        let found = found_func(vec!["OtherData"], Some("OtherData"));

        let mismatch = compare_signature(&expected, &found, &TypeContext::default())
            .0
            .expect("mismatch");
        assert_eq!(mismatch.problems.len(), 2);
        assert!(mismatch.problems[0].contains("schema.testdata"));
        assert!(mismatch.problems[1].contains("schema.testdata"));
    }

    #[test]
    fn compares_optional_and_vec_by_structure() {
        let cases = [
            ("int", "i64", TypeComparison::Matches),
            ("int?", "Option<i64>", TypeComparison::Matches),
            ("int", "Option<i64>", TypeComparison::Mismatch),
            ("int[]", "Vec<i64>", TypeComparison::Matches),
            ("int[]?", "Option<Vec<i64>>", TypeComparison::Matches),
            ("int?[]", "Vec<Option<i64>>", TypeComparison::Matches),
            ("int[]?", "Vec<Option<i64>>", TypeComparison::Mismatch),
            ("int?[]", "Option<Vec<i64>>", TypeComparison::Mismatch),
            ("int[]", "Vec<f64>", TypeComparison::Mismatch),
            ("int", "i32", TypeComparison::Mismatch),
            ("int", "usize", TypeComparison::Mismatch),
            ("float", "f64", TypeComparison::Matches),
            ("float", "f32", TypeComparison::Mismatch),
            ("int", "f64", TypeComparison::Mismatch),
            ("[User]", "Vec<User>", TypeComparison::Matches),
            ("int[]", "&[i64]", TypeComparison::Matches),
            ("int?", "Option<&i64>", TypeComparison::Matches),
            ("int?", "&'a Option<i64>", TypeComparison::Matches),
            ("bytes", "Vec<u8>", TypeComparison::Matches),
            ("unit", "()", TypeComparison::Matches),
            ("any", "Any", TypeComparison::Unverified),
            ("int", "Int", TypeComparison::Unverified),
            ("schema.User", "UserView", TypeComparison::Mismatch),
            ("pending|paid", "Status", TypeComparison::Unverified),
            ("int?", "Option<Box<i64>>", TypeComparison::Unverified),
            ("int[]", "Vec<Box<i64>>", TypeComparison::Unverified),
            (
                "std::option::Option<int>",
                "Option<i64>",
                TypeComparison::Mismatch,
            ),
        ];
        for (ir, rust, expected) in cases {
            let actual = compare_type_expr(
                &parse_ir_type(ir, None),
                &parse_impl_type(rust, None, None),
                &TypeContext::default(),
            );
            assert_eq!(actual, expected, "{ir} vs {rust}: {actual:?}");
        }
    }

    #[test]
    fn unknown_types_are_unverified_not_mismatches() {
        let actual = parse_impl_type("Box<i64>", None, None);
        assert_eq!(actual, TypeExpr::Unknown);
        assert_eq!(
            compare_type_expr(
                &parse_ir_type("int", None),
                &actual,
                &TypeContext::default()
            ),
            TypeComparison::Unverified
        );
    }

    #[test]
    fn reports_unverified_fields_separately_from_mismatches() {
        let expected = expected_schema(vec![("count", "int")]);
        let found = found_schema(vec![("count", "Box<i64>")]);
        let (mismatch, unverified) =
            compare_schema_fields(&expected, &found, &TypeContext::default());
        assert!(mismatch.is_none());
        assert_eq!(unverified.len(), 1);
        assert!(unverified[0].contains("could not be verified"));
    }

    #[test]
    fn reports_unverified_signatures_separately_from_mismatches() {
        let expected = expected_func(vec!["int"], vec!["int"]);
        let found = found_func(vec!["Box<i64>"], Some("Box<i64>"));
        let (mismatch, unverified) = compare_signature(&expected, &found, &TypeContext::default());
        assert!(mismatch.is_none());
        assert_eq!(unverified.len(), 2);
    }

    #[test]
    fn uses_language_specific_scalar_mapping() {
        assert_eq!(
            parse_ir_type("int", Some("c")),
            TypeExpr::Named("i32".to_string())
        );
        assert_eq!(
            parse_impl_type("int", None, Some("c")),
            TypeExpr::Named("i32".to_string())
        );
        assert_eq!(
            compare_type_expr(
                &parse_ir_type("int", Some("c")),
                &parse_impl_type("int", None, Some("c")),
                &TypeContext::default(),
            ),
            TypeComparison::Matches
        );
        assert_eq!(
            compare_type_expr(
                &parse_ir_type("int", Some("rust")),
                &parse_impl_type("i32", None, Some("rust")),
                &TypeContext::default(),
            ),
            TypeComparison::Mismatch
        );
        assert_eq!(
            parse_impl_type("number", None, Some("ts")),
            TypeExpr::Unknown
        );
        assert_eq!(
            parse_impl_type("?i32", None, Some("zig")),
            TypeExpr::Optional(Box::new(TypeExpr::Named("i32".to_string())))
        );
    }

    #[test]
    fn compares_nested_types_in_signatures_and_fields() {
        let expected = expected_func(vec!["int?[]"], vec!["int[]?"]);
        let found = found_func(
            vec!["Vec<Option<i64>>"],
            Some("Result<Option<Vec<i64>>, Error>"),
        );
        assert!(
            compare_signature(&expected, &found, &TypeContext::default())
                .0
                .is_none()
        );

        let expected = expected_schema(vec![
            ("count", "int"),
            ("sizes", "int[]"),
            ("matrix", "int[]?"),
        ]);
        let found = found_schema(vec![
            ("count", "Option<i64>"),
            ("sizes", "Vec<i64>"),
            ("matrix", "Option<Vec<i64>>"),
        ]);
        let mismatch = compare_schema_fields(&expected, &found, &TypeContext::default())
            .0
            .expect("mismatch");
        assert_eq!(mismatch.problems.len(), 1);
        assert!(mismatch.problems[0].contains("count"));

        let found = found_schema(vec![
            ("count", "i64"),
            ("sizes", "Vec<f64>"),
            ("matrix", "Option<Vec<i64>>"),
        ]);
        assert!(
            compare_schema_fields(&expected, &found, &TypeContext::default())
                .0
                .expect("mismatch")
                .problems
                .iter()
                .any(|problem| problem.contains("sizes"))
        );
    }

    #[test]
    fn compares_optional_result_and_reference_types() {
        let expected = expected_func(vec!["int[]"], vec!["int[]"]);
        let found = found_func(vec!["&[i64]"], Some("Result<Vec<i64>, Error>"));
        assert!(
            compare_signature(&expected, &found, &TypeContext::default())
                .0
                .is_none()
        );
    }

    #[test]
    fn reports_schema_field_mismatch() {
        let expected = expected_schema(vec![("id", "String"), ("name", "String")]);
        let found = found_schema(vec![("id", "String"), ("age", "u32")]);

        let mismatch = compare_schema_fields(&expected, &found, &TypeContext::default())
            .0
            .expect("mismatch");
        assert_eq!(mismatch.problems.len(), 2);
        assert!(mismatch.problems[0].contains("name"));
        assert!(mismatch.problems[1].contains("age"));
    }

    #[test]
    fn keeps_unmatched_same_name_symbol_in_other_file_as_extra() {
        let mut expected = expected_func(vec![], vec![]);
        expected.impl_path = Some("alpha".to_string());

        let mut found_alpha = found_func(vec![], None);
        found_alpha.uri = "file:///tmp/alpha.rs".to_string();
        found_alpha.relative_path = Some("src/alpha.rs".to_string());
        found_alpha.module_path = Some("alpha".to_string());
        found_alpha.impl_path = Some("alpha::processData".to_string());

        let mut found_beta = found_func(vec![], None);
        found_beta.uri = "file:///tmp/beta.rs".to_string();
        found_beta.relative_path = Some("src/beta.rs".to_string());
        found_beta.module_path = Some("beta".to_string());
        found_beta.impl_path = Some("beta::processData".to_string());

        let result = match_symbols(&[expected], &[found_alpha, found_beta], "rust", None);
        assert_eq!(result.matched, 1);
        assert_eq!(result.extra.len(), 1);
        assert_eq!(result.extra[0].uri, "file:///tmp/beta.rs");
    }

    #[test]
    fn matches_impl_path_by_segments_without_partial_token_false_positives() {
        let mut symbol = found_func(vec![], None);
        symbol.module_path = Some("superuser_handler".to_string());
        symbol.impl_path = Some("superuser_handler::processData".to_string());
        symbol.relative_path = Some("src/superuser_handler.rs".to_string());

        assert!(!symbol_matches_expected_path(&symbol, "user"));

        symbol.module_path = Some("ui::workspace::tree".to_string());
        symbol.impl_path = Some("ui::workspace::tree::processData".to_string());
        symbol.relative_path = Some("src/ui/workspace/tree.ts".to_string());

        assert!(symbol_matches_expected_path(&symbol, "ui.workspace.tree"));
        assert!(symbol_matches_expected_path(&symbol, "workspace::tree"));
    }
}
