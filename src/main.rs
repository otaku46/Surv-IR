use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use survibe_parser_rs::{
    check_deploy_file, check_project, check_surv_file, load_project, parse_deploy_file,
    parse_surv_file, HtmlExporter, MermaidExporter, ProjectAST, Section,
};
use survibe_parser_rs::codegen::{GitHubActionsGenerator, GitLabCIGenerator};

mod status_commands;
mod deps_commands;
mod split_commands;
mod query_commands;
use status_commands::run_status;
use deps_commands::run_deps;
use split_commands::run_split;
use query_commands::{run_refs, run_slice, run_trace};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "--help" | "-h" | "help" => {
            print_usage();
            Ok(())
        }
        "parse" => {
            if args.len() >= 3 && (args[2] == "--help" || args[2] == "-h") {
                eprintln!("Usage: surc parse <file.toml>");
                eprintln!();
                eprintln!("Parse a Surv IR file and output its AST as JSON.");
                eprintln!();
                eprintln!("Arguments:");
                eprintln!("  <file.toml>    Path to a single Surv IR file");
                return Ok(());
            }
            if args.len() < 3 {
                eprintln!("Usage: surc parse <file.toml>");
                eprintln!();
                eprintln!("Parse a Surv IR file and output its AST as JSON.");
                std::process::exit(1);
            }
            run_parse(&args[2])
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("Usage: surc check <file.toml>");
                std::process::exit(1);
            }
            run_check(&args[2])
        }
        "project-check" => {
            if args.len() < 3 {
                eprintln!("Usage: surc project-check <surv.toml>");
                std::process::exit(1);
            }
            run_project_check(&args[2])
        }
        "deploy-check" => {
            if args.len() < 3 {
                eprintln!("Usage: surc deploy-check <deploy.toml>");
                std::process::exit(1);
            }
            run_deploy_check(&args[2])
        }
        "export" => {
            if args.len() < 3 {
                eprintln!("Usage: surc export <type> <file>");
                eprintln!("Types: pipeline, modules, schemas, module-detail, deploy-mermaid, deploy-html");
                std::process::exit(1);
            }
            run_export(&args[2..])
        }
        "codegen" => {
            if args.len() < 3 {
                eprintln!("Usage: surc codegen <platform> <deploy.toml>");
                eprintln!("Platforms: github-actions, gitlab-ci");
                std::process::exit(1);
            }
            run_codegen(&args[2..])
        }
        "inspect" => {
            if args.len() < 4 {
                eprintln!("Usage: surc inspect <module-name> <file.toml>");
                eprintln!();
                eprintln!("Inspect a module and show its schemas, functions, and pipeline.");
                eprintln!();
                eprintln!("Example: surc inspect mod.todo_api examples/todo_api.toml");
                std::process::exit(1);
            }
            run_inspect(&args[2], &args[3])
        }
        "status" => {
            run_status(&args[2..])
        }
        "deps" => {
            if args.len() < 3 {
                eprintln!("Usage: surc deps <surv.toml> [options]");
                std::process::exit(1);
            }
            run_deps(&args[2..])
        }
        "split" => {
            if args.len() < 3 {
                eprintln!("Usage: surc split <input.toml> --config <split_config.toml>");
                std::process::exit(1);
            }
            run_split(&args[2..])
        }
        "slice" => {
            run_slice(&args[2..])
        }
        "refs" => {
            run_refs(&args[2..])
        }
        "trace" => {
            run_trace(&args[2..])
        }
        "diff-impl" => {
            if args.len() < 4 {
                print_diff_impl_usage();
                std::process::exit(1);
            }
            run_diff_impl(&args[2..])
        }
        "-" => run_parse_reader(io::stdin()),
        other => {
            if args.len() == 2 {
                run_parse(other)
            } else {
                print_usage();
                std::process::exit(1);
            }
        }
    }
}

fn print_usage() {
    eprintln!("Usage: surc <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  parse <file>                Parse IR and output AST as JSON");
    eprintln!("  check <file>                Run static analysis for a single file");
    eprintln!("  project-check <manifest>    Check a project manifest (surv.toml)");
    eprintln!("  deploy-check <file>         Check a deploy IR file");
    eprintln!("  inspect <module> <file>     Inspect a module's schemas, funcs, and pipeline");
    eprintln!("  status <subcommand>         Manage implementation status");
    eprintln!("  deps <manifest>             Show package and module dependencies");
    eprintln!("  split <input> --config <c>  Split single IR file into multi-package project");
    eprintln!("  slice <target> <file>       Slice minimal IR fragment for a target");
    eprintln!("  refs <target> <file>        List references to a symbol");
    eprintln!("  trace <target> <file>       Trace pipeline flow for a func or mod");
    eprintln!("  diff-impl <ir> <workspace>  Detect drift between IR and implementation");
    eprintln!("  export <type> <file>        Export visualizations");
    eprintln!("  codegen <platform> <file>   Generate CI/CD configuration");
    eprintln!();
    eprintln!("Export types:");
    eprintln!("  pipeline <file> <mod>       Export module pipeline as Mermaid");
    eprintln!("  modules <manifest>          Export module dependency graph");
    eprintln!("  schemas <manifest>          Export schema relationship graph");
    eprintln!("  html <manifest>             Export interactive HTML visualization");
    eprintln!("  module-detail <file> <mod>  Export detailed module view");
    eprintln!("  deploy-mermaid <file>       Export deploy pipeline as Mermaid");
    eprintln!("  deploy-html <file>          Export deploy pipeline as interactive HTML");
    eprintln!();
    eprintln!("Codegen platforms:");
    eprintln!("  github-actions              Generate GitHub Actions workflow");
    eprintln!("  gitlab-ci                   Generate GitLab CI configuration");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  surc parse example.toml");
    eprintln!("  surc check example.toml");
    eprintln!("  surc project-check surv.toml");
    eprintln!("  surc inspect mod.user_api example.toml");
    eprintln!("  surc export pipeline example.toml user_api");
    eprintln!("  surc export modules surv.toml");
    eprintln!("  surc export html surv.toml > output.html");
    eprintln!("  surc deploy-check deploy.toml");
    eprintln!("  surc codegen github-actions deploy.toml > .github/workflows/deploy.yml");
}

fn print_export_usage() {
    eprintln!("Usage: surc export <type> <file> [args...]");
    eprintln!();
    eprintln!("Export visualizations and diagrams from Surv IR or Deploy IR files.");
    eprintln!();
    eprintln!("SURV IR EXPORTS (single file or manifest):");
    eprintln!();
    eprintln!("  pipeline <file.toml> <module-name>");
    eprintln!("      Export a module's pipeline as Mermaid flowchart");
    eprintln!("      Input: Single Surv IR file");
    eprintln!("      Example: surc export pipeline user_api.toml user_http_api");
    eprintln!();
    eprintln!("  module-detail <file.toml> <module-name>");
    eprintln!("      Export detailed module view as Mermaid");
    eprintln!("      Input: Single Surv IR file");
    eprintln!("      Example: surc export module-detail user_api.toml user_http_api");
    eprintln!();
    eprintln!("  modules <surv.toml>");
    eprintln!("      Export module dependency graph as Mermaid");
    eprintln!("      Input: Project manifest (surv.toml)");
    eprintln!("      Example: surc export modules surv.toml");
    eprintln!();
    eprintln!("  schemas <surv.toml>");
    eprintln!("      Export schema relationship graph as Mermaid");
    eprintln!("      Input: Project manifest (surv.toml)");
    eprintln!("      Example: surc export schemas surv.toml");
    eprintln!();
    eprintln!("  html <surv.toml>");
    eprintln!("      Export interactive HTML visualization (D3.js)");
    eprintln!("      Input: Project manifest (surv.toml)");
    eprintln!("      Example: surc export html surv.toml > output.html");
    eprintln!();
    eprintln!("DEPLOY IR EXPORTS:");
    eprintln!();
    eprintln!("  deploy-mermaid <deploy.toml>");
    eprintln!("      Export deployment pipeline as Mermaid flowchart");
    eprintln!("      Input: Deploy IR file");
    eprintln!("      Example: surc export deploy-mermaid deploy.toml");
    eprintln!();
    eprintln!("  deploy-html <deploy.toml>");
    eprintln!("      Export deployment pipeline as interactive HTML");
    eprintln!("      Input: Deploy IR file");
    eprintln!("      Example: surc export deploy-html deploy.toml > pipeline.html");
    eprintln!();
    eprintln!("Note: Use 'surv.toml' for project-level exports (modules, schemas, html)");
    eprintln!("      Use individual '.toml' files for single-file exports (pipeline, module-detail)");
}

fn run_parse(filename: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(filename)?;
    run_parse_reader(file)
}

fn run_parse_reader<R: Read>(reader: R) -> Result<(), Box<dyn Error>> {
    let ast = parse_surv_file(reader)?;
    let mut stdout = io::stdout();
    serde_json::to_writer_pretty(&mut stdout, &ast)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn run_check(filename: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(filename)?;
    let ast = parse_surv_file(file)?;
    let diags = check_surv_file(&ast);

    if diags.is_empty() {
        println!("✓ No issues found");
        return Ok(());
    }

    let mut error_count = 0;
    let mut warning_count = 0;

    for diag in &diags {
        let icon = match diag.severity.as_str() {
            "error" => {
                error_count += 1;
                "✗"
            }
            "warning" => {
                warning_count += 1;
                "⚠"
            }
            _ => "?",
        };
        println!("{icon} [{}] {}", diag.kind, diag.message);
        println!("  at {}\n", diag.location);
    }

    println!("---");
    println!("{error_count} error(s), {warning_count} warning(s)");

    if error_count > 0 {
        Err("diagnostics reported errors".into())
    } else {
        Ok(())
    }
}

fn run_project_check(manifest: &str) -> Result<(), Box<dyn Error>> {
    let project = load_project(Path::new(manifest))?;
    let diags = check_project(&project);

    if diags.is_empty() {
        println!("✓ No project issues found");
        return Ok(());
    }

    let mut error_count = 0;

    for diag in &diags {
        if diag.severity == "error" {
            error_count += 1;
        }
        let icon = match diag.severity.as_str() {
            "error" => "✗",
            "warning" => "⚠",
            _ => "?",
        };
        println!("{icon} [{}] {}", diag.kind, diag.message);
        if !diag.location.is_empty() {
            println!("  at {}\n", diag.location);
        } else {
            println!();
        }
    }

    println!("---");
    println!("{error_count} error(s)");

    if error_count > 0 {
        Err("project diagnostics reported errors".into())
    } else {
        Ok(())
    }
}

fn run_deploy_check(filename: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(filename)?;
    let deploy = parse_deploy_file(file)?;
    let diags = check_deploy_file(&deploy);

    if diags.is_empty() {
        println!("✓ No deploy issues found");
        return Ok(());
    }

    let mut error_count = 0;
    let mut warning_count = 0;

    for diag in &diags {
        let icon = match diag.severity.as_str() {
            "error" => {
                error_count += 1;
                "✗"
            }
            "warning" => {
                warning_count += 1;
                "⚠"
            }
            _ => "ℹ",
        };
        println!("{icon} [{}] {}", diag.kind, diag.message);
        println!("  at {}\n", diag.location);
    }

    println!("---");
    println!("{error_count} error(s), {warning_count} warning(s)");

    if error_count > 0 {
        Err("deploy diagnostics reported errors".into())
    } else {
        Ok(())
    }
}

fn run_export(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_export_usage();
        if args.is_empty() {
            std::process::exit(1);
        } else {
            return Ok(());
        }
    }

    let export_type = &args[0];
    let exporter = MermaidExporter::new();

    match export_type.as_str() {
        "pipeline" => {
            if args.len() < 3 {
                eprintln!("Usage: surc export pipeline <file> <module-name>");
                std::process::exit(1);
            }
            let file = File::open(&args[1])?;
            let ast = parse_surv_file(file)?;
            let module_name = args[2].clone();
            let file_path = args[1].clone();

            let project = ProjectAST::from_files(vec![(file_path.into(), ast)]);

            let module = project.files[0]
                .1
                .sections
                .iter()
                .find_map(|s| match s {
                    Section::Mod(m) if m.name == module_name => Some(m),
                    _ => None,
                })
                .ok_or_else(|| format!("Module '{}' not found", module_name))?;

            let output = exporter.export_pipeline(module, &project);
            println!("{}", output);
        }
        "modules" => {
            if args.len() < 2 {
                eprintln!("Usage: surc export modules <surv.toml>");
                eprintln!();
                eprintln!("Error: Missing manifest file");
                eprintln!("Expected: A project manifest file (surv.toml) with [project] section");
                std::process::exit(1);
            }
            let project = load_project(Path::new(&args[1])).map_err(|e| {
                format!("Failed to load project manifest '{}':\n  {}\n\nExpected: A surv.toml file with [project] and [files] sections", args[1], e)
            })?;
            let output = exporter.export_module_dependencies(&project);
            println!("{}", output);
        }
        "schemas" => {
            if args.len() < 2 {
                eprintln!("Usage: surc export schemas <surv.toml>");
                eprintln!();
                eprintln!("Error: Missing manifest file");
                eprintln!("Expected: A project manifest file (surv.toml) with [project] section");
                std::process::exit(1);
            }
            let project = load_project(Path::new(&args[1])).map_err(|e| {
                format!("Failed to load project manifest '{}':\n  {}\n\nExpected: A surv.toml file with [project] and [files] sections", args[1], e)
            })?;
            let output = exporter.export_schema_graph(&project);
            println!("{}", output);
        }
        "html" => {
            if args.len() < 2 {
                eprintln!("Usage: surc export html <surv.toml>");
                eprintln!();
                eprintln!("Error: Missing manifest file");
                eprintln!("Expected: A project manifest file (surv.toml) with [project] section");
                eprintln!();
                eprintln!("Example surv.toml:");
                eprintln!("  [project]");
                eprintln!("  name = \"my-project\"");
                eprintln!();
                eprintln!("  [files]");
                eprintln!("  \"api.toml\" = {{}}");
                std::process::exit(1);
            }
            let project = load_project(Path::new(&args[1])).map_err(|e| {
                format!("Failed to load project manifest '{}':\n  {}\n\nExpected: A surv.toml file with [project] and [files] sections.\n\nIf you have a single Surv IR file, use 'surc export pipeline <file> <module>' instead.", args[1], e)
            })?;
            let html_exporter = HtmlExporter::new();
            let output = html_exporter.export_interactive(&project);
            println!("{}", output);
        }
        "module-detail" => {
            if args.len() < 3 {
                eprintln!("Usage: surc export module-detail <file> <module-name>");
                std::process::exit(1);
            }
            let file = File::open(&args[1])?;
            let ast = parse_surv_file(file)?;
            let module_name = args[2].clone();
            let file_path = args[1].clone();

            let project = ProjectAST::from_files(vec![(file_path.into(), ast)]);

            let module = project.files[0]
                .1
                .sections
                .iter()
                .find_map(|s| match s {
                    Section::Mod(m) if m.name == module_name => Some(m),
                    _ => None,
                })
                .ok_or_else(|| format!("Module '{}' not found", module_name))?;

            let output = exporter.export_module_detail(module, &project);
            println!("{}", output);
        }
        "deploy-mermaid" => {
            if args.len() < 2 {
                eprintln!("Usage: surc export deploy-mermaid <deploy.toml>");
                std::process::exit(1);
            }
            let file = File::open(&args[1])?;
            let deploy = parse_deploy_file(file)?;
            let output = exporter.export_deploy_pipeline(&deploy);
            println!("{}", output);
        }
        "deploy-html" => {
            if args.len() < 2 {
                eprintln!("Usage: surc export deploy-html <deploy.toml>");
                std::process::exit(1);
            }
            let file = File::open(&args[1])?;
            let deploy = parse_deploy_file(file)?;
            let html_exporter = HtmlExporter::new();
            let output = html_exporter.export_deploy_interactive(&deploy);
            println!("{}", output);
        }
        other => {
            eprintln!("Unknown export type: {}", other);
            eprintln!("Valid types: pipeline, modules, schemas, html, module-detail, deploy-mermaid, deploy-html");
            std::process::exit(1);
        }
    }

    Ok(())
}

fn run_codegen(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() {
        eprintln!("Usage: surc codegen <platform> <deploy.toml>");
        std::process::exit(1);
    }

    let platform = &args[0];

    match platform.as_str() {
        "github-actions" => {
            if args.len() < 2 {
                eprintln!("Usage: surc codegen github-actions <deploy.toml>");
                std::process::exit(1);
            }
            let file = File::open(&args[1])?;
            let deploy = parse_deploy_file(file)?;
            let generator = GitHubActionsGenerator::new();
            let yaml = generator.generate(&deploy);
            println!("{}", yaml);
        }
        "gitlab-ci" => {
            if args.len() < 2 {
                eprintln!("Usage: surc codegen gitlab-ci <deploy.toml>");
                std::process::exit(1);
            }
            let file = File::open(&args[1])?;
            let deploy = parse_deploy_file(file)?;
            let generator = GitLabCIGenerator::new();
            let yaml = generator.generate(&deploy);
            println!("{}", yaml);
        }
        other => {
            eprintln!("Unknown platform: {}", other);
            eprintln!("Valid platforms: github-actions, gitlab-ci");
            std::process::exit(1);
        }
    }

    Ok(())
}

fn run_inspect(module_name: &str, filename: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(filename)?;
    let parsed = parse_surv_file(file)?;

    // Strip "mod." prefix if present
    let module_name = module_name.strip_prefix("mod.").unwrap_or(module_name);

    // Find the module
    let module = parsed.sections.iter().find_map(|section| {
        if let Section::Mod(m) = section {
            if m.name == module_name {
                Some(m)
            } else {
                None
            }
        } else {
            None
        }
    });

    let module = match module {
        Some(m) => m,
        None => {
            eprintln!("Error: Module 'mod.{}' not found in {}", module_name, filename);
            eprintln!();
            eprintln!("Available modules:");
            for section in &parsed.sections {
                if let Section::Mod(m) = section {
                    eprintln!("  - mod.{}", m.name);
                }
            }
            std::process::exit(1);
        }
    };

    // Print module information
    println!("Module: mod.{}", module.name);
    println!();

    if !module.purpose.is_empty() {
        println!("Purpose: {}", module.purpose);
        println!();
    }

    // Print schemas
    if !module.schemas.is_empty() {
        println!("Schemas ({}):", module.schemas.len());
        for schema in &module.schemas {
            println!("  - {}", schema);
        }
        println!();
    }

    // Print functions
    if !module.funcs.is_empty() {
        println!("Functions ({}):", module.funcs.len());
        for func in &module.funcs {
            println!("  - {}", func);
        }
        println!();
    }

    // Print pipeline
    if !module.pipeline.is_empty() {
        println!("Pipeline ({} steps):", module.pipeline.len());
        for (i, step) in module.pipeline.iter().enumerate() {
            if i == 0 {
                println!("  {}", step);
            } else {
                println!("    ↓");
                println!("  {}", step);
            }
        }
        println!();
    }

    // Find and print status information
    let status = parsed.sections.iter().find_map(|section| {
        if let Section::Status(s) = section {
            Some(s)
        } else {
            None
        }
    });

    if let Some(status_section) = status {
        if let Some(module_status) = status_section.modules.get(module_name) {
            println!("Status:");
            if !module_status.state.is_empty() {
                println!("  State: {}", module_status.state);
            }
            if module_status.coverage > 0.0 {
                println!("  Coverage: {:.0}%", module_status.coverage * 100.0);
            }
            if !module_status.notes.is_empty() {
                println!("  Notes: {}", module_status.notes);
            }
            if !status_section.updated_at.is_empty() {
                println!("  Updated: {}", status_section.updated_at);
            }
            println!();
        }
    }

    Ok(())
}


fn print_diff_impl_usage() {
    eprintln!("Usage: surc diff-impl <design.toml> <workspace_root> [options]");
    eprintln!();
    eprintln!("Detect drift between Surv IR specifications and actual implementation.");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  <design.toml>     Path to Surv IR file");
    eprintln!("  <workspace_root>  Path to codebase root directory");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --mod <module>    Filter to specific module (with reference closure)");
    eprintln!("  --lang <lang>     Language to check (ts, rust, both) [default: both]");
    eprintln!("  --format <fmt>    Output format (text, json, md) [default: text]");
    eprintln!("  --strategy <str>  Analysis strategy (static, lsp) [default: static]");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  surc diff-impl design.toml .");
    eprintln!("  surc diff-impl design.toml . --mod ui_workspace_pane");
    eprintln!("  surc diff-impl design.toml . --strategy lsp");
}

fn run_diff_impl(args: &[String]) -> Result<(), Box<dyn Error>> {
    use survibe_parser_rs::diff_impl::{diff_impl, reporter};

    if args.len() < 2 {
        print_diff_impl_usage();
        std::process::exit(1);
    }

    let ir_file = Path::new(&args[0]);
    let workspace_root = Path::new(&args[1]);

    // Parse options
    let mut filter_mod: Option<&str> = None;
    let mut language = "both";
    let mut format = "text";
    let mut strategy = "static";

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--mod" => {
                if i + 1 < args.len() {
                    filter_mod = Some(&args[i + 1]);
                    i += 2;
                } else {
                    eprintln!("Error: --mod requires a module name");
                    std::process::exit(1);
                }
            }
            "--lang" => {
                if i + 1 < args.len() {
                    language = &args[i + 1];
                    if !matches!(language, "ts" | "rust" | "both") {
                        eprintln!("Error: --lang must be 'ts', 'rust', or 'both'");
                        std::process::exit(1);
                    }
                    i += 2;
                } else {
                    eprintln!("Error: --lang requires a value");
                    std::process::exit(1);
                }
            }
            "--format" => {
                if i + 1 < args.len() {
                    format = &args[i + 1];
                    if !matches!(format, "text" | "json" | "md") {
                        eprintln!("Error: --format must be 'text', 'json', or 'md'");
                        std::process::exit(1);
                    }
                    i += 2;
                } else {
                    eprintln!("Error: --format requires a value");
                    std::process::exit(1);
                }
            }
            "--strategy" => {
                if i + 1 < args.len() {
                    strategy = &args[i + 1];
                    if !matches!(strategy, "static" | "lsp") {
                        eprintln!("Error: --strategy must be 'static' or 'lsp'");
                        std::process::exit(1);
                    }
                    i += 2;
                } else {
                    eprintln!("Error: --strategy requires a value");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("Error: Unknown option: {}", args[i]);
                print_diff_impl_usage();
                std::process::exit(1);
            }
        }
    }

    // Run diff-impl analysis
    let result = diff_impl(ir_file, workspace_root, filter_mod, language, strategy)?;

    // Format and print output
    let output = match format {
        "json" => reporter::report_json(&result),
        "md" => reporter::report_markdown(&result),
        _ => reporter::report_text(&result),
    };

    println!("{}", output);

    // Exit with non-zero code if issues detected
    if result.has_issues() {
        std::process::exit(1);
    }

    Ok(())
}
