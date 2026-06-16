use crate::diff_impl::types::{FoundSymbol, FunctionSignature, ParameterSignature, SymbolRange};
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fs;
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor};
use walkdir::WalkDir;

pub struct StaticAnalyzer {
    parser_rust: Parser,
    parser_ts: Parser,
    query_rust: Query,
    query_ts: Query,
}

impl StaticAnalyzer {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let mut parser_rust = Parser::new();
        parser_rust.set_language(tree_sitter_rust::language())?;

        let mut parser_ts = Parser::new();
        parser_ts.set_language(tree_sitter_typescript::language_typescript())?;

        // Queries to extract relevant symbols
        // Captures: @name (identifier), @kind (string mapping handled in code), @container (optional)

        let rust_query_str = r#"
            (function_item name: (identifier) @name) @def
            (struct_item name: (type_identifier) @name) @def
            (enum_item name: (type_identifier) @name) @def
            (trait_item name: (type_identifier) @name) @def
            (type_item name: (type_identifier) @name) @def
            (impl_item 
                trait: (type_identifier)? @container
                body: (declaration_list 
                    (function_item name: (identifier) @name) @def
                )
            )
        "#;

        let ts_query_str = r#"
            (function_declaration name: (identifier) @name) @def
            (class_declaration name: (type_identifier) @name) @def
            (interface_declaration name: (type_identifier) @name) @def
            (type_alias_declaration name: (type_identifier) @name) @def
            (enum_declaration name: (identifier) @name) @def
            
            ; Arrow functions assigned to variables (const myFunc = () => {})
            (lexical_declaration 
                (variable_declarator 
                    name: (identifier) @name 
                    value: (arrow_function)
                )
            ) @def

            ; Class methods
            (class_declaration 
                name: (type_identifier) @container
                body: (class_body 
                    (method_definition name: (property_identifier) @name) @def
                )
            )
        "#;

        let query_rust = Query::new(tree_sitter_rust::language(), rust_query_str)?;
        let query_ts = Query::new(tree_sitter_typescript::language_typescript(), ts_query_str)?;

        Ok(Self {
            parser_rust,
            parser_ts,
            query_rust,
            query_ts,
        })
    }

    pub fn scan_workspace(
        &mut self,
        root: &Path,
        lang_filter: &str,
    ) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
        let mut symbols = Vec::new();

        for entry in WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Skip node_modules and target directories for performance
            if path.components().any(|c| {
                c.as_os_str() == "node_modules"
                    || c.as_os_str() == "target"
                    || c.as_os_str() == ".git"
            }) {
                continue;
            }

            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

            match ext {
                "rs" if lang_filter == "both" || lang_filter == "rust" => {
                    symbols.append(&mut self.scan_file(root, path, "rust")?);
                }
                "ts" | "tsx"
                    if lang_filter == "both"
                        || lang_filter == "ts"
                        || lang_filter == "typescript" =>
                {
                    symbols.append(&mut self.scan_file(root, path, "ts")?);
                }
                "c" | "h" if lang_filter == "c" || lang_filter == "all" => {
                    symbols.append(&mut scan_c_file(root, path)?);
                }
                "zig" if lang_filter == "zig" || lang_filter == "all" => {
                    symbols.append(&mut scan_zig_file(root, path)?);
                }
                _ => {}
            }
        }

        Ok(dedup_symbols(symbols))
    }

    fn scan_file(
        &mut self,
        root: &Path,
        path: &Path,
        lang: &str,
    ) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
        let content = fs::read_to_string(path)?;
        let (parser, query) = if lang == "rust" {
            (&mut self.parser_rust, &self.query_rust)
        } else {
            (&mut self.parser_ts, &self.query_ts)
        };

        let tree = parser.parse(&content, None).ok_or("Failed to parse file")?;
        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(query, tree.root_node(), content.as_bytes());

        let mut symbols = Vec::new();

        for m in matches {
            let mut name = String::new();
            let mut container = None;
            let mut range_node = None;

            for capture in m.captures {
                let capture_name = query.capture_names()[capture.index as usize].as_str();
                match capture_name {
                    "name" => {
                        if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                            name = text.to_string();
                        }
                    }
                    "container" => {
                        if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                            container = Some(text.to_string());
                        }
                    }
                    "def" => {
                        range_node = Some(capture.node);
                    }
                    _ => {}
                }
            }

            if !name.is_empty() {
                let node = range_node.unwrap_or_else(|| m.captures[0].node);
                let start = node.start_position();
                let end = node.end_position();

                let kind_str = map_node_kind_to_symbol_kind(node.kind());
                let def_text = node.utf8_text(content.as_bytes()).unwrap_or("");
                if lang == "rust" && node.kind() == "function_item" {
                    if let Some(owner) = infer_rust_impl_container(&node, &content) {
                        container = Some(owner);
                    }
                }
                let signature = if kind_str == "Function" || kind_str == "Variable" {
                    extract_signature(def_text, lang, &name)
                } else {
                    None
                };
                let fields = if is_schema_kind(&kind_str) && node.kind() != "trait_item" {
                    extract_fields(def_text, lang)
                } else {
                    BTreeMap::new()
                };
                let relative_path = relative_path(root, path);
                let module_path = relative_path.as_deref().map(|p| infer_module_path(p, lang));
                let is_method = container.is_some();
                let impl_path =
                    build_impl_path(module_path.as_deref(), container.as_deref(), &name);
                let visibility = extract_visibility(def_text);
                let is_test = is_test_symbol(path, &content, &node, &name);

                if lang == "rust" && node.kind() == "trait_item" {
                    symbols.extend(extract_trait_methods(
                        root,
                        path,
                        &content,
                        &node,
                        module_path.as_deref(),
                        &name,
                    ));
                }

                symbols.push(FoundSymbol {
                    language: Some(lang.to_string()),
                    name,
                    kind: kind_str,
                    uri: format!("file://{}", path.display()),
                    relative_path,
                    module_path,
                    impl_path,
                    range: SymbolRange {
                        start_line: start.row as u32,
                        start_char: start.column as u32,
                        end_line: end.row as u32,
                        end_char: end.column as u32,
                    },
                    container_name: container,
                    visibility,
                    is_test,
                    is_method,
                    detail: None,
                    signature,
                    fields,
                });
            }
        }

        Ok(symbols)
    }
}

fn scan_c_file(root: &Path, path: &Path) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let clean = strip_c_preprocessor_and_comments(&content);
    let mut symbols = Vec::new();
    let relative_path = relative_path(root, path);
    let module_path = relative_path.as_deref().map(|p| infer_module_path(p, "c"));

    symbols.extend(extract_c_structs(
        path,
        &clean,
        relative_path.clone(),
        module_path.clone(),
    ));

    if path.extension().and_then(|s| s.to_str()) == Some("c") {
        symbols.extend(extract_c_functions(
            path,
            &clean,
            relative_path,
            module_path,
        ));
    }

    Ok(symbols)
}

fn scan_zig_file(root: &Path, path: &Path) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let clean = strip_line_comments(&content);
    let relative_path = relative_path(root, path);
    let module_path = relative_path
        .as_deref()
        .map(|p| infer_module_path(p, "zig"));

    let mut symbols = Vec::new();
    symbols.extend(extract_zig_types(
        path,
        &clean,
        relative_path.clone(),
        module_path.clone(),
    ));
    symbols.extend(extract_zig_functions(
        path,
        &clean,
        relative_path,
        module_path,
    ));
    Ok(symbols)
}

fn is_schema_kind(kind: &str) -> bool {
    matches!(kind, "Struct" | "Interface" | "Class" | "Type")
}

fn extract_fields(def_text: &str, lang: &str) -> BTreeMap<String, String> {
    let Some(body) = extract_first_balanced_body(def_text) else {
        return BTreeMap::new();
    };

    let mut fields = BTreeMap::new();
    let field_body = if lang == "ts" {
        body.replace(';', ",")
    } else {
        body
    };

    for field in split_top_level(&field_body, ',') {
        let field = field.trim();
        if field.is_empty() || field.starts_with("//") {
            continue;
        }
        if let Some((name, type_name)) = parse_field(field, lang) {
            fields.insert(name, type_name);
        }
    }
    fields
}

fn extract_trait_methods(
    root: &Path,
    path: &Path,
    content: &str,
    trait_node: &tree_sitter::Node,
    module_path: Option<&str>,
    trait_name: &str,
) -> Vec<FoundSymbol> {
    let Some(body) = trait_node.child_by_field_name("body") else {
        return Vec::new();
    };

    let mut methods = Vec::new();
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() != "function_signature_item" {
            continue;
        }
        let method_name = child
            .child_by_field_name("name")
            .and_then(|node| node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");
        if method_name.is_empty() {
            continue;
        }

        let def_text = child.utf8_text(content.as_bytes()).unwrap_or("");
        let signature = extract_signature(def_text, "rust", method_name);
        let start = child.start_position();
        let end = child.end_position();
        let relative_path = relative_path(root, path);
        let method_impl_path = build_impl_path(module_path, Some(trait_name), method_name);

        methods.push(FoundSymbol {
            language: Some("rust".to_string()),
            name: method_name.to_string(),
            kind: "Function".to_string(),
            uri: format!("file://{}", path.display()),
            relative_path,
            module_path: module_path.map(str::to_string),
            impl_path: method_impl_path,
            range: SymbolRange {
                start_line: start.row as u32,
                start_char: start.column as u32,
                end_line: end.row as u32,
                end_char: end.column as u32,
            },
            container_name: Some(trait_name.to_string()),
            visibility: None,
            is_test: is_test_symbol(path, content, &child, method_name),
            is_method: true,
            detail: Some("trait signature".to_string()),
            signature,
            fields: BTreeMap::new(),
        });
    }

    methods
}

fn infer_rust_impl_container(node: &tree_sitter::Node, content: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "impl_item" {
            let container = parent
                .child_by_field_name("type")
                .or_else(|| parent.child_by_field_name("trait"))?;
            return container
                .utf8_text(content.as_bytes())
                .ok()
                .map(|text| text.to_string());
        }
        current = parent.parent();
    }
    None
}

fn extract_first_balanced_body(text: &str) -> Option<String> {
    let open = text.find('{')?;
    extract_balanced(text, open, '{', '}')
}

fn parse_field(field: &str, lang: &str) -> Option<(String, String)> {
    let field = field
        .trim()
        .trim_start_matches("pub ")
        .trim_start_matches("readonly ")
        .trim();

    if lang == "rust" {
        let (name, type_name) = field.split_once(':')?;
        let name = name.trim().trim_start_matches("r#").to_string();
        return Some((name, clean_type(type_name)));
    }

    let (name, type_name) = field.split_once(':')?;
    let name = name.trim().trim_end_matches('?').to_string();
    Some((name, clean_type(type_name)))
}

fn extract_c_functions(
    path: &Path,
    content: &str,
    relative_path: Option<String>,
    module_path: Option<String>,
) -> Vec<FoundSymbol> {
    let re = Regex::new(
        r"(?m)([A-Za-z_][A-Za-z0-9_\s\*\(\)]*?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^;{}]*)\)\s*\{",
    )
    .expect("valid C function regex");
    let mut symbols = Vec::new();

    for captures in re.captures_iter(content) {
        let Some(full_match) = captures.get(0) else {
            continue;
        };
        let return_type = clean_c_type(captures.get(1).map(|m| m.as_str()).unwrap_or(""));
        let name = captures.get(2).map(|m| m.as_str()).unwrap_or("");
        let params = captures.get(3).map(|m| m.as_str()).unwrap_or("");

        if is_c_control_keyword(name) || return_type.is_empty() {
            continue;
        }

        let (start_line, start_char) = line_char_at(content, full_match.start());
        let (end_line, end_char) = line_char_at(content, full_match.end());
        let signature = FunctionSignature {
            parameters: parse_c_parameters(params),
            return_type: if return_type == "void" {
                None
            } else {
                Some(return_type)
            },
        };

        symbols.push(FoundSymbol {
            language: Some("c".to_string()),
            name: name.to_string(),
            kind: "Function".to_string(),
            uri: format!("file://{}", path.display()),
            relative_path: relative_path.clone(),
            module_path: module_path.clone(),
            impl_path: build_impl_path(module_path.as_deref(), None, name),
            range: SymbolRange {
                start_line,
                start_char,
                end_line,
                end_char,
            },
            container_name: None,
            visibility: None,
            is_test: name.starts_with("test_") || name.ends_with("_test"),
            is_method: false,
            detail: None,
            signature: Some(signature),
            fields: BTreeMap::new(),
        });
    }

    symbols
}

fn extract_c_structs(
    path: &Path,
    content: &str,
    relative_path: Option<String>,
    module_path: Option<String>,
) -> Vec<FoundSymbol> {
    let re = Regex::new(
        r"(?s)(?:typedef\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)?\s*\{(.*?)\}\s*([A-Za-z_][A-Za-z0-9_]*)?\s*;",
    )
    .expect("valid C struct regex");
    let mut symbols = Vec::new();

    for captures in re.captures_iter(content) {
        let Some(full_match) = captures.get(0) else {
            continue;
        };
        let name = captures
            .get(3)
            .or_else(|| captures.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let body = captures.get(2).map(|m| m.as_str()).unwrap_or("");
        let fields = parse_c_fields(body);
        let (start_line, start_char) = line_char_at(content, full_match.start());
        let (end_line, end_char) = line_char_at(content, full_match.end());

        symbols.push(FoundSymbol {
            language: Some("c".to_string()),
            name: c_type_display_name(name),
            kind: "Struct".to_string(),
            uri: format!("file://{}", path.display()),
            relative_path: relative_path.clone(),
            module_path: module_path.clone(),
            impl_path: build_impl_path(module_path.as_deref(), None, &c_type_display_name(name)),
            range: SymbolRange {
                start_line,
                start_char,
                end_line,
                end_char,
            },
            container_name: None,
            visibility: None,
            is_test: false,
            is_method: false,
            detail: Some(format!("struct {}", name)),
            signature: None,
            fields,
        });
    }

    symbols
}

fn extract_zig_functions(
    path: &Path,
    content: &str,
    relative_path: Option<String>,
    module_path: Option<String>,
) -> Vec<FoundSymbol> {
    let re =
        Regex::new(r"(?m)^\s*pub\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)\s*([^{;\n]*)\{")
            .expect("valid Zig function regex");
    let mut symbols = Vec::new();

    for captures in re.captures_iter(content) {
        let Some(full_match) = captures.get(0) else {
            continue;
        };
        let name = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        let params = captures.get(2).map(|m| m.as_str()).unwrap_or("");
        let return_type = clean_zig_type(captures.get(3).map(|m| m.as_str()).unwrap_or(""));
        if name.is_empty() {
            continue;
        }
        let (start_line, start_char) = line_char_at(content, full_match.start());
        let (end_line, end_char) = line_char_at(content, full_match.end());

        symbols.push(FoundSymbol {
            language: Some("zig".to_string()),
            name: name.to_string(),
            kind: "Function".to_string(),
            uri: format!("file://{}", path.display()),
            relative_path: relative_path.clone(),
            module_path: module_path.clone(),
            impl_path: build_impl_path(module_path.as_deref(), None, name),
            range: SymbolRange {
                start_line,
                start_char,
                end_line,
                end_char,
            },
            container_name: None,
            visibility: Some("pub".to_string()),
            is_test: name.starts_with("test") || name.ends_with("Test"),
            is_method: false,
            detail: None,
            signature: Some(FunctionSignature {
                parameters: parse_zig_parameters(params),
                return_type: if return_type.is_empty() || return_type == "void" {
                    None
                } else {
                    Some(return_type)
                },
            }),
            fields: BTreeMap::new(),
        });
    }

    symbols
}

fn extract_zig_types(
    path: &Path,
    content: &str,
    relative_path: Option<String>,
    module_path: Option<String>,
) -> Vec<FoundSymbol> {
    let re = Regex::new(
        r"(?s)pub\s+const\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:extern\s+)?(struct|enum|union)\s*\{(.*?)\}\s*;",
    )
    .expect("valid Zig type regex");
    let mut symbols = Vec::new();

    for captures in re.captures_iter(content) {
        let Some(full_match) = captures.get(0) else {
            continue;
        };
        let name = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        let kind = match captures.get(2).map(|m| m.as_str()).unwrap_or("") {
            "enum" => "Enum",
            _ => "Struct",
        };
        let body = captures.get(3).map(|m| m.as_str()).unwrap_or("");
        let fields = if kind == "Struct" {
            parse_zig_fields(body)
        } else {
            BTreeMap::new()
        };
        let (start_line, start_char) = line_char_at(content, full_match.start());
        let (end_line, end_char) = line_char_at(content, full_match.end());

        symbols.push(FoundSymbol {
            language: Some("zig".to_string()),
            name: name.to_string(),
            kind: kind.to_string(),
            uri: format!("file://{}", path.display()),
            relative_path: relative_path.clone(),
            module_path: module_path.clone(),
            impl_path: build_impl_path(module_path.as_deref(), None, name),
            range: SymbolRange {
                start_line,
                start_char,
                end_line,
                end_char,
            },
            container_name: None,
            visibility: Some("pub".to_string()),
            is_test: false,
            is_method: false,
            detail: None,
            signature: None,
            fields,
        });
    }

    symbols
}

fn dedup_symbols(symbols: Vec<FoundSymbol>) -> Vec<FoundSymbol> {
    let mut seen = HashMap::new();
    let mut deduped = Vec::new();

    for symbol in symbols {
        let key = dedup_key(&symbol);
        match seen.get(&key).copied() {
            None => {
                seen.insert(key, deduped.len());
                deduped.push(symbol);
            }
            Some(idx) => {
                if is_better_symbol(&symbol, &deduped[idx]) {
                    deduped[idx] = symbol;
                }
            }
        }
    }

    deduped
}

fn dedup_key(symbol: &FoundSymbol) -> String {
    let mut parts = vec![
        symbol.language.as_deref().unwrap_or("").to_string(),
        symbol
            .relative_path
            .as_deref()
            .unwrap_or(&symbol.uri)
            .to_string(),
        symbol.kind.clone(),
        symbol.name.clone(),
    ];

    if let Some(signature) = &symbol.signature {
        parts.push(format!("sig={}", signature_fingerprint(signature)));
    } else if !symbol.fields.is_empty() {
        parts.push(format!("fields={}", fields_fingerprint(&symbol.fields)));
    } else {
        parts.push(format!(
            "range={}:{}-{}:{}",
            symbol.range.start_line,
            symbol.range.start_char,
            symbol.range.end_line,
            symbol.range.end_char
        ));
        if let Some(container) = &symbol.container_name {
            parts.push(format!("container={}", container));
        }
    }

    parts.join(":")
}

fn signature_fingerprint(signature: &FunctionSignature) -> String {
    let params = signature
        .parameters
        .iter()
        .map(|param| param.type_name.as_deref().unwrap_or("_").to_string())
        .collect::<Vec<_>>()
        .join(",");
    let return_type = signature.return_type.as_deref().unwrap_or("_");
    format!("({})->{}", params, return_type)
}

fn fields_fingerprint(fields: &BTreeMap<String, String>) -> String {
    fields
        .iter()
        .map(|(name, value)| format!("{}={}", name, value))
        .collect::<Vec<_>>()
        .join(",")
}

fn is_better_symbol(new: &FoundSymbol, existing: &FoundSymbol) -> bool {
    let new_score = symbol_score(new);
    let existing_score = symbol_score(existing);
    new_score > existing_score
}

fn symbol_score(symbol: &FoundSymbol) -> usize {
    let mut score = 0usize;
    if symbol.impl_path.is_some() {
        score += 4;
    }
    if symbol.signature.is_some() {
        score += 3;
    }
    if !symbol.fields.is_empty() {
        score += 3;
    }
    if symbol.visibility.is_some() {
        score += 1;
    }
    if !symbol.is_method {
        score += 1;
    }
    score
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

fn infer_module_path(relative_path: &str, lang: &str) -> String {
    let mut path = relative_path.replace('\\', "/");
    for prefix in ["src/", "lib/", "app/"] {
        if let Some(stripped) = path.strip_prefix(prefix) {
            path = stripped.to_string();
            break;
        }
    }

    for suffix in [".rs", ".ts", ".tsx", ".c", ".h", ".zig"] {
        if let Some(stripped) = path.strip_suffix(suffix) {
            path = stripped.to_string();
            break;
        }
    }

    if lang == "rust" {
        if path == "lib" || path == "main" {
            return path;
        }
        if let Some(stripped) = path.strip_suffix("/mod") {
            path = stripped.to_string();
        }
    }

    path.split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("::")
}

fn build_impl_path(
    module_path: Option<&str>,
    container: Option<&str>,
    name: &str,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(module_path) = module_path {
        if !module_path.is_empty() {
            parts.push(module_path.to_string());
        }
    }
    if let Some(container) = container {
        if !container.is_empty() {
            parts.push(container.to_string());
        }
    }
    parts.push(name.to_string());

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("::"))
    }
}

fn extract_visibility(def_text: &str) -> Option<String> {
    let trimmed = def_text.trim_start();
    if trimmed.starts_with("pub ") || trimmed.starts_with("pub(") {
        Some("pub".to_string())
    } else {
        None
    }
}

fn is_test_symbol(path: &Path, content: &str, node: &tree_sitter::Node, name: &str) -> bool {
    if name.starts_with("test_") || name.ends_with("_test") {
        return true;
    }

    if path.components().any(|c| c.as_os_str() == "tests") {
        return true;
    }

    let end = node.start_byte().min(content.len());
    let mut start = end.saturating_sub(256);
    while start < end && !content.is_char_boundary(start) {
        start += 1;
    }
    let prefix = &content[start..end];
    prefix.contains("#[test]") || prefix.contains("#[cfg(test)]")
}

fn extract_signature(def_text: &str, lang: &str, name: &str) -> Option<FunctionSignature> {
    let compact = def_text.replace('\n', " ");
    let params = extract_parenthesized_after_name(&compact, name)
        .or_else(|| extract_first_parenthesized(&compact))?;

    let parameters = split_top_level(&params, ',')
        .into_iter()
        .filter_map(|param| parse_parameter(&param))
        .collect();

    let return_type = if lang == "rust" {
        extract_rust_return_type(&compact)
    } else {
        extract_ts_return_type(&compact)
    };

    Some(FunctionSignature {
        parameters,
        return_type,
    })
}

fn extract_parenthesized_after_name(text: &str, name: &str) -> Option<String> {
    let name_pos = text.find(name)?;
    let open_rel = text[name_pos + name.len()..].find('(')?;
    let open = name_pos + name.len() + open_rel;
    extract_balanced(text, open, '(', ')')
}

fn extract_first_parenthesized(text: &str) -> Option<String> {
    let open = text.find('(')?;
    extract_balanced(text, open, '(', ')')
}

fn extract_balanced(text: &str, open: usize, open_ch: char, close_ch: char) -> Option<String> {
    let mut depth = 0usize;
    let mut start = None;
    for (offset, ch) in text[open..].char_indices() {
        if ch == open_ch {
            if depth == 0 {
                start = Some(open + offset + ch.len_utf8());
            }
            depth += 1;
        } else if ch == close_ch {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return start.map(|s| text[s..open + offset].trim().to_string());
            }
        }
    }
    None
}

fn split_top_level(input: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut angle = 0i32;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;

    for (idx, ch) in input.char_indices() {
        match ch {
            '<' => angle += 1,
            '>' => angle -= 1,
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            _ => {}
        }

        if ch == separator && angle <= 0 && paren <= 0 && bracket <= 0 && brace <= 0 {
            parts.push(input[start..idx].trim().to_string());
            start = idx + ch.len_utf8();
        }
    }

    let tail = input[start..].trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }
    parts
}

fn parse_parameter(param: &str) -> Option<ParameterSignature> {
    let param = param.trim();
    if param.is_empty() || param == "self" || param == "&self" || param == "&mut self" {
        return None;
    }

    let param = param
        .strip_prefix("pub ")
        .unwrap_or(param)
        .trim()
        .trim_start_matches("mut ")
        .trim();

    let (name, type_name) = param.split_once(':')?;
    Some(ParameterSignature {
        name: Some(
            name.trim()
                .trim_start_matches("mut ")
                .trim_start_matches('&')
                .trim()
                .to_string(),
        ),
        type_name: Some(clean_type(type_name)),
    })
}

fn extract_rust_return_type(text: &str) -> Option<String> {
    let re = Regex::new(r"->\s*([^\{;]+)").ok()?;
    let captures = re.captures(text)?;
    captures.get(1).map(|m| clean_type(m.as_str()))
}

fn extract_ts_return_type(text: &str) -> Option<String> {
    let re = Regex::new(r"\)\s*:\s*([^=\{]+)").ok()?;
    let captures = re.captures(text)?;
    captures.get(1).map(|m| clean_type(m.as_str()))
}

fn clean_type(type_name: &str) -> String {
    type_name
        .trim()
        .trim_end_matches("where")
        .trim()
        .trim_end_matches("=>")
        .trim()
        .trim_end_matches('{')
        .trim()
        .to_string()
}

fn strip_c_preprocessor_and_comments(content: &str) -> String {
    let without_blocks = Regex::new(r"(?s)/\*.*?\*/")
        .expect("valid block comment regex")
        .replace_all(content, " ");
    without_blocks
        .lines()
        .map(|line| {
            let line = line.split_once("//").map(|(head, _)| head).unwrap_or(line);
            if line.trim_start().starts_with('#') {
                String::new()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_line_comments(content: &str) -> String {
    content
        .lines()
        .map(|line| line.split_once("//").map(|(head, _)| head).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_c_parameters(params: &str) -> Vec<ParameterSignature> {
    split_top_level(params, ',')
        .into_iter()
        .filter_map(|param| {
            let param = param.trim();
            if param.is_empty() || param == "void" || param == "..." {
                return None;
            }
            let param = param.trim_start_matches("const ").trim();
            let name = Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*(?:\[[^\]]*\])?\s*$")
                .expect("valid C param name regex")
                .captures(param)
                .and_then(|captures| captures.get(1))
                .map(|m| m.as_str().to_string());
            let type_name = if let Some(name) = name.as_deref() {
                let prefix = param
                    .strip_suffix(name)
                    .unwrap_or(param)
                    .trim()
                    .trim_end_matches('*')
                    .trim();
                let pointer_suffix =
                    if param[..param.len().saturating_sub(name.len())].contains('*') {
                        "*"
                    } else {
                        ""
                    };
                format!("{}{}", clean_c_type(prefix), pointer_suffix)
            } else {
                clean_c_type(param)
            };
            Some(ParameterSignature {
                name,
                type_name: Some(type_name),
            })
        })
        .collect()
}

fn parse_zig_parameters(params: &str) -> Vec<ParameterSignature> {
    split_top_level(params, ',')
        .into_iter()
        .filter_map(|param| {
            let param = param.trim();
            if param.is_empty() {
                return None;
            }
            let param = param.strip_prefix("comptime ").unwrap_or(param).trim();
            let (name, type_name) = param.split_once(':')?;
            let type_name = clean_zig_type(type_name);
            Some(ParameterSignature {
                name: Some(name.trim().to_string()),
                type_name: Some(if type_name == "type" {
                    "anytype".to_string()
                } else {
                    type_name
                }),
            })
        })
        .collect()
}

fn parse_c_fields(body: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for field in body.split(';') {
        let field = field.trim();
        if field.is_empty() || field.starts_with('#') {
            continue;
        }
        if let Some((name, type_name)) = parse_c_field(field) {
            fields.insert(name, type_name);
        }
    }
    fields
}

fn parse_c_field(field: &str) -> Option<(String, String)> {
    let captures = Regex::new(r"(?s)(.*?)\s+(\*?[A-Za-z_][A-Za-z0-9_]*)\s*(?:\[[^\]]*\])?$")
        .expect("valid C field regex")
        .captures(field)?;
    let type_name = clean_c_type(captures.get(1)?.as_str());
    let raw_name = captures.get(2)?.as_str().trim();
    let pointer = raw_name.starts_with('*');
    let name = raw_name.trim_start_matches('*').to_string();
    Some((
        name,
        if pointer {
            format!("{}*", type_name)
        } else {
            type_name
        },
    ))
}

fn parse_zig_fields(body: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for field in split_top_level(body, ',') {
        let field = field.trim();
        if field.is_empty() || field.starts_with("pub fn") || field.starts_with("fn ") {
            continue;
        }
        if let Some((name, type_name)) = field.split_once(':') {
            fields.insert(name.trim().to_string(), clean_zig_type(type_name));
        }
    }
    fields
}

fn clean_c_type(type_name: &str) -> String {
    type_name
        .replace('\n', " ")
        .split_whitespace()
        .filter(|part| {
            !matches!(
                *part,
                "static" | "inline" | "extern" | "register" | "__inline" | "__inline__"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn clean_zig_type(type_name: &str) -> String {
    type_name
        .trim()
        .trim_start_matches('!')
        .trim_start_matches('?')
        .trim_end_matches('{')
        .trim()
        .to_string()
}

fn c_type_display_name(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect()
}

fn is_c_control_keyword(name: &str) -> bool {
    matches!(
        name,
        "if" | "for" | "while" | "switch" | "return" | "sizeof" | "do"
    )
}

fn line_char_at(content: &str, byte_offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (idx, ch) in content.char_indices() {
        if idx >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = idx + ch.len_utf8();
        }
    }
    (line, byte_offset.saturating_sub(line_start) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn extracts_rust_function_signature() {
        let signature = extract_signature(
            "pub fn process(input: TestData, count: usize) -> Result<TestData, Error> { todo!() }",
            "rust",
            "process",
        )
        .expect("signature");

        assert_eq!(signature.parameters.len(), 2);
        assert_eq!(signature.parameters[0].name.as_deref(), Some("input"));
        assert_eq!(
            signature.parameters[0].type_name.as_deref(),
            Some("TestData")
        );
        assert_eq!(
            signature.return_type.as_deref(),
            Some("Result<TestData, Error>")
        );
    }

    #[test]
    fn extracts_typescript_function_signature() {
        let signature = extract_signature(
            "export function process(input: TestData): Promise<TestData> { return input; }",
            "ts",
            "process",
        )
        .expect("signature");

        assert_eq!(signature.parameters.len(), 1);
        assert_eq!(signature.parameters[0].name.as_deref(), Some("input"));
        assert_eq!(
            signature.parameters[0].type_name.as_deref(),
            Some("TestData")
        );
        assert_eq!(signature.return_type.as_deref(), Some("Promise<TestData>"));
    }

    #[test]
    fn extracts_rust_struct_fields() {
        let fields = extract_fields(
            "pub struct User { pub id: String, name: Option<String>, age: u32 }",
            "rust",
        );

        assert_eq!(fields.get("id").map(String::as_str), Some("String"));
        assert_eq!(
            fields.get("name").map(String::as_str),
            Some("Option<String>")
        );
        assert_eq!(fields.get("age").map(String::as_str), Some("u32"));
    }

    #[test]
    fn extracts_typescript_interface_fields() {
        let fields = extract_fields(
            "export interface User { id: string; name?: string; age: number; }",
            "ts",
        );

        assert_eq!(fields.get("id").map(String::as_str), Some("string"));
        assert_eq!(fields.get("name").map(String::as_str), Some("string"));
        assert_eq!(fields.get("age").map(String::as_str), Some("number"));
    }

    #[test]
    fn extracts_trait_methods_as_functions() {
        let content = "trait Backend { fn kind(&self) -> String; fn execute(&self, plan: Plan) -> Result<ExecutionResult, BackendError>; }";
        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_rust::language())
            .expect("rust language");
        let tree = parser.parse(content, None).expect("tree");
        let root = tree.root_node();
        let trait_node = root
            .named_children(&mut root.walk())
            .find(|node| node.kind() == "trait_item")
            .expect("trait item");

        let methods = extract_trait_methods(
            Path::new("/tmp"),
            Path::new("/tmp/lib.rs"),
            content,
            &trait_node,
            Some("backend"),
            "Backend",
        );

        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].kind, "Function");
        assert_eq!(methods[0].name, "kind");
        assert!(methods[0].is_method);
        assert_eq!(methods[0].container_name.as_deref(), Some("Backend"));
        assert_eq!(
            methods[1]
                .signature
                .as_ref()
                .and_then(|s| s.return_type.as_deref()),
            Some("Result<ExecutionResult, BackendError>")
        );
    }

    #[test]
    fn extracts_c_functions_and_structs() {
        let content = r#"
            #define SKIP_FN(x) int skipped_##x(void) { return 0; }
            typedef struct amdgpu_ring {
                uint32_t wptr;
                struct amdgpu_device *adev;
            } amdgpu_ring;

            int amdgpu_ring_commit(struct amdgpu_ring *ring, uint32_t count) {
                return 0;
            }
        "#;

        let structs = extract_c_structs(
            Path::new("/tmp/driver.c"),
            &strip_c_preprocessor_and_comments(content),
            Some("driver.c".to_string()),
            Some("driver".to_string()),
        );
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "AmdgpuRing");
        assert_eq!(
            structs[0].fields.get("adev").map(String::as_str),
            Some("struct amdgpu_device*")
        );

        let funcs = extract_c_functions(
            Path::new("/tmp/driver.c"),
            &strip_c_preprocessor_and_comments(content),
            Some("driver.c".to_string()),
            Some("driver".to_string()),
        );
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "amdgpu_ring_commit");
        assert_eq!(
            funcs[0]
                .signature
                .as_ref()
                .and_then(|sig| sig.parameters.first())
                .and_then(|param| param.type_name.as_deref()),
            Some("struct amdgpu_ring*")
        );
    }

    #[test]
    fn extracts_only_zig_native_pub_functions() {
        let content = r#"
            const c = @cImport({ @cInclude("driver.h"); });
            extern fn amdgpu_ring_commit(ring: *AmdgpuRing) c_int;

            pub const AmdgpuRing = struct {
                id: u32,
                name: []u8,
            };

            pub fn executePlan(ring: *AmdgpuRing, comptime T: type) !ExecutionResult {
                _ = ring;
                _ = T;
            }
        "#;

        let funcs = extract_zig_functions(
            Path::new("/tmp/driver.zig"),
            &strip_line_comments(content),
            Some("driver.zig".to_string()),
            Some("driver".to_string()),
        );
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "executePlan");
        assert_eq!(
            funcs[0]
                .signature
                .as_ref()
                .and_then(|sig| sig.parameters.get(1))
                .and_then(|param| param.type_name.as_deref()),
            Some("anytype")
        );

        let types = extract_zig_types(
            Path::new("/tmp/driver.zig"),
            &strip_line_comments(content),
            Some("driver.zig".to_string()),
            Some("driver".to_string()),
        );
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "AmdgpuRing");
        assert_eq!(types[0].fields.get("id").map(String::as_str), Some("u32"));
    }
}

fn map_node_kind_to_symbol_kind(kind: &str) -> String {
    match kind {
        "function_item" | "function_declaration" | "method_definition" | "arrow_function" => {
            "Function".to_string()
        }
        "struct_item" | "struct" => "Struct".to_string(),
        "enum_item" | "enum_declaration" => "Enum".to_string(),
        "class_declaration" => "Class".to_string(),
        "interface_declaration" | "trait_item" => "Interface".to_string(),
        "type_item" | "type_alias_declaration" => "Type".to_string(),
        "lexical_declaration" => "Variable".to_string(), // Often used for const functions
        _ => "Unknown".to_string(),
    }
}
