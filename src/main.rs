use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use survibe_parser_rs::codegen::{GitHubActionsGenerator, GitLabCIGenerator};
use survibe_parser_rs::{
    check_deploy_file, check_project, check_surv_file, load_manifest, load_project,
    parse_deploy_file, parse_surv_file, Diagnostic, HtmlExporter, MermaidExporter, ProjectAST,
    Section,
};

mod deps_commands;
mod query_commands;
mod status_commands;
use deps_commands::run_deps;
use query_commands::{run_refs, run_slice, run_trace};
use status_commands::run_status;

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
        "check" => run_unified_check(&args[2..]),
        // Legacy aliases kept for backward compatibility
        "project-check" => {
            if args.len() < 3 {
                eprintln!("Usage: surc check <surv.toml>");
                std::process::exit(1);
            }
            run_project_check(&args[2])
        }
        "deploy-check" => {
            if args.len() < 3 {
                eprintln!("Usage: surc check <deploy.toml>");
                std::process::exit(1);
            }
            run_deploy_check(&args[2])
        }
        "parse" => {
            if args.len() >= 3 && (args[2] == "--help" || args[2] == "-h") {
                eprintln!("Usage: surc check <file.toml> --ast");
                return Ok(());
            }
            if args.len() < 3 {
                eprintln!("Usage: surc check <file.toml> --ast");
                std::process::exit(1);
            }
            run_parse(&args[2])
        }
        "inspect" => run_inspect_cmd(&args[2..]),
        "slice" => run_slice(&args[2..]),
        "refs" => run_refs(&args[2..]),
        "trace" => run_trace(&args[2..]),
        "deps" => {
            if args.len() < 3 {
                eprintln!("Usage: surc deps <surv.toml> [options]");
                std::process::exit(1);
            }
            run_deps(&args[2..])
        }
        "diff" | "diff-impl" => run_diff_impl(&args[2..]),
        "skeleton" | "init-ir" => run_skeleton(&args[1], &args[2..]),
        "export" => {
            if args.len() < 3 {
                print_export_usage();
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
        "status" => run_status(&args[2..]),
        "-" => run_parse_reader(io::stdin()),
        other => {
            if args.len() == 2 && Path::new(other).exists() {
                run_unified_check(&args[1..])
            } else {
                eprintln!("Unknown command: {}", other);
                eprintln!();
                print_usage();
                std::process::exit(1);
            }
        }
    }
}

fn print_usage() {
    eprintln!("Usage: surc <command> [options]");
    eprintln!();
    eprintln!("Core Commands:");
    eprintln!("  check <path> [--ast]        Validate Surv IR file, project (surv.toml), or Deploy IR");
    eprintln!("  inspect [target] <path>     Inspect a module or project overview");
    eprintln!("  slice <target> <path>       Extract minimal IR fragment for a target (--with-defs)");
    eprintln!("  refs <target> <path>        List references to a symbol across IR");
    eprintln!("  trace <target> <path>       Trace upstream/downstream pipeline & module flow");
    eprintln!("  diff <ir> <workspace>       Detect drift between IR and implementation (alias: diff-impl)");
    eprintln!("  skeleton <mode> <target>    Generate IR skeleton from code or code stubs from IR (alias: init-ir)");
    eprintln!("  export <type> <path>        Export Mermaid diagrams or interactive HTML");
    eprintln!("  codegen <platform> <file>   Generate CI/CD workflow (github-actions, gitlab-ci)");
    eprintln!("  status <subcommand>         Track module implementation status");
    eprintln!();
    eprintln!("Export types:");
    eprintln!("  pipeline <path> <mod>       Export module pipeline as Mermaid");
    eprintln!("  module-detail <path> <mod>  Export detailed module view as Mermaid");
    eprintln!("  modules <path>              Export module dependency graph as Mermaid");
    eprintln!("  schemas <path>              Export schema relationship graph as Mermaid");
    eprintln!("  html <path>                 Export interactive HTML visualization (Surv IR or Deploy IR)");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  surc check examples/todo_api.toml");
    eprintln!("  surc check surv.toml");
    eprintln!("  surc check examples/deploy.toml");
    eprintln!("  surc inspect mod.todo_api examples/todo_api.toml");
    eprintln!("  surc slice mod.todo_api examples/todo_api.toml --with-defs");
    eprintln!("  surc diff design.toml .");
    eprintln!("  surc init-ir . --exclude-tests");
    eprintln!("  surc export html surv.toml > output.html");
}

fn print_export_usage() {
    eprintln!("Usage: surc export <type> <path> [module|--mod <module>]");
    eprintln!();
    eprintln!("Export visualizations and diagrams from Surv IR or Deploy IR files.");
    eprintln!();
    eprintln!("Types:");
    eprintln!("  pipeline <path> <mod>       Export a module's pipeline as Mermaid flowchart");
    eprintln!("  module-detail <path> <mod>  Export detailed module view as Mermaid");
    eprintln!("  modules <path>              Export module dependency graph as Mermaid");
    eprintln!("  schemas <path>              Export schema relationship graph as Mermaid");
    eprintln!("  html <path>                 Export interactive HTML visualization (D3.js)");
    eprintln!("  deploy-mermaid <deploy.toml> Export deployment pipeline as Mermaid flowchart");
    eprintln!("  deploy-html <deploy.toml>   Export deployment pipeline as interactive HTML");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IrKind {
    Manifest,
    Deploy,
    SurvFile,
}

fn detect_ir_kind(path: &Path) -> Result<IrKind, Box<dyn Error>> {
    if load_manifest(path).is_ok() {
        return Ok(IrKind::Manifest);
    }
    let raw = fs::read_to_string(path)?;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed == "[project]" || trimmed == "[paths]" || trimmed.starts_with("[packages.") {
            return Ok(IrKind::Manifest);
        }
        if trimmed == "[deploy]" || trimmed.starts_with("[deploy.") {
            return Ok(IrKind::Deploy);
        }
    }
    Ok(IrKind::SurvFile)
}

fn load_project_or_single(path: &Path) -> Result<ProjectAST, Box<dyn Error>> {
    match detect_ir_kind(path)? {
        IrKind::Manifest => load_project(path),
        IrKind::SurvFile => {
            let file = File::open(path)?;
            let ast = parse_surv_file(file)?;
            Ok(ProjectAST::from_files(vec![(path.to_path_buf(), ast)]))
        }
        IrKind::Deploy => Err(format!(
            "'{}' is a Deploy IR file; expected a Surv IR file or surv.toml manifest",
            path.display()
        )
        .into()),
    }
}

fn run_unified_check(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Usage: surc check <file.toml|surv.toml|deploy.toml> [--ast]");
        eprintln!();
        eprintln!("Automatically detects and validates:");
        eprintln!("  - Single Surv IR design files (*.toml)");
        eprintln!("  - Multi-package project manifests (surv.toml)");
        eprintln!("  - Deploy IR pipeline files ([deploy.*])");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --ast, --dump-ast   Output parsed AST as JSON instead of diagnostics");
        if args.is_empty() {
            std::process::exit(1);
        }
        return Ok(());
    }

    let dump_ast = args.iter().any(|a| a == "--ast" || a == "--dump-ast");
    let target = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .ok_or("Missing file path for 'surc check'")?;

    if dump_ast {
        return run_parse(target);
    }

    match detect_ir_kind(Path::new(target))? {
        IrKind::Manifest => run_project_check(target),
        IrKind::Deploy => run_deploy_check(target),
        IrKind::SurvFile => run_check(target),
    }
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

fn print_diagnostics(
    diags: &[Diagnostic],
    ok_msg: &str,
    err_label: &str,
) -> Result<(), Box<dyn Error>> {
    if diags.is_empty() {
        println!("✓ {ok_msg}");
        return Ok(());
    }

    let mut error_count = 0;
    let mut warning_count = 0;

    for diag in diags {
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
        if !diag.location.is_empty() {
            println!("  at {}\n", diag.location);
        } else {
            println!();
        }
    }

    println!("---");
    println!("{error_count} error(s), {warning_count} warning(s)");

    if error_count > 0 {
        Err(format!("{err_label} reported errors").into())
    } else {
        Ok(())
    }
}

fn run_check(filename: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(filename)?;
    let ast = parse_surv_file(file)?;
    let diags = check_surv_file(&ast);
    print_diagnostics(&diags, "No issues found", "diagnostics")
}

fn run_project_check(manifest: &str) -> Result<(), Box<dyn Error>> {
    let project = load_project(Path::new(manifest))?;
    let diags = check_project(&project);
    print_diagnostics(&diags, "No project issues found", "project diagnostics")
}

fn run_deploy_check(filename: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(filename)?;
    let deploy = parse_deploy_file(file)?;
    let diags = check_deploy_file(&deploy);
    print_diagnostics(&diags, "No deploy issues found", "deploy diagnostics")
}

fn resolve_file_and_module_args(args: &[String]) -> Result<(String, String), Box<dyn Error>> {
    let mut mod_flag: Option<String> = None;
    let mut positionals = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mod" | "--module" => {
                if i + 1 >= args.len() {
                    return Err("--mod requires a module name".into());
                }
                mod_flag = Some(args[i + 1].clone());
                i += 2;
            }
            other => {
                positionals.push(other.to_string());
                i += 1;
            }
        }
    }

    if let Some(module) = mod_flag {
        let file = positionals
            .into_iter()
            .next()
            .ok_or("Missing file path")?;
        return Ok((file, module));
    }

    if positionals.len() < 2 {
        return Err("Expected <path> and <module-name>".into());
    }

    let first = &positionals[0];
    let second = &positionals[1];
    let first_is_file = first.ends_with(".toml") || Path::new(first).is_file();
    let second_is_file = second.ends_with(".toml") || Path::new(second).is_file();

    if first_is_file && !second_is_file {
        Ok((first.clone(), second.clone()))
    } else if second_is_file && !first_is_file {
        Ok((second.clone(), first.clone()))
    } else {
        Ok((first.clone(), second.clone()))
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
        "pipeline" | "module-detail" => {
            let Ok((file_path, raw_mod)) = resolve_file_and_module_args(&args[1..]) else {
                eprintln!("Usage: surc export {} <path> <module-name>", export_type);
                std::process::exit(1);
            };
            let module_name = raw_mod.strip_prefix("mod.").unwrap_or(&raw_mod);
            let project = load_project_or_single(Path::new(&file_path))?;

            let module = project
                .files
                .iter()
                .flat_map(|(_, f)| &f.sections)
                .find_map(|s| match s {
                    Section::Mod(m) if m.name == module_name => Some(m),
                    _ => None,
                })
                .ok_or_else(|| format!("Module 'mod.{}' not found in '{}'", module_name, file_path))?;

            let output = if export_type == "pipeline" {
                exporter.export_pipeline(module, &project)
            } else {
                exporter.export_module_detail(module, &project)
            };
            println!("{}", output);
        }
        "modules" => {
            if args.len() < 2 {
                eprintln!("Usage: surc export modules <surv.toml|file.toml>");
                std::process::exit(1);
            }
            let project = load_project_or_single(Path::new(&args[1]))?;
            let output = exporter.export_module_dependencies(&project);
            println!("{}", output);
        }
        "schemas" => {
            if args.len() < 2 {
                eprintln!("Usage: surc export schemas <surv.toml|file.toml>");
                std::process::exit(1);
            }
            let project = load_project_or_single(Path::new(&args[1]))?;
            let output = exporter.export_schema_graph(&project);
            println!("{}", output);
        }
        "html" => {
            if args.len() < 2 {
                eprintln!("Usage: surc export html <surv.toml|file.toml|deploy.toml>");
                std::process::exit(1);
            }
            let path = Path::new(&args[1]);
            let html_exporter = HtmlExporter::new();
            if detect_ir_kind(path)? == IrKind::Deploy {
                let file = File::open(path)?;
                let deploy = parse_deploy_file(file)?;
                println!("{}", html_exporter.export_deploy_interactive(&deploy));
            } else {
                let project = load_project_or_single(path)?;
                println!("{}", html_exporter.export_interactive(&project));
            }
        }
        "deploy-mermaid" | "mermaid" => {
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
            eprintln!("Valid types: pipeline, module-detail, modules, schemas, html, deploy-mermaid, deploy-html");
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

fn run_inspect_cmd(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Usage:");
        eprintln!("  surc inspect <module-name> <file.toml|surv.toml>");
        eprintln!("  surc inspect <surv.toml|file.toml>");
        eprintln!();
        eprintln!("Inspect a specific module (schemas, functions, pipeline, status) or a project/file overview.");
        if args.is_empty() {
            std::process::exit(1);
        }
        return Ok(());
    }

    if args.len() == 1 {
        let path = Path::new(&args[0]);
        if detect_ir_kind(path)? == IrKind::Manifest {
            return run_deps(args);
        }
        return run_file_overview(&args[0]);
    }

    let (filename, module_name) = resolve_file_and_module_args(args)?;
    run_inspect(&module_name, &filename)
}

fn run_file_overview(filename: &str) -> Result<(), Box<dyn Error>> {
    let project = load_project_or_single(Path::new(filename))?;
    let mut schemas = 0usize;
    let mut funcs = 0usize;
    let mut mods = Vec::new();

    for (_, file) in &project.files {
        for section in &file.sections {
            match section {
                Section::Schema(_) => schemas += 1,
                Section::Func(_) => funcs += 1,
                Section::Mod(m) => mods.push(format!("mod.{}", m.name)),
                _ => {}
            }
        }
    }

    println!("File: {}", filename);
    println!("Schemas: {}, Functions: {}, Modules: {}", schemas, funcs, mods.len());
    if !mods.is_empty() {
        println!();
        println!("Modules:");
        for m in mods {
            println!("  - {}", m);
        }
    }
    Ok(())
}

fn run_inspect(module_name: &str, filename: &str) -> Result<(), Box<dyn Error>> {
    let project = load_project_or_single(Path::new(filename))?;
    let module_name = module_name.strip_prefix("mod.").unwrap_or(module_name);

    let mut found_mod = None;
    let mut found_status = None;

    for (_, parsed) in &project.files {
        for section in &parsed.sections {
            match section {
                Section::Mod(m) if m.name == module_name => {
                    found_mod = Some(m);
                }
                Section::Status(s) => {
                    if s.modules.contains_key(module_name) {
                        found_status = Some(s);
                    }
                }
                _ => {}
            }
        }
    }

    let module = match found_mod {
        Some(m) => m,
        None => {
            eprintln!(
                "Error: Module 'mod.{}' not found in {}",
                module_name, filename
            );
            eprintln!();
            eprintln!("Available modules:");
            for (_, parsed) in &project.files {
                for section in &parsed.sections {
                    if let Section::Mod(m) = section {
                        eprintln!("  - mod.{}", m.name);
                    }
                }
            }
            std::process::exit(1);
        }
    };

    println!("Module: mod.{}", module.name);
    println!();

    if !module.purpose.is_empty() {
        println!("Purpose: {}", module.purpose);
        println!();
    }

    if !module.schemas.is_empty() {
        println!("Schemas ({}):", module.schemas.len());
        for schema in &module.schemas {
            println!("  - {}", schema);
        }
        println!();
    }

    if !module.funcs.is_empty() {
        println!("Functions ({}):", module.funcs.len());
        for func in &module.funcs {
            println!("  - {}", func);
        }
        println!();
    }

    let pipeline_calls = module.get_pipeline_calls();
    if !pipeline_calls.is_empty() {
        println!("Pipeline ({} steps):", pipeline_calls.len());
        for (i, step) in pipeline_calls.iter().enumerate() {
            if i == 0 {
                println!("  {}", step);
            } else {
                println!("    ↓");
                println!("  {}", step);
            }
        }
        println!();
    }

    if let Some(status_section) = found_status {
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

fn print_skeleton_usage() {
    eprintln!("Usage:");
    eprintln!("  surc init-ir <workspace_root> [options]");
    eprintln!("  surc skeleton [design] <workspace_root> [options]");
    eprintln!("  surc skeleton code <design.toml|surv.toml> [workspace_root] [options]");
    eprintln!();
    eprintln!("Generate Surv IR skeletons from source code (Code -> IR) or implementation stubs from IR (IR -> Code).");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --lang <lang>       Language filter (ts, rust, c, zig, both, all) [default: both]");
    eprintln!("  --exclude-tests     Exclude test-only symbols from design skeleton output");
    eprintln!("  --dedup <mode>      Deduplicate symbols by name, path, or none [default: path]");
    eprintln!("  --emit <items>      Comma-separated sections: schemas,funcs,mods,mapping");
    eprintln!("  --mod <module>      Filter to specific module (for 'skeleton code')");
    eprintln!("  --ir <file.toml>    Only emit design skeleton for symbols not already in <file.toml>");
    eprintln!("  --strategy <str>    Analysis strategy (static, lsp) [default: static]");
}

fn run_skeleton(invoked_as: &str, args: &[String]) -> Result<(), Box<dyn Error>> {
    use survibe_parser_rs::diff_impl::{
        diff_impl, extract_ir_for_code_skeleton, reporter, scan_workspace_for_skeleton, DedupMode,
        DesignSkeletonOptions,
    };

    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print_skeleton_usage();
        if args.is_empty() {
            std::process::exit(1);
        }
        return Ok(());
    }

    let (mode, rest) = if invoked_as == "init-ir" {
        ("design", args)
    } else if matches!(args[0].as_str(), "design" | "ir" | "code") {
        (args[0].as_str(), &args[1..])
    } else {
        ("design", args)
    };

    if rest.is_empty() {
        print_skeleton_usage();
        std::process::exit(1);
    }

    let mut language = "both";
    let mut strategy = "static";
    let mut filter_mod: Option<&str> = None;
    let mut base_ir: Option<&str> = None;
    let mut design_options = DesignSkeletonOptions::default();
    let mut positionals = Vec::new();

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--lang" => {
                if i + 1 >= rest.len() {
                    return Err("Error: --lang requires a value".into());
                }
                language = &rest[i + 1];
                if !matches!(language, "ts" | "rust" | "c" | "zig" | "both" | "all") {
                    return Err(
                        "Error: --lang must be 'ts', 'rust', 'c', 'zig', 'both', or 'all'".into(),
                    );
                }
                i += 2;
            }
            "--exclude-tests" => {
                design_options.exclude_tests = true;
                i += 1;
            }
            "--dedup" => {
                if i + 1 >= rest.len() {
                    return Err("Error: --dedup requires a value".into());
                }
                design_options.dedup = DedupMode::parse(&rest[i + 1])
                    .ok_or("Error: --dedup must be name, path, or none")?;
                i += 2;
            }
            "--emit" => {
                if i + 1 >= rest.len() {
                    return Err("Error: --emit requires a value".into());
                }
                design_options.emit_schemas = false;
                design_options.emit_funcs = false;
                design_options.emit_mods = false;
                design_options.emit_mapping = false;
                for item in rest[i + 1].split(',') {
                    match item.trim() {
                        "schemas" => design_options.emit_schemas = true,
                        "funcs" => design_options.emit_funcs = true,
                        "mods" => design_options.emit_mods = true,
                        "mapping" => design_options.emit_mapping = true,
                        "" => {}
                        other => {
                            return Err(format!(
                                "Error: unknown --emit item '{}'; expected schemas, funcs, mods, mapping",
                                other
                            )
                            .into());
                        }
                    }
                }
                i += 2;
            }
            "--mod" => {
                if i + 1 >= rest.len() {
                    return Err("Error: --mod requires a module name".into());
                }
                filter_mod = Some(&rest[i + 1]);
                i += 2;
            }
            "--ir" => {
                if i + 1 >= rest.len() {
                    return Err("Error: --ir requires a file path".into());
                }
                base_ir = Some(&rest[i + 1]);
                i += 2;
            }
            "--strategy" => {
                if i + 1 >= rest.len() {
                    return Err("Error: --strategy requires a value".into());
                }
                strategy = &rest[i + 1];
                if !matches!(strategy, "static" | "lsp") {
                    return Err("Error: --strategy must be 'static' or 'lsp'".into());
                }
                i += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(format!("Error: Unknown option: {}", flag).into());
            }
            pos => {
                positionals.push(pos);
                i += 1;
            }
        }
    }

    if positionals.is_empty() {
        print_skeleton_usage();
        std::process::exit(1);
    }

    match mode {
        "code" => {
            let ir_file = Path::new(positionals[0]);
            let result = if let Some(ws) = positionals.get(1) {
                diff_impl(ir_file, Path::new(ws), filter_mod, language, strategy, None)?
            } else {
                extract_ir_for_code_skeleton(ir_file, filter_mod, language)?
            };
            println!("{}", reporter::report_code_skeleton(&result, language));
        }
        _ => {
            let workspace_root = Path::new(positionals[0]);
            let result = if let Some(ir) = base_ir {
                diff_impl(
                    Path::new(ir),
                    workspace_root,
                    filter_mod,
                    language,
                    strategy,
                    None,
                )?
            } else {
                scan_workspace_for_skeleton(workspace_root, language, strategy)?
            };
            println!(
                "{}",
                reporter::report_design_skeleton(&result, &design_options)
            );
        }
    }

    Ok(())
}

fn print_diff_impl_usage() {
    eprintln!("Usage: surc diff <design.toml|surv.toml> <workspace_root> [options]");
    eprintln!("   (or surc diff-impl <design.toml|surv.toml> <workspace_root> [options])");
    eprintln!();
    eprintln!("Detect drift between Surv IR specifications and actual implementation.");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  <design.toml>     Path to a Surv IR file or project manifest (surv.toml)");
    eprintln!("  <workspace_root>  Path to codebase root directory");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --mod <module>    Filter to specific module (with reference closure)");
    eprintln!(
        "  --lang <lang>     Language to check (ts, rust, c, zig, both, all) [default: both]"
    );
    eprintln!("  --format <fmt>    Output format (text, json, md, gha) [default: text]");
    eprintln!("  --strategy <str>  Analysis strategy (static, lsp) [default: static]");
    eprintln!("  --threshold <n>   Maximum allowed drift rate, 0.0-1.0 [default: 0.0]");
    eprintln!("  --mapping <file>  Path to symbol mapping file (.surv/mapping.toml)");
    eprintln!();
    eprintln!("Note: For generating skeletons, prefer 'surc init-ir <workspace>' or 'surc skeleton code <ir>'.");
}

fn run_diff_impl(args: &[String]) -> Result<(), Box<dyn Error>> {
    use survibe_parser_rs::diff_impl::{diff_impl, reporter, DedupMode, DesignSkeletonOptions};

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_diff_impl_usage();
        return Ok(());
    }

    if args.len() < 2 {
        print_diff_impl_usage();
        std::process::exit(1);
    }

    let ir_file = Path::new(&args[0]);
    let workspace_root = Path::new(&args[1]);

    let mut filter_mod: Option<&str> = None;
    let mut language = "both";
    let mut format = "text";
    let mut strategy = "static";
    let mut threshold = 0.0;
    let mut design_options = DesignSkeletonOptions::default();
    let mut mapping_path: Option<&str> = None;

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
                    if !matches!(language, "ts" | "rust" | "c" | "zig" | "both" | "all") {
                        eprintln!(
                            "Error: --lang must be 'ts', 'rust', 'c', 'zig', 'both', or 'all'"
                        );
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
                    if !matches!(
                        format,
                        "text"
                            | "json"
                            | "md"
                            | "gha"
                            | "skeleton"
                            | "design-skeleton"
                            | "code-skeleton"
                    ) {
                        eprintln!(
                            "Error: --format must be 'text', 'json', 'md', 'gha', 'skeleton', 'design-skeleton', or 'code-skeleton'"
                        );
                        std::process::exit(1);
                    }
                    i += 2;
                } else {
                    eprintln!("Error: --format requires a value");
                    std::process::exit(1);
                }
            }
            "--exclude-tests" => {
                design_options.exclude_tests = true;
                i += 1;
            }
            "--dedup" => {
                if i + 1 < args.len() {
                    design_options.dedup = DedupMode::parse(&args[i + 1])
                        .ok_or("Error: --dedup must be name, path, or none")?;
                    i += 2;
                } else {
                    eprintln!("Error: --dedup requires a value");
                    std::process::exit(1);
                }
            }
            "--emit" => {
                if i + 1 < args.len() {
                    design_options.emit_schemas = false;
                    design_options.emit_funcs = false;
                    design_options.emit_mods = false;
                    design_options.emit_mapping = false;

                    for item in args[i + 1].split(',') {
                        match item.trim() {
                            "schemas" => design_options.emit_schemas = true,
                            "funcs" => design_options.emit_funcs = true,
                            "mods" => design_options.emit_mods = true,
                            "mapping" => design_options.emit_mapping = true,
                            "" => {}
                            other => {
                                return Err(format!(
                                    "Error: unknown --emit item '{}'; expected schemas, funcs, mods, mapping",
                                    other
                                )
                                .into());
                            }
                        }
                    }
                    i += 2;
                } else {
                    eprintln!("Error: --emit requires a value");
                    std::process::exit(1);
                }
            }
            "--mapping" => {
                if i + 1 < args.len() {
                    mapping_path = Some(&args[i + 1]);
                    i += 2;
                } else {
                    eprintln!("Error: --mapping requires a file path");
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
            "--threshold" => {
                if i + 1 < args.len() {
                    threshold = args[i + 1]
                        .parse::<f64>()
                        .map_err(|_| "Error: --threshold must be a number between 0.0 and 1.0")?;
                    if !(0.0..=1.0).contains(&threshold) {
                        return Err("Error: --threshold must be between 0.0 and 1.0".into());
                    }
                    i += 2;
                } else {
                    eprintln!("Error: --threshold requires a value");
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

    let mapping_path = mapping_path.map(Path::new);
    let result = diff_impl(
        ir_file,
        workspace_root,
        filter_mod,
        language,
        strategy,
        mapping_path,
    )?;

    let output = match format {
        "json" => reporter::report_json(&result),
        "md" => reporter::report_markdown(&result),
        "gha" => reporter::report_github_actions(&result),
        "skeleton" => reporter::report_skeletons(&result),
        "design-skeleton" => reporter::report_design_skeleton(&result, &design_options),
        "code-skeleton" => reporter::report_code_skeleton(&result, language),
        _ => reporter::report_text(&result),
    };

    println!("{}", output);

    if result.drift_rate() > threshold {
        std::process::exit(1);
    }

    Ok(())
}
