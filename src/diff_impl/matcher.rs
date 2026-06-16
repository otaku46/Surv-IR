use super::lsp_client::LspClient;
use super::mapping::{find_by_surv_ref, load_mapping};
use super::static_analysis::StaticAnalyzer;
use super::types::{
    DiffResult, ExpectedSymbol, FoundSymbol, FunctionSignature, MappingFile, SchemaFieldMismatch,
    SignatureMismatch, SymbolKind,
};
use crate::ast::Section;
use crate::loader::load_project;
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
    strategy: &str, // "lsp" or "tree-sitter"
    mapping_path: Option<&Path>,
) -> Result<DiffResult, Box<dyn Error>> {
    // 1. Parse IR file and extract expected symbols
    let expected = extract_expected_symbols(ir_file, filter_mod)?;
    let mapping = if let Some(path) = mapping_path {
        Some(load_mapping(path)?)
    } else {
        None
    };

    // 2. Query symbols based on strategy
    let found = if strategy == "lsp" {
        query_workspace_symbols_lsp(workspace_root, language, &expected)?
    } else {
        query_workspace_symbols_static(workspace_root, language)?
    };

    // 3. Match expected vs found
    let result = match_symbols(&expected, &found, language, mapping.as_ref());

    Ok(result)
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
) -> Result<Vec<ExpectedSymbol>, Box<dyn Error>> {
    if is_manifest_path(ir_file) {
        if let Ok(project) = load_project(ir_file) {
            let sections = project
                .files
                .iter()
                .flat_map(|(_, file)| file.sections.clone())
                .collect::<Vec<_>>();
            return extract_expected_symbols_from_sections(&sections, filter_mod);
        }
    }

    let parsed = parse_file(ir_file)?;
    extract_expected_symbols_from_sections(&parsed.sections, filter_mod)
}

fn extract_expected_symbols_from_sections(
    sections: &[Section],
    filter_mod: Option<&str>,
) -> Result<Vec<ExpectedSymbol>, Box<dyn Error>> {
    let mut expected = Vec::new();
    let module_refs = collect_module_refs(sections);

    // If filter_mod is specified, compute reference closure
    let included_refs = if let Some(mod_name) = filter_mod {
        let mod_name = mod_name.strip_prefix("mod.").unwrap_or(mod_name);
        compute_reference_closure(sections, mod_name)?
    } else {
        // Include all schemas and funcs
        HashSet::new()
    };

    let use_filter = filter_mod.is_some();

    // Extract schemas
    for section in sections {
        if let Section::Schema(schema) = section {
            if use_filter && !included_refs.contains(&format!("schema.{}", schema.name)) {
                continue;
            }
            let surv_ref = format!("schema.{}", schema.name);

            expected.push(ExpectedSymbol {
                stable_id: None,
                surv_name: schema.name.clone(),
                surv_path: Some(surv_ref.clone()),
                impl_bind: schema.impl_bind.clone(),
                impl_lang: schema.impl_lang.clone(),
                impl_path: schema.impl_path.clone(),
                module_refs: module_refs.get(&surv_ref).cloned().unwrap_or_default(),
                kind: SymbolKind::Schema,
                input: Vec::new(),
                output: Vec::new(),
                fields: schema.fields.clone(),
            });
        }
    }

    // Extract functions
    for section in sections {
        if let Section::Func(func) = section {
            if use_filter && !included_refs.contains(&format!("func.{}", func.name)) {
                continue;
            }
            let surv_ref = format!("func.{}", func.name);

            expected.push(ExpectedSymbol {
                stable_id: None,
                surv_name: func.name.clone(),
                surv_path: Some(surv_ref.clone()),
                impl_bind: func.impl_bind.clone(),
                impl_lang: func.impl_lang.clone(),
                impl_path: func.impl_path.clone(),
                module_refs: module_refs.get(&surv_ref).cloned().unwrap_or_default(),
                kind: SymbolKind::Func,
                input: func.input.clone(),
                output: func.output.clone(),
                fields: Default::default(),
            });
        }
    }

    Ok(expected)
}

fn is_manifest_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == "surv.toml")
        .unwrap_or(false)
}

fn collect_module_refs(sections: &[Section]) -> HashMap<String, Vec<String>> {
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    for section in sections {
        if let Section::Mod(module) = section {
            let module_ref = format!("mod.{}", module.name);
            for schema in &module.schemas {
                refs.entry(schema.clone())
                    .or_default()
                    .push(module_ref.clone());
            }
            for func in &module.funcs {
                refs.entry(func.clone())
                    .or_default()
                    .push(module_ref.clone());
            }
        }
    }
    refs
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
fn query_workspace_symbols_lsp(
    workspace_root: &Path,
    language: &str,
    expected: &[ExpectedSymbol],
) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let mut all_symbols = Vec::new();

    let languages = if matches!(language, "both" | "all") {
        vec!["ts", "rust"]
    } else {
        vec![language]
    };

    for lang in languages {
        // Check if any expected symbols support this language
        let has_expected_for_lang = expected
            .iter()
            .any(|exp: &ExpectedSymbol| exp.matches_language(lang));

        if !has_expected_for_lang && matches!(language, "both" | "all") {
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
    mapping: Option<&MappingFile>,
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
        if !matches!(language, "both" | "all") && !exp.matches_language(language) {
            continue;
        }

        let search_name = exp.search_name();
        let candidates = find_candidates(&found_map, found, exp, search_name, mapping);

        match candidates.len() {
            0 => {
                // Missing
                result.missing.push(exp.clone());
            }
            1 => {
                // Matched
                result.matched += 1;
                if let Some(mismatch) = compare_signature(exp, candidates[0]) {
                    result.signature_mismatches.push(mismatch);
                }
                if let Some(mismatch) = compare_schema_fields(exp, candidates[0]) {
                    result.schema_field_mismatches.push(mismatch);
                }
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
    found: &'a [FoundSymbol],
    exp: &ExpectedSymbol,
    search_name: &str,
    mapping: Option<&MappingFile>,
) -> Vec<&'a FoundSymbol> {
    let mut candidates = Vec::new();

    if let (Some(mapping), Some(surv_path)) = (mapping, exp.surv_path.as_deref()) {
        if let Some(entry) = find_by_surv_ref(mapping, surv_path) {
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
    }

    if let Some(symbols) = found_map.get(search_name) {
        for symbol in symbols {
            // Check if symbol kind matches expectation
            if symbol_kind_matches(symbol, exp) {
                // Check impl.path if specified
                if let Some(expected_path) = &exp.impl_path {
                    let impl_path_matches = symbol
                        .impl_path
                        .as_deref()
                        .map(|path| path.contains(expected_path) || expected_path.contains(path))
                        .unwrap_or(false);
                    let container_matches = symbol
                        .container_name
                        .as_deref()
                        .map(|container| {
                            container.contains(expected_path) || expected_path.contains(container)
                        })
                        .unwrap_or(false);
                    let module_matches = symbol
                        .module_path
                        .as_deref()
                        .map(|module| {
                            module.contains(expected_path) || expected_path.contains(module)
                        })
                        .unwrap_or(false);

                    if impl_path_matches || container_matches || module_matches {
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

fn compare_signature(exp: &ExpectedSymbol, found: &FoundSymbol) -> Option<SignatureMismatch> {
    if exp.kind != SymbolKind::Func {
        return None;
    }

    let signature = found.signature.as_ref()?;
    let mut problems = Vec::new();
    let owner = found
        .container_name
        .as_deref()
        .or(Some(found.name.as_str()));

    compare_inputs(exp, signature, owner, &mut problems);
    compare_outputs(exp, signature, owner, &mut problems);

    if problems.is_empty() {
        None
    } else {
        Some(SignatureMismatch {
            expected: exp.clone(),
            found: found.clone(),
            problems,
        })
    }
}

fn compare_schema_fields(exp: &ExpectedSymbol, found: &FoundSymbol) -> Option<SchemaFieldMismatch> {
    if exp.kind != SymbolKind::Schema || exp.fields.is_empty() || found.fields.is_empty() {
        return None;
    }

    let mut problems = Vec::new();
    for (name, expected_type) in &exp.fields {
        match found.fields.get(name) {
            None => problems.push(format!("missing implementation field '{}'", name)),
            Some(actual_type) => {
                let expected = normalize_type_name(expected_type, None);
                let actual = normalize_type_name(actual_type, None);
                if !actual.contains(&expected) && !expected.contains(&actual) {
                    problems.push(format!(
                        "field '{}' type mismatch: IR '{}' vs implementation '{}'",
                        name, expected_type, actual_type
                    ));
                }
            }
        }
    }

    for name in found.fields.keys() {
        if !exp.fields.contains_key(name) {
            problems.push(format!("extra implementation field '{}'", name));
        }
    }

    if problems.is_empty() {
        None
    } else {
        Some(SchemaFieldMismatch {
            expected: exp.clone(),
            found: found.clone(),
            problems,
        })
    }
}

fn compare_inputs(
    exp: &ExpectedSymbol,
    signature: &FunctionSignature,
    owner: Option<&str>,
    problems: &mut Vec<String>,
) {
    let expected_inputs = normalized_schema_refs(&exp.input);
    if expected_inputs.is_empty() {
        return;
    }

    let actual_param_types: Vec<String> = signature
        .parameters
        .iter()
        .filter_map(|param| param.type_name.as_ref())
        .map(|t| normalize_type_name(t, owner))
        .collect();

    if actual_param_types.len() != expected_inputs.len() {
        problems.push(format!(
            "expected {} input parameter(s) from IR, found {} typed parameter(s)",
            expected_inputs.len(),
            actual_param_types.len()
        ));
    }

    for expected in expected_inputs {
        if !actual_param_types
            .iter()
            .any(|actual| type_mentions_schema(actual, &expected))
        {
            problems.push(format!(
                "missing input type compatible with schema.{}",
                expected
            ));
        }
    }
}

fn compare_outputs(
    exp: &ExpectedSymbol,
    signature: &FunctionSignature,
    owner: Option<&str>,
    problems: &mut Vec<String>,
) {
    let expected_outputs = normalized_schema_refs(&exp.output);
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

    let actual_return = normalize_type_name(return_type, owner);
    for expected in expected_outputs {
        if !type_mentions_schema(&actual_return, &expected) {
            problems.push(format!(
                "return type '{}' does not mention schema.{}",
                return_type, expected
            ));
        }
    }
}

fn normalized_schema_refs(refs: &[String]) -> Vec<String> {
    refs.iter()
        .map(|r| {
            r.strip_prefix("schema.")
                .or_else(|| r.rsplit_once(".schema.").map(|(_, name)| name))
                .unwrap_or(r)
        })
        .map(|value| normalize_type_name(value, None))
        .collect()
}

fn normalize_type_name(value: &str, owner: Option<&str>) -> String {
    let value = value
        .trim()
        .trim_start_matches('&')
        .trim_start_matches('*')
        .trim_start_matches("mut ")
        .trim_start_matches("dyn ")
        .trim_start_matches("impl ")
        .trim_start_matches("const ")
        .trim_start_matches("volatile ")
        .trim_start_matches("struct ")
        .trim_end_matches('*')
        .trim();

    if matches!(value, "Self" | "self") {
        return owner.map(normalize_type_name_scalar).unwrap_or_default();
    }

    if let Some(builtin) = builtin_scalar_name(value) {
        return builtin.to_string();
    }

    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>()
        .to_ascii_lowercase()
}

fn normalize_type_name_scalar(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>()
        .to_ascii_lowercase()
}

fn builtin_scalar_name(value: &str) -> Option<&'static str> {
    match value {
        "String" | "str" | "[]u8" | "[:0]u8" => Some("string"),
        "bool" => Some("bool"),
        "char" => Some("string"),
        "void" | "()" => Some("unit"),
        "usize" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32"
        | "i64" | "i128" | "f16" | "f32" | "f64" | "int" | "unsigned" | "long" | "short"
        | "size_t" | "uint8_t" | "uint16_t" | "uint32_t" | "uint64_t" | "int8_t" | "int16_t"
        | "int32_t" | "int64_t" | "c_int" | "c_uint" => Some("number"),
        "anytype" | "type" => Some("anytype"),
        _ => None,
    }
}

fn type_mentions_schema(actual: &str, expected_schema: &str) -> bool {
    !expected_schema.is_empty() && actual.contains(expected_schema)
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
            signature: None,
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

        assert!(compare_signature(&expected, &found).is_none());
    }

    #[test]
    fn reports_signature_mismatch() {
        let expected = expected_func(vec!["schema.TestData"], vec!["schema.TestData"]);
        let found = found_func(vec!["OtherData"], Some("OtherData"));

        let mismatch = compare_signature(&expected, &found).expect("mismatch");
        assert_eq!(mismatch.problems.len(), 2);
        assert!(mismatch.problems[0].contains("schema.testdata"));
        assert!(mismatch.problems[1].contains("schema.testdata"));
    }

    #[test]
    fn reports_schema_field_mismatch() {
        let expected = expected_schema(vec![("id", "String"), ("name", "String")]);
        let found = found_schema(vec![("id", "String"), ("age", "u32")]);

        let mismatch = compare_schema_fields(&expected, &found).expect("mismatch");
        assert_eq!(mismatch.problems.len(), 2);
        assert!(mismatch.problems[0].contains("name"));
        assert!(mismatch.problems[1].contains("age"));
    }
}
