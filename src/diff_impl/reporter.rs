use super::types::{
    DedupMode, DesignSkeletonOptions, DiffResult, FoundSymbol, FunctionSignature, SymbolKind,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Format diff result as plain text
pub fn report_text(result: &DiffResult) -> String {
    let mut output = String::new();

    output.push_str("=== Surv IR vs Implementation Diff ===\n\n");

    // Summary
    output.push_str(&format!(
        "Summary: {} matched, {} missing, {} signature mismatch, {} schema field mismatch, {} ambiguous, {} extra\n\n",
        result.matched,
        result.missing.len(),
        result.signature_mismatches.len(),
        result.schema_field_mismatches.len(),
        result.ambiguous.len(),
        result.extra.len()
    ));

    // Missing symbols
    if !result.missing.is_empty() {
        output.push_str("❌ Missing (in IR but not in code):\n");
        for exp in &result.missing {
            let kind_icon = match exp.kind {
                SymbolKind::Func => "ƒ",
                SymbolKind::Schema => "T",
            };
            output.push_str(&format!("  {} {}", kind_icon, exp.surv_name));
            if let Some(bind) = &exp.impl_bind {
                output.push_str(&format!(" (bind: {})", bind));
            }
            if let Some(lang) = &exp.impl_lang {
                output.push_str(&format!(" [lang: {}]", lang));
            }
            if let Some(path) = &exp.impl_path {
                output.push_str(&format!(" @{}", path));
            }
            output.push('\n');
        }
        output.push('\n');
    }

    if !result.schema_field_mismatches.is_empty() {
        output.push_str("Schema field mismatches:\n");
        for mismatch in &result.schema_field_mismatches {
            output.push_str(&format!(
                "  T {} at {}\n",
                mismatch.expected.surv_name,
                format_location(&mismatch.found.uri, &mismatch.found.range)
            ));
            for problem in &mismatch.problems {
                output.push_str(&format!("    - {}\n", problem));
            }
        }
        output.push('\n');
    }

    // Signature mismatches
    if !result.signature_mismatches.is_empty() {
        output.push_str("⚠️  Signature mismatches:\n");
        for mismatch in &result.signature_mismatches {
            output.push_str(&format!(
                "  ƒ {} at {}\n",
                mismatch.expected.surv_name,
                format_location(&mismatch.found.uri, &mismatch.found.range)
            ));
            for problem in &mismatch.problems {
                output.push_str(&format!("    - {}\n", problem));
            }
        }
        output.push('\n');
    }

    // Ambiguous symbols
    if !result.ambiguous.is_empty() {
        output.push_str("⚠️  Ambiguous (multiple candidates found):\n");
        for (exp, candidates) in &result.ambiguous {
            let kind_icon = match exp.kind {
                SymbolKind::Func => "ƒ",
                SymbolKind::Schema => "T",
            };
            output.push_str(&format!(
                "  {} {} ({} candidates):\n",
                kind_icon,
                exp.surv_name,
                candidates.len()
            ));
            for candidate in candidates {
                output.push_str(&format!(
                    "    - {} at {}",
                    candidate.name,
                    format_location(&candidate.uri, &candidate.range)
                ));
                if let Some(container) = &candidate.container_name {
                    output.push_str(&format!(" in {}", container));
                }
                output.push('\n');
            }
        }
        output.push('\n');
    }

    // Extra symbols (optional, can be noisy)
    if !result.extra.is_empty() {
        output.push_str(&format!(
            "ℹ️  Extra symbols in code (not in IR): {} symbols\n",
            result.extra.len()
        ));
        output.push_str("  (use --format json for full list)\n\n");
    }

    // Status
    if !result.has_issues() {
        output.push_str("✅ No drift detected! IR and implementation are in sync.\n");
    } else {
        output
            .push_str("⚠️  Drift detected. Review missing/ambiguous/signature mismatch symbols.\n");
    }

    output
}

/// Format diff result as JSON
pub fn report_json(result: &DiffResult) -> String {
    let output = json!({
        "summary": {
            "matched": result.matched,
            "missing": result.missing.len(),
            "signature_mismatches": result.signature_mismatches.len(),
            "schema_field_mismatches": result.schema_field_mismatches.len(),
            "ambiguous": result.ambiguous.len(),
            "extra": result.extra.len(),
            "has_issues": result.has_issues()
        },
        "missing": result.missing.iter().map(|exp| {
            json!({
                "name": exp.surv_name,
                "impl_bind": exp.impl_bind,
                "impl_lang": exp.impl_lang,
                "impl_path": exp.impl_path,
                "kind": format!("{:?}", exp.kind)
            })
        }).collect::<Vec<_>>(),
        "signature_mismatches": result.signature_mismatches.iter().map(|mismatch| {
            json!({
                "expected": {
                    "name": mismatch.expected.surv_name,
                    "impl_bind": mismatch.expected.impl_bind,
                    "impl_lang": mismatch.expected.impl_lang,
                    "impl_path": mismatch.expected.impl_path,
                    "input": mismatch.expected.input,
                    "output": mismatch.expected.output,
                    "kind": format!("{:?}", mismatch.expected.kind)
                },
                "found": found_symbol_json(&mismatch.found),
                "problems": mismatch.problems,
            })
        }).collect::<Vec<_>>(),
        "schema_field_mismatches": result.schema_field_mismatches.iter().map(|mismatch| {
            json!({
                "expected": {
                    "name": mismatch.expected.surv_name,
                    "impl_bind": mismatch.expected.impl_bind,
                    "impl_lang": mismatch.expected.impl_lang,
                    "impl_path": mismatch.expected.impl_path,
                    "fields": mismatch.expected.fields,
                    "kind": format!("{:?}", mismatch.expected.kind)
                },
                "found": found_symbol_json(&mismatch.found),
                "problems": mismatch.problems,
            })
        }).collect::<Vec<_>>(),
        "ambiguous": result.ambiguous.iter().map(|(exp, candidates)| {
            json!({
                "expected": {
                    "name": exp.surv_name,
                    "impl_bind": exp.impl_bind,
                    "impl_lang": exp.impl_lang,
                    "impl_path": exp.impl_path,
                    "kind": format!("{:?}", exp.kind)
                },
                "candidates": candidates.iter().map(found_symbol_json).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>(),
        "extra": result.extra.iter().map(found_symbol_json).collect::<Vec<_>>()
    });

    serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string())
}

/// Format diff result as Markdown
pub fn report_markdown(result: &DiffResult) -> String {
    let mut output = String::new();

    output.push_str("# Surv IR vs Implementation Diff\n\n");

    // Summary
    output.push_str("## Summary\n\n");
    output.push_str(&format!("- ✅ Matched: **{}**\n", result.matched));
    output.push_str(&format!("- ❌ Missing: **{}**\n", result.missing.len()));
    output.push_str(&format!(
        "- ⚠️  Signature mismatch: **{}**\n",
        result.signature_mismatches.len()
    ));
    output.push_str(&format!(
        "- ⚠️  Schema field mismatch: **{}**\n",
        result.schema_field_mismatches.len()
    ));
    output.push_str(&format!(
        "- ⚠️  Ambiguous: **{}**\n",
        result.ambiguous.len()
    ));
    output.push_str(&format!("- ℹ️  Extra: **{}**\n\n", result.extra.len()));

    // Missing
    if !result.missing.is_empty() {
        output.push_str("## ❌ Missing (in IR but not in code)\n\n");
        output.push_str("| Kind | Name | Binding | Language | Path |\n");
        output.push_str("|------|------|---------|----------|------|\n");
        for exp in &result.missing {
            let kind = match exp.kind {
                SymbolKind::Func => "Function",
                SymbolKind::Schema => "Schema",
            };
            output.push_str(&format!(
                "| {} | `{}` | {} | {} | {} |\n",
                kind,
                exp.surv_name,
                exp.impl_bind.as_deref().unwrap_or("-"),
                exp.impl_lang.as_deref().unwrap_or("both"),
                exp.impl_path.as_deref().unwrap_or("-")
            ));
        }
        output.push('\n');
    }

    if !result.schema_field_mismatches.is_empty() {
        output.push_str("## Schema field mismatches\n\n");
        for mismatch in &result.schema_field_mismatches {
            output.push_str(&format!("### `{}`\n\n", mismatch.expected.surv_name));
            for problem in &mismatch.problems {
                output.push_str(&format!("- {}\n", problem));
            }
            output.push('\n');
        }
    }

    // Signature mismatches
    if !result.signature_mismatches.is_empty() {
        output.push_str("## ⚠️  Signature mismatches\n\n");
        for mismatch in &result.signature_mismatches {
            output.push_str(&format!("### `{}`\n\n", mismatch.expected.surv_name));
            output.push_str(&format!(
                "Implementation: `{}` at {}\n\n",
                mismatch.found.name,
                format_location(&mismatch.found.uri, &mismatch.found.range)
            ));
            for problem in &mismatch.problems {
                output.push_str(&format!("- {}\n", problem));
            }
            output.push('\n');
        }
    }

    // Ambiguous
    if !result.ambiguous.is_empty() {
        output.push_str("## ⚠️  Ambiguous (multiple candidates)\n\n");
        for (exp, candidates) in &result.ambiguous {
            output.push_str(&format!("### `{}`\n\n", exp.surv_name));
            output.push_str(&format!("Found {} candidates:\n\n", candidates.len()));
            for candidate in candidates {
                output.push_str(&format!(
                    "- `{}` ({}) at {}",
                    candidate.name,
                    candidate.kind,
                    format_location(&candidate.uri, &candidate.range)
                ));
                if let Some(container) = &candidate.container_name {
                    output.push_str(&format!(" in `{}`", container));
                }
                output.push('\n');
            }
            output.push('\n');
        }
    }

    // Extra
    if !result.extra.is_empty() {
        output.push_str(&format!("## ℹ️  Extra symbols\n\n"));
        output.push_str(&format!(
            "{} symbols in code but not in IR.\n\n",
            result.extra.len()
        ));
    }

    // Conclusion
    output.push_str("## Status\n\n");
    if !result.has_issues() {
        output.push_str("✅ **No drift detected!** IR and implementation are in sync.\n");
    } else {
        output.push_str(
            "⚠️  **Drift detected.** Review missing/ambiguous/signature mismatch symbols above.\n",
        );
    }

    output
}

fn format_location(uri: &str, range: &super::types::SymbolRange) -> String {
    let file_path = uri.strip_prefix("file://").unwrap_or(uri);
    format!(
        "{}:{}:{}",
        file_path,
        range.start_line + 1,
        range.start_char + 1
    )
}

/// Format diff result as GitHub Actions annotations
pub fn report_github_actions(result: &DiffResult) -> String {
    let mut output = String::new();

    // Missing symbols
    for exp in &result.missing {
        let kind = match exp.kind {
            SymbolKind::Func => "function",
            SymbolKind::Schema => "schema",
        };
        output.push_str(&format!(
            "::error::Missing implementation for {} '{}'\n",
            kind, exp.surv_name
        ));
    }

    // Ambiguous symbols
    for (exp, candidates) in &result.ambiguous {
        let msg = format!(
            "Ambiguous implementation for '{}' ({} candidates found)",
            exp.surv_name,
            candidates.len()
        );
        for candidate in candidates {
            let file_path = candidate
                .uri
                .strip_prefix("file://")
                .unwrap_or(&candidate.uri);
            output.push_str(&format!(
                "::warning file={},line={},col={}::{}\n",
                file_path,
                candidate.range.start_line + 1,
                candidate.range.start_char + 1,
                msg
            ));
        }
    }

    // Signature mismatches
    for mismatch in &result.signature_mismatches {
        let file_path = mismatch
            .found
            .uri
            .strip_prefix("file://")
            .unwrap_or(&mismatch.found.uri);
        let msg = format!(
            "Signature mismatch for '{}': {}",
            mismatch.expected.surv_name,
            mismatch.problems.join("; ")
        );
        output.push_str(&format!(
            "::error file={},line={},col={}::{}\n",
            file_path,
            mismatch.found.range.start_line + 1,
            mismatch.found.range.start_char + 1,
            escape_github_annotation(&msg)
        ));
    }

    for mismatch in &result.schema_field_mismatches {
        let file_path = mismatch
            .found
            .uri
            .strip_prefix("file://")
            .unwrap_or(&mismatch.found.uri);
        let msg = format!(
            "Schema field mismatch for '{}': {}",
            mismatch.expected.surv_name,
            mismatch.problems.join("; ")
        );
        output.push_str(&format!(
            "::error file={},line={},col={}::{}\n",
            file_path,
            mismatch.found.range.start_line + 1,
            mismatch.found.range.start_char + 1,
            escape_github_annotation(&msg)
        ));
    }

    output
}

/// Generate Surv IR function skeletons for extra implementation symbols.
pub fn report_skeletons(result: &DiffResult) -> String {
    let mut output = String::new();

    for symbol in &result.extra {
        if !matches!(symbol.kind.as_str(), "Function" | "Method" | "Variable") {
            continue;
        }

        output.push_str(&format!("[func.{}]\n", sanitize_surv_name(&symbol.name)));
        output.push_str(&format!(
            "intent = \"TODO: describe implementation found at {}\"\n",
            format_location(&symbol.uri, &symbol.range)
        ));

        if let Some(signature) = &symbol.signature {
            let inputs =
                type_refs_from_signature_params(signature, symbol.container_name.as_deref())
                    .into_iter()
                    .map(|type_name| format!("schema.{}", type_name))
                    .collect::<Vec<_>>();

            if !inputs.is_empty() {
                output.push_str(&format!("input = {:?}\n", inputs));
            } else {
                output.push_str("input = []\n");
            }

            let outputs =
                type_refs_from_signature_return(signature, symbol.container_name.as_deref())
                    .into_iter()
                    .map(|type_name| format!("schema.{}", type_name))
                    .collect::<Vec<_>>();
            if !outputs.is_empty() {
                output.push_str(&format!("output = {:?}\n", outputs));
            } else {
                output.push_str("output = []\n");
            }
        } else {
            output.push_str("input = []\n");
            output.push_str("output = []\n");
        }

        output.push_str(&format!(
            "impl.bind = \"{}\"\n",
            escape_toml_string(&symbol.name)
        ));
        output.push('\n');
    }

    output
}

fn found_symbol_json(symbol: &FoundSymbol) -> serde_json::Value {
    json!({
        "language": symbol.language,
        "name": symbol.name,
        "kind": symbol.kind,
        "uri": symbol.uri,
        "relative_path": symbol.relative_path,
        "module_path": symbol.module_path,
        "impl_path": symbol.impl_path,
        "range": {
            "start": { "line": symbol.range.start_line, "char": symbol.range.start_char },
            "end": { "line": symbol.range.end_line, "char": symbol.range.end_char }
        },
        "container_name": symbol.container_name,
        "visibility": symbol.visibility,
        "is_test": symbol.is_test,
        "is_method": symbol.is_method,
        "signature": symbol.signature,
        "fields": symbol.fields
    })
}

/// Generate a fuller Surv IR skeleton from extra implementation symbols.
pub fn report_design_skeleton(result: &DiffResult, options: &DesignSkeletonOptions) -> String {
    let symbols = select_symbols(&result.extra, options);
    let mut output = String::new();

    output.push_str("[meta]\n");
    output.push_str("name = \"implementation_skeleton\"\n");
    output.push_str("version = \"0.1.0\"\n");
    output.push_str("description = \"Generated by surc diff-impl --format design-skeleton\"\n\n");

    if options.emit_schemas {
        let concrete_schema_names = symbols
            .iter()
            .filter(|s| is_schema_symbol(s))
            .filter_map(|symbol| non_empty_surv_name(&schema_name(symbol)))
            .collect::<BTreeSet<_>>();

        for symbol in symbols.iter().filter(|s| is_schema_symbol(s)) {
            output.push_str(&format!(
                "[schema.{}]\n",
                sanitize_surv_name(&schema_name(symbol))
            ));
            output.push_str("kind = \"node\"\n");
            if !symbol.fields.is_empty() {
                let fields = symbol
                    .fields
                    .iter()
                    .map(|(name, type_name)| {
                        format!(
                            "{} = \"{}\"",
                            sanitize_surv_name(name),
                            escape_toml_string(type_name)
                        )
                    })
                    .collect::<Vec<_>>();
                output.push_str(&format!("fields = {{{}}}\n", fields.join(", ")));
            }
            output.push_str(&format!(
                "impl.bind = \"{}\"\n",
                escape_toml_string(&symbol.name)
            ));
            if let Some(lang) = &symbol.language {
                output.push_str(&format!("impl.lang = \"{}\"\n", escape_toml_string(lang)));
            }
            if let Some(path) = &symbol.impl_path {
                output.push_str(&format!("impl.path = \"{}\"\n", escape_toml_string(path)));
            }
            output.push('\n');
        }

        for schema_name in signature_schema_names(&symbols) {
            if concrete_schema_names.contains(&schema_name) {
                continue;
            }
            output.push_str(&format!("[schema.{}]\n", schema_name));
            if is_builtin_scalar_schema_name(&schema_name) {
                output.push_str("kind = \"scalar\"\n");
                output.push_str("role = \"builtin\"\n\n");
            } else {
                output.push_str("kind = \"node\"\n");
                output.push_str("role = \"external\"\n\n");
            }
        }
    }

    if options.emit_funcs {
        for symbol in symbols.iter().filter(|s| is_func_symbol(s)) {
            output.push_str(&format!(
                "[func.{}]\n",
                sanitize_surv_name(&func_name(symbol))
            ));
            output.push_str(&format!(
                "intent = \"TODO: describe implementation found at {}\"\n",
                format_location(&symbol.uri, &symbol.range)
            ));

            let (inputs, outputs) = signature_refs(symbol);
            output.push_str(&format!("input = {:?}\n", inputs));
            output.push_str(&format!("output = {:?}\n", outputs));
            output.push_str(&format!(
                "impl.bind = \"{}\"\n",
                escape_toml_string(&symbol.name)
            ));
            if let Some(lang) = &symbol.language {
                output.push_str(&format!("impl.lang = \"{}\"\n", escape_toml_string(lang)));
            }
            if let Some(path) = &symbol.impl_path {
                output.push_str(&format!("impl.path = \"{}\"\n", escape_toml_string(path)));
            }
            output.push('\n');
        }
    }

    if options.emit_mods {
        let modules = group_by_module(&symbols);
        for (module, module_symbols) in &modules {
            output.push_str(&format!(
                "[mod.implementation.{}]\n",
                sanitize_module_path(module)
            ));
            output.push_str(&format!(
                "purpose = \"TODO: describe implementation module {}\"\n",
                escape_toml_string(module)
            ));

            let schemas: Vec<String> = module_symbols
                .iter()
                .filter(|s| is_schema_symbol(s))
                .map(|s| format!("schema.{}", sanitize_surv_name(&schema_name(s))))
                .collect();
            let funcs: Vec<String> = module_symbols
                .iter()
                .filter(|s| is_func_symbol(s))
                .map(|s| format!("func.{}", sanitize_surv_name(&func_name(s))))
                .collect();

            if !schemas.is_empty() {
                output.push_str(&format!("schemas = {:?}\n", schemas));
            }
            if !funcs.is_empty() {
                output.push_str(&format!("funcs = {:?}\n", funcs));
            }
            output.push('\n');
        }

        let submods: Vec<String> = modules
            .keys()
            .map(|module| sanitize_module_path(module))
            .collect();
        if !submods.is_empty() {
            output.push_str("[mod.implementation]\n");
            output.push_str("purpose = \"TODO: describe implementation root\"\n");
            output.push_str(&format!("submods = {:?}\n\n", submods));
        }
    }

    if options.emit_mapping {
        output.push_str("# Mapping facts\n");
        for symbol in &symbols {
            if let Some(path) = &symbol.impl_path {
                output.push_str("[[mapping.entries]]\n");
                output.push_str(&format!(
                    "stable_id = \"{}\"\n",
                    escape_toml_string(&format!(
                        "{}:{}",
                        symbol.language.as_deref().unwrap_or("unknown"),
                        path
                    ))
                ));
                output.push_str(&format!(
                    "surv_ref = \"{}\"\n",
                    if is_schema_symbol(symbol) {
                        format!("schema.{}", sanitize_surv_name(&schema_name(symbol)))
                    } else {
                        format!("func.{}", sanitize_surv_name(&func_name(symbol)))
                    }
                ));
                output.push_str(&format!("impl_path = \"{}\"\n", escape_toml_string(path)));
                if let Some(file) = &symbol.relative_path {
                    output.push_str(&format!("source_file = \"{}\"\n", escape_toml_string(file)));
                }
                output.push('\n');
            }
        }
    }

    output
}

/// Generate implementation skeleton code for missing IR symbols.
pub fn report_code_skeleton(result: &DiffResult, language: &str) -> String {
    let lang = if language == "ts" || language == "typescript" {
        "ts"
    } else {
        "rust"
    };
    let mut output = String::new();

    for exp in &result.missing {
        match exp.kind {
            SymbolKind::Schema => {
                if lang == "ts" {
                    output.push_str(&typescript_schema_stub(exp));
                } else {
                    output.push_str(&rust_schema_stub(exp));
                }
            }
            SymbolKind::Func => {
                if lang == "ts" {
                    output.push_str(&typescript_func_stub(exp));
                } else {
                    output.push_str(&rust_func_stub(exp));
                }
            }
        }
        output.push('\n');
    }

    output
}

fn rust_schema_stub(exp: &super::types::ExpectedSymbol) -> String {
    let name = rust_type_name(exp.search_name());
    if exp.fields.is_empty() {
        return format!(
            "#[derive(Debug, Clone)]\npub struct {} {{\n    // TODO: add fields\n}}\n",
            name
        );
    }

    let mut output = format!("#[derive(Debug, Clone)]\npub struct {} {{\n", name);
    for (field, type_name) in &exp.fields {
        output.push_str(&format!(
            "    pub {}: {},\n",
            rust_field_name(field),
            rust_type_name(type_name)
        ));
    }
    output.push_str("}\n");
    output
}

fn rust_func_stub(exp: &super::types::ExpectedSymbol) -> String {
    let name = rust_field_name(exp.search_name());
    let params = exp
        .input
        .iter()
        .enumerate()
        .map(|(idx, schema)| {
            format!(
                "input{}: {}",
                idx + 1,
                rust_type_name(schema_name_from_ref(schema))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let return_type = match exp.output.as_slice() {
        [] => "()".to_string(),
        [one] => rust_type_name(schema_name_from_ref(one)),
        many => format!(
            "({})",
            many.iter()
                .map(|schema| rust_type_name(schema_name_from_ref(schema)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };

    format!(
        "pub fn {}({}) -> {} {{\n    todo!(\"implement {}\")\n}}\n",
        name, params, return_type, exp.surv_name
    )
}

fn typescript_schema_stub(exp: &super::types::ExpectedSymbol) -> String {
    let name = ts_type_name(exp.search_name());
    let mut output = format!("export interface {} {{\n", name);
    if exp.fields.is_empty() {
        output.push_str("  // TODO: add fields\n");
    } else {
        for (field, type_name) in &exp.fields {
            output.push_str(&format!("  {}: {};\n", field, ts_type_name(type_name)));
        }
    }
    output.push_str("}\n");
    output
}

fn typescript_func_stub(exp: &super::types::ExpectedSymbol) -> String {
    let name = ts_func_name(exp.search_name());
    let params = exp
        .input
        .iter()
        .enumerate()
        .map(|(idx, schema)| {
            format!(
                "input{}: {}",
                idx + 1,
                ts_type_name(schema_name_from_ref(schema))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let return_type = match exp.output.as_slice() {
        [] => "void".to_string(),
        [one] => ts_type_name(schema_name_from_ref(one)),
        many => format!(
            "[{}]",
            many.iter()
                .map(|schema| ts_type_name(schema_name_from_ref(schema)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };

    format!(
        "export function {}({}): {} {{\n  throw new Error(\"TODO: implement {}\");\n}}\n",
        name, params, return_type, exp.surv_name
    )
}

fn schema_name_from_ref(value: &str) -> &str {
    value
        .strip_prefix("schema.")
        .or_else(|| value.rsplit_once(".schema.").map(|(_, name)| name))
        .unwrap_or(value)
}

fn rust_type_name(value: &str) -> String {
    let value = schema_name_from_ref(value);
    match value {
        "string" => "String".to_string(),
        "bool" => "bool".to_string(),
        "number" => "f64".to_string(),
        "unit" => "()".to_string(),
        other => to_pascal_identifier(other),
    }
}

fn ts_type_name(value: &str) -> String {
    let value = schema_name_from_ref(value);
    match value {
        "string" => "string".to_string(),
        "bool" => "boolean".to_string(),
        "number" => "number".to_string(),
        "unit" => "void".to_string(),
        other => to_pascal_identifier(other),
    }
}

fn rust_field_name(value: &str) -> String {
    to_snake_identifier(value)
}

fn ts_func_name(value: &str) -> String {
    to_camel_identifier(value)
}

fn to_pascal_identifier(value: &str) -> String {
    let mut out = String::new();
    for part in identifier_parts(value) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    if out.is_empty() {
        "GeneratedType".to_string()
    } else {
        out
    }
}

fn to_camel_identifier(value: &str) -> String {
    let pascal = to_pascal_identifier(value);
    let mut chars = pascal.chars();
    if let Some(first) = chars.next() {
        format!(
            "{}{}",
            first.to_ascii_lowercase(),
            chars.collect::<String>()
        )
    } else {
        "generatedFunction".to_string()
    }
}

fn to_snake_identifier(value: &str) -> String {
    let parts = identifier_parts(value);
    if parts.is_empty() {
        "generated_field".to_string()
    } else {
        parts.join("_").to_ascii_lowercase()
    }
}

fn identifier_parts(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn select_symbols<'a>(
    symbols: &'a [FoundSymbol],
    options: &DesignSkeletonOptions,
) -> Vec<&'a FoundSymbol> {
    let mut selected = Vec::new();
    let mut seen = HashMap::new();

    for symbol in symbols {
        if options.exclude_tests && symbol.is_test {
            continue;
        }
        if !is_schema_symbol(symbol) && !is_func_symbol(symbol) {
            continue;
        }

        let key = match options.dedup {
            DedupMode::None => None,
            DedupMode::Name => Some(format!(
                "{}:{}:{}",
                symbol.kind,
                symbol.name,
                symbol
                    .signature
                    .as_ref()
                    .map(signature_fingerprint)
                    .unwrap_or_default()
            )),
            DedupMode::Path => Some(dedup_key(symbol)),
        };

        if let Some(key) = key {
            match seen.get(&key).copied() {
                None => {
                    seen.insert(key, selected.len());
                    selected.push(symbol);
                }
                Some(idx) => {
                    if symbol_score(symbol) > symbol_score(selected[idx]) {
                        selected[idx] = symbol;
                    }
                }
            }
        } else {
            selected.push(symbol);
        }
    }

    selected
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
        parts.push(signature_fingerprint(signature));
    } else if !symbol.fields.is_empty() {
        parts.push(fields_fingerprint(&symbol.fields));
    } else {
        parts.push(format!(
            "{}:{}-{}:{}",
            symbol.range.start_line,
            symbol.range.start_char,
            symbol.range.end_line,
            symbol.range.end_char
        ));
        if let Some(container) = &symbol.container_name {
            parts.push(container.clone());
        }
    }

    parts.join("|")
}

fn signature_fingerprint(signature: &FunctionSignature) -> String {
    let params = signature
        .parameters
        .iter()
        .map(|param| param.type_name.as_deref().unwrap_or("_").to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "({})->{}",
        params,
        signature.return_type.as_deref().unwrap_or("_")
    )
}

fn fields_fingerprint(fields: &BTreeMap<String, String>) -> String {
    fields
        .iter()
        .map(|(name, value)| format!("{}={}", name, value))
        .collect::<Vec<_>>()
        .join(",")
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

fn group_by_module<'a>(symbols: &[&'a FoundSymbol]) -> BTreeMap<String, Vec<&'a FoundSymbol>> {
    let mut modules: BTreeMap<String, Vec<&FoundSymbol>> = BTreeMap::new();
    for symbol in symbols {
        let module = symbol
            .module_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .unwrap_or("root")
            .to_string();
        modules.entry(module).or_default().push(*symbol);
    }
    modules
}

fn is_schema_symbol(symbol: &FoundSymbol) -> bool {
    matches!(
        symbol.kind.as_str(),
        "Interface" | "Class" | "Struct" | "Enum" | "Type"
    )
}

fn is_func_symbol(symbol: &FoundSymbol) -> bool {
    matches!(symbol.kind.as_str(), "Function" | "Method" | "Variable")
}

fn schema_name(symbol: &FoundSymbol) -> String {
    symbol.name.clone()
}

fn func_name(symbol: &FoundSymbol) -> String {
    let mut parts = Vec::new();
    if let Some(module) = &symbol.module_path {
        if !module.is_empty() && module != "root" {
            parts.extend(module.split("::").map(str::to_string));
        }
    }
    if let Some(container) = &symbol.container_name {
        if !container.is_empty() {
            parts.push(container.clone());
        }
    }
    parts.push(symbol.name.clone());
    parts.join("_")
}

fn signature_refs(symbol: &FoundSymbol) -> (Vec<String>, Vec<String>) {
    let Some(signature) = &symbol.signature else {
        return (Vec::new(), Vec::new());
    };

    let owner = symbol
        .container_name
        .as_deref()
        .or(Some(symbol.name.as_str()));

    let inputs = type_refs_from_signature_params(signature, owner)
        .into_iter()
        .map(|type_name| format!("schema.{}", type_name))
        .collect();
    let outputs = type_refs_from_signature_return(signature, owner)
        .into_iter()
        .map(|type_name| format!("schema.{}", type_name))
        .collect();

    (inputs, outputs)
}

fn signature_schema_names(symbols: &[&FoundSymbol]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for symbol in symbols.iter().filter(|s| is_func_symbol(s)) {
        let (inputs, outputs) = signature_refs(symbol);
        for schema_ref in inputs.into_iter().chain(outputs) {
            if let Some(name) = schema_ref.strip_prefix("schema.") {
                names.insert(name.to_string());
            }
        }
    }
    names
}

fn type_refs_from_signature_params(
    signature: &FunctionSignature,
    owner: Option<&str>,
) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    for param in &signature.parameters {
        if let Some(type_name) = param.type_name.as_deref() {
            refs.extend(type_ref_names(type_name, owner));
        }
    }
    refs
}

fn type_refs_from_signature_return(
    signature: &FunctionSignature,
    owner: Option<&str>,
) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    if let Some(return_type) = signature.return_type.as_deref() {
        refs.extend(type_ref_names(return_type, owner));
    }
    refs
}

fn non_empty_surv_name(value: &str) -> Option<String> {
    let name = sanitize_surv_name(value);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn type_ref_names(type_name: &str, owner: Option<&str>) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    for leaf in collect_type_leaves(type_name, owner) {
        if let Some(name) = non_empty_surv_name(&leaf) {
            refs.insert(name);
        }
    }
    refs
}

fn collect_type_leaves(type_name: &str, owner: Option<&str>) -> Vec<String> {
    let is_pointer_like = is_pointer_like_type(type_name);
    let trimmed = strip_type_wrappers(type_name);
    if let Some((base, inner)) = split_type_generics(&trimmed) {
        let mut leaves = Vec::new();
        for inner_type in split_top_level_types(&inner, ',') {
            leaves.extend(collect_type_leaves(&inner_type, owner));
        }
        if leaves.is_empty() {
            let base = normalize_type_segment(base, owner);
            if !base.is_empty() {
                leaves.push(base);
            }
        }
        leaves
    } else {
        let base = normalize_type_segment(&trimmed, owner);
        if base.is_empty() {
            Vec::new()
        } else if is_pointer_like && !is_builtin_scalar_schema_name(&base) {
            vec![format!("{}Handle", base)]
        } else {
            vec![base]
        }
    }
}

fn strip_type_wrappers(type_name: &str) -> String {
    let mut value = type_name
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim_start_matches("dyn ")
        .trim_start_matches("impl ")
        .trim_start_matches("const ")
        .trim_start_matches("volatile ")
        .trim()
        .trim_matches(|ch| matches!(ch, '(' | ')' | '[' | ']'))
        .to_string();

    while value.starts_with('*') {
        value = value[1..].trim_start().to_string();
        value = value.trim_start_matches("const ").trim_start().to_string();
    }
    while value.ends_with('*') {
        value.pop();
        value = value.trim_end().to_string();
    }
    if let Some(stripped) = value.strip_prefix("struct ") {
        value = stripped.trim().to_string();
    }
    value
}

fn split_type_generics(type_name: &str) -> Option<(&str, String)> {
    let open = type_name.find('<')?;
    let inner = extract_balanced_text(type_name, open, '<', '>')?;
    Some((type_name[..open].trim(), inner))
}

fn split_top_level_types(input: &str, separator: char) -> Vec<String> {
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

fn extract_balanced_text(text: &str, open: usize, open_ch: char, close_ch: char) -> Option<String> {
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

fn normalize_type_segment(value: &str, owner: Option<&str>) -> String {
    let value = value
        .split("::")
        .last()
        .unwrap_or(value)
        .split('.')
        .last()
        .unwrap_or(value)
        .trim()
        .trim_end_matches('?')
        .trim_end_matches(',')
        .trim_end_matches(')')
        .trim_end_matches(']')
        .trim_end_matches('>')
        .trim_end_matches('*')
        .trim();

    if matches!(value, "Self" | "self") {
        return owner.map(sanitize_surv_name).unwrap_or_default();
    }

    if let Some(builtin) = builtin_scalar_name(value) {
        return builtin.to_string();
    }

    let cleaned = if value.contains('_') {
        to_pascal_identifier(value)
    } else {
        sanitize_surv_name(value)
    };
    if cleaned.is_empty() {
        value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect()
    } else {
        cleaned
    }
}

fn builtin_scalar_name(value: &str) -> Option<&'static str> {
    match value {
        "String" | "str" | "[]u8" | "[:0]u8" => Some("string"),
        "bool" | "bool_t" => Some("bool"),
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

fn is_builtin_scalar_schema_name(value: &str) -> bool {
    matches!(value, "string" | "bool" | "number" | "unit" | "anytype")
}

fn is_pointer_like_type(type_name: &str) -> bool {
    let trimmed = type_name.trim();
    trimmed.starts_with('&')
        || trimmed.starts_with('*')
        || trimmed.ends_with('*')
        || trimmed.contains(" *")
}

fn escape_github_annotation(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn sanitize_surv_name(value: &str) -> String {
    let trimmed = value
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim();
    let mut out = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if matches!(
            ch,
            ':' | '<' | '>' | '[' | ']' | '(' | ')' | ',' | ' ' | '&'
        ) {
            if !out.ends_with('_') {
                out.push('_');
            }
        }
    }
    out.trim_matches('_').to_string()
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn sanitize_module_path(value: &str) -> String {
    value
        .split("::")
        .map(sanitize_surv_name)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_impl::types::FunctionSignature;

    #[test]
    fn flattens_wrapper_types_into_leaf_schema_refs() {
        let leaves = collect_type_leaves("Result<Arc<ExecutionResult>, BackendError>", None);
        assert!(leaves.contains(&"ExecutionResult".to_string()));
        assert!(leaves.contains(&"BackendError".to_string()));

        let leaves = collect_type_leaves("Arc<HashMap<ComputeId, Vec<f32>>>", None);
        assert!(leaves.contains(&"ComputeId".to_string()));
        assert!(leaves.contains(&"number".to_string()));
    }

    #[test]
    fn signature_ref_generation_uses_leaf_types() {
        let signature = FunctionSignature {
            parameters: vec![
                crate::diff_impl::types::ParameterSignature {
                    name: Some("input".to_string()),
                    type_name: Some("Arc<TestData>".to_string()),
                },
                crate::diff_impl::types::ParameterSignature {
                    name: Some("deps".to_string()),
                    type_name: Some("HashMap<ComputeId, Vec<f32>>".to_string()),
                },
            ],
            return_type: Some("Result<Self, BackendError>".to_string()),
        };

        let params = type_refs_from_signature_params(&signature, Some("Backend"));
        assert!(params.contains("TestData"));
        assert!(params.contains("ComputeId"));
        assert!(params.contains("number"));

        let returns = type_refs_from_signature_return(&signature, Some("Backend"));
        assert!(returns.contains("Backend"));
        assert!(returns.contains("BackendError"));
    }

    #[test]
    fn maps_pointer_like_types_to_handles() {
        let leaves = collect_type_leaves("struct amdgpu_ring*", None);
        assert_eq!(leaves, vec!["AmdgpuRingHandle".to_string()]);

        let leaves = collect_type_leaves("*AmdgpuRing", None);
        assert_eq!(leaves, vec!["AmdgpuRingHandle".to_string()]);

        let leaves = collect_type_leaves("&mut AmdgpuRing", None);
        assert_eq!(leaves, vec!["AmdgpuRingHandle".to_string()]);

        let leaves = collect_type_leaves("char*", None);
        assert_eq!(leaves, vec!["string".to_string()]);
    }
}
