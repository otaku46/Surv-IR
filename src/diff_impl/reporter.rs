use super::types::{DiffResult, SymbolKind};
use serde_json::json;

/// Format diff result as plain text
pub fn report_text(result: &DiffResult) -> String {
    let mut output = String::new();

    output.push_str("=== Surv IR vs Implementation Diff ===\n\n");

    // Summary
    output.push_str(&format!(
        "Summary: {} matched, {} missing, {} ambiguous, {} extra\n\n",
        result.matched,
        result.missing.len(),
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

    // Ambiguous symbols
    if !result.ambiguous.is_empty() {
        output.push_str("⚠️  Ambiguous (multiple candidates found):\n");
        for (exp, candidates) in &result.ambiguous {
            let kind_icon = match exp.kind {
                SymbolKind::Func => "ƒ",
                SymbolKind::Schema => "T",
            };
            output.push_str(&format!("  {} {} ({} candidates):\n", kind_icon, exp.surv_name, candidates.len()));
            for candidate in candidates {
                output.push_str(&format!("    - {} at {}", candidate.name, format_location(&candidate.uri, &candidate.range)));
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
        output.push_str(&format!("ℹ️  Extra symbols in code (not in IR): {} symbols\n", result.extra.len()));
        output.push_str("  (use --format json for full list)\n\n");
    }

    // Status
    if !result.has_issues() {
        output.push_str("✅ No drift detected! IR and implementation are in sync.\n");
    } else {
        output.push_str("⚠️  Drift detected. Review missing/ambiguous symbols.\n");
    }

    output
}

/// Format diff result as JSON
pub fn report_json(result: &DiffResult) -> String {
    let output = json!({
        "summary": {
            "matched": result.matched,
            "missing": result.missing.len(),
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
        "ambiguous": result.ambiguous.iter().map(|(exp, candidates)| {
            json!({
                "expected": {
                    "name": exp.surv_name,
                    "impl_bind": exp.impl_bind,
                    "impl_lang": exp.impl_lang,
                    "impl_path": exp.impl_path,
                    "kind": format!("{:?}", exp.kind)
                },
                "candidates": candidates.iter().map(|c| {
                    json!({
                        "name": c.name,
                        "kind": c.kind,
                        "uri": c.uri,
                        "range": {
                            "start": { "line": c.range.start_line, "char": c.range.start_char },
                            "end": { "line": c.range.end_line, "char": c.range.end_char }
                        },
                        "container_name": c.container_name
                    })
                }).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>(),
        "extra": result.extra.iter().map(|symbol| {
            json!({
                "name": symbol.name,
                "kind": symbol.kind,
                "uri": symbol.uri,
                "range": {
                    "start": { "line": symbol.range.start_line, "char": symbol.range.start_char },
                    "end": { "line": symbol.range.end_line, "char": symbol.range.end_char }
                },
                "container_name": symbol.container_name
            })
        }).collect::<Vec<_>>()
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
    output.push_str(&format!("- ⚠️  Ambiguous: **{}**\n", result.ambiguous.len()));
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
        output.push_str(&format!("{} symbols in code but not in IR.\n\n", result.extra.len()));
    }

    // Conclusion
    output.push_str("## Status\n\n");
    if !result.has_issues() {
        output.push_str("✅ **No drift detected!** IR and implementation are in sync.\n");
    } else {
        output.push_str("⚠️  **Drift detected.** Review missing/ambiguous symbols above.\n");
    }

    output
}

fn format_location(uri: &str, range: &super::types::SymbolRange) -> String {
    let file_path = uri.strip_prefix("file://").unwrap_or(uri);
    format!("{}:{}:{}", file_path, range.start_line + 1, range.start_char + 1)
}
