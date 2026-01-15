use super::lsp_client::LspClient;
use super::types::{DiffResult, ExpectedSymbol, FoundSymbol, SymbolKind};
use crate::ast::Section;
use crate::parser::parse_file;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

/// Main entry point for diff-impl
pub fn diff_impl(
    ir_file: &Path,
    workspace_root: &Path,
    filter_mod: Option<&str>,
    language: &str,
) -> Result<DiffResult, Box<dyn Error>> {
    // 1. Parse IR file and extract expected symbols
    let expected = extract_expected_symbols(ir_file, filter_mod)?;

    // 2. Query LSP for actual symbols
    let found = query_workspace_symbols(workspace_root, language, &expected)?;

    // 3. Match expected vs found
    let result = match_symbols(&expected, &found, language);

    Ok(result)
}

/// Extract expected symbols from IR file
fn extract_expected_symbols(
    ir_file: &Path,
    filter_mod: Option<&str>,
) -> Result<Vec<ExpectedSymbol>, Box<dyn Error>> {
    let parsed = parse_file(ir_file)?;

    let mut expected = Vec::new();

    // If filter_mod is specified, compute reference closure
    let included_refs = if let Some(mod_name) = filter_mod {
        let mod_name = mod_name.strip_prefix("mod.").unwrap_or(mod_name);
        compute_reference_closure(&parsed.sections, mod_name)?
    } else {
        // Include all schemas and funcs
        HashSet::new()
    };

    let use_filter = filter_mod.is_some();

    // Extract schemas
    for section in &parsed.sections {
        if let Section::Schema(schema) = section {
            if use_filter && !included_refs.contains(&format!("schema.{}", schema.name)) {
                continue;
            }

            expected.push(ExpectedSymbol {
                surv_name: schema.name.clone(),
                impl_bind: schema.impl_bind.clone(),
                impl_lang: schema.impl_lang.clone(),
                impl_path: schema.impl_path.clone(),
                kind: SymbolKind::Schema,
            });
        }
    }

    // Extract functions
    for section in &parsed.sections {
        if let Section::Func(func) = section {
            if use_filter && !included_refs.contains(&format!("func.{}", func.name)) {
                continue;
            }

            expected.push(ExpectedSymbol {
                surv_name: func.name.clone(),
                impl_bind: func.impl_bind.clone(),
                impl_lang: func.impl_lang.clone(),
                impl_path: func.impl_path.clone(),
                kind: SymbolKind::Func,
            });
        }
    }

    Ok(expected)
}

/// Compute reference closure for a module (similar to surc inspect)
fn compute_reference_closure(
    sections: &[Section],
    mod_name: &str,
) -> Result<HashSet<String>, Box<dyn Error>> {
    let mut closure = HashSet::new();

    // Find the module
    let module = sections.iter().find_map(|s| {
        if let Section::Mod(m) = s {
            if m.name == mod_name {
                Some(m)
            } else {
                None
            }
        } else {
            None
        }
    });

    let module = module.ok_or_else(|| format!("Module 'mod.{}' not found", mod_name))?;

    // Add direct references
    for schema_ref in &module.schemas {
        closure.insert(schema_ref.clone());
    }

    for func_ref in &module.funcs {
        closure.insert(func_ref.clone());
    }

    // Add transitive references from functions (input/output schemas)
    let mut to_process: Vec<String> = module.funcs.clone();
    let mut processed = HashSet::new();

    while let Some(func_ref) = to_process.pop() {
        if processed.contains(&func_ref) {
            continue;
        }
        processed.insert(func_ref.clone());

        // Find the function
        let func_name = func_ref.strip_prefix("func.").unwrap_or(&func_ref);
        if let Some(Section::Func(func)) = sections.iter().find(|s| {
            if let Section::Func(f) = s {
                f.name == func_name
            } else {
                false
            }
        }) {
            // Add input/output schemas
            for schema_ref in func.input.iter().chain(func.output.iter()) {
                closure.insert(schema_ref.clone());
            }
        }
    }

    Ok(closure)
}

/// Query LSP servers for workspace symbols
fn query_workspace_symbols(
    workspace_root: &Path,
    language: &str,
    expected: &[ExpectedSymbol],
) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let mut all_symbols = Vec::new();

    let languages = if language == "both" {
        vec!["ts", "rust"]
    } else {
        vec![language]
    };

    for lang in languages {
        // Check if any expected symbols support this language
        let has_expected_for_lang = expected
            .iter()
            .any(|exp| exp.matches_language(lang));

        if !has_expected_for_lang && language == "both" {
            continue;
        }

        match query_language_symbols(workspace_root, lang, expected) {
            Ok(mut symbols) => all_symbols.append(&mut symbols),
            Err(e) => {
                eprintln!("Warning: Failed to query {} symbols: {}", lang, e);
            }
        }
    }

    Ok(all_symbols)
}

fn query_language_symbols(
    workspace_root: &Path,
    lang: &str,
    _expected: &[ExpectedSymbol],
) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let mut client = LspClient::new(lang, workspace_root)?;

    // Query for all symbols (empty query returns all)
    let symbols = client.workspace_symbol("")?;

    client.shutdown()?;

    Ok(symbols)
}

/// Match expected symbols against found symbols
fn match_symbols(
    expected: &[ExpectedSymbol],
    found: &[FoundSymbol],
    language: &str,
) -> DiffResult {
    let mut result = DiffResult::default();

    // Create a map of found symbols by name for quick lookup
    let mut found_map: HashMap<String, Vec<&FoundSymbol>> = HashMap::new();
    for symbol in found {
        found_map
            .entry(symbol.name.clone())
            .or_default()
            .push(symbol);
    }

    let mut matched_found = HashSet::new();

    // Match each expected symbol
    for exp in expected {
        // Skip if language doesn't match
        if language != "both" && !exp.matches_language(language) {
            continue;
        }

        let search_name = exp.search_name();
        let candidates = find_candidates(&found_map, exp, search_name);

        match candidates.len() {
            0 => {
                // Missing
                result.missing.push(exp.clone());
            }
            1 => {
                // Matched
                result.matched += 1;
                matched_found.insert(candidates[0].name.clone());
            }
            _ => {
                // Ambiguous
                result.ambiguous.push((
                    exp.clone(),
                    candidates.iter().map(|&s| (*s).clone()).collect(),
                ));
                for candidate in &candidates {
                    matched_found.insert(candidate.name.clone());
                }
            }
        }
    }

    // Find extra symbols (in code but not in IR)
    // Note: This can be very noisy, so we'll be conservative
    // Only report symbols that look like they could be expected
    for symbol in found {
        if !matched_found.contains(&symbol.name) {
            // Filter noise: only include types that match our expectations
            if is_relevant_symbol(symbol) {
                result.extra.push(symbol.clone());
            }
        }
    }

    result
}

fn find_candidates<'a>(
    found_map: &'a HashMap<String, Vec<&'a FoundSymbol>>,
    exp: &ExpectedSymbol,
    search_name: &str,
) -> Vec<&'a FoundSymbol> {
    let mut candidates = Vec::new();

    if let Some(symbols) = found_map.get(search_name) {
        for symbol in symbols {
            // Check if symbol kind matches expectation
            if symbol_kind_matches(symbol, exp) {
                // Check impl.path if specified
                if let Some(expected_path) = &exp.impl_path {
                    if let Some(container) = &symbol.container_name {
                        if container.contains(expected_path) || expected_path.contains(container) {
                            candidates.push(*symbol);
                        }
                    }
                } else {
                    candidates.push(*symbol);
                }
            }
        }
    }

    candidates
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
    // Only include symbols that are likely to be user-defined
    // Filter out common library symbols, test symbols, etc.
    matches!(
        symbol.kind.as_str(),
        "Function" | "Method" | "Interface" | "Class" | "Struct" | "Enum"
    )
}
