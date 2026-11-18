use std::error::Error;
use std::path::Path;
use survibe_parser_rs::{
    load_project_context, reference_candidates, Manifest, ProjectContext, SymbolEntry, SymbolKind,
};

enum Scope {
    Packages,
    Package(String),
    Module(String),
    CrossPackage,
}

struct DepsOptions {
    scope: Scope,
    format: OutputFormat,
}

enum OutputFormat {
    Text,
    Mermaid,
}

#[derive(Clone)]
struct ModuleInfo {
    fq: String,
    package: String,
    namespace: Option<String>,
    display: String,
}

#[derive(Clone)]
struct RequireEdge {
    from_fq: String,
    from_display: String,
    from_package: String,
    to_fq: String,
    to_display: String,
    to_package: String,
}

pub fn run_deps(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() {
        print_deps_usage();
        std::process::exit(1);
    }

    let manifest_path = Path::new(&args[0]);
    let options = parse_options(&args[1..])?;
    let context = load_project_context(manifest_path)?;
    let manifest = context
        .manifest
        .as_ref()
        .ok_or("Expected a surv.toml manifest")?;

    match options.format {
        OutputFormat::Text => match options.scope {
            Scope::Packages => show_package_deps(manifest),
            Scope::Package(name) => show_package_modules(&context, &name),
            Scope::Module(name) => show_module_deps(&context, &name)?,
            Scope::CrossPackage => show_cross_package_deps(&context),
        },
        OutputFormat::Mermaid => match options.scope {
            Scope::Packages => export_package_deps_mermaid(manifest),
            Scope::CrossPackage => export_cross_package_mermaid(&context),
            _ => {
                eprintln!("Mermaid format only supported for --packages and --cross-package");
                std::process::exit(1);
            }
        },
    }

    Ok(())
}

fn parse_options(args: &[String]) -> Result<DepsOptions, Box<dyn Error>> {
    let mut scope = Scope::Packages;
    let mut format = OutputFormat::Text;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--package" => {
                if i + 1 < args.len() {
                    scope = Scope::Package(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err("--package requires a package name".into());
                }
            }
            "--module" | "--mod" => {
                if i + 1 < args.len() {
                    let raw = &args[i + 1];
                    let module_name = if raw.contains(".mod.") || raw.starts_with("mod.") {
                        raw.clone()
                    } else {
                        format!("mod.{raw}")
                    };
                    scope = Scope::Module(module_name);
                    i += 2;
                } else {
                    return Err("--module requires a module name".into());
                }
            }
            "--cross-package" => {
                scope = Scope::CrossPackage;
                i += 1;
            }
            "--format" => {
                if i + 1 < args.len() {
                    format = match args[i + 1].as_str() {
                        "text" => OutputFormat::Text,
                        "mermaid" => OutputFormat::Mermaid,
                        other => return Err(format!("Unknown format: {}", other).into()),
                    };
                    i += 2;
                } else {
                    return Err("--format requires a value (text, mermaid)".into());
                }
            }
            other => {
                return Err(format!("Unknown option: {}", other).into());
            }
        }
    }

    Ok(DepsOptions { scope, format })
}

fn modules(context: &ProjectContext) -> Vec<ModuleInfo> {
    context
        .symbols
        .entries
        .iter()
        .filter(|entry| entry.kind == SymbolKind::Mod)
        .map(|entry| ModuleInfo {
            fq: entry.fq_name.clone(),
            package: entry.package.clone(),
            namespace: entry.namespace.clone(),
            display: format!("mod.{}", entry.local_name),
        })
        .collect()
}

fn find_module<'a>(modules: &'a [ModuleInfo], raw: &str) -> Result<&'a ModuleInfo, Box<dyn Error>> {
    let matches: Vec<&ModuleInfo> = modules
        .iter()
        .filter(|module| {
            module.display == raw
                || module.fq == raw
                || module.display == format!("mod.{raw}")
                || format!("{}.{}", module.package, module.display) == raw
                || module
                    .namespace
                    .as_ref()
                    .is_some_and(|namespace| format!("{namespace}.{}", module.display) == raw)
        })
        .collect();
    match matches.as_slice() {
        [module] => Ok(*module),
        [] => Err(format!("Module '{raw}' not found").into()),
        _ => {
            let candidates = matches
                .iter()
                .map(|module| module.fq.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!("Ambiguous module '{raw}'; candidates: {candidates}").into())
        }
    }
}

fn resolved_edges(context: &ProjectContext) -> Vec<RequireEdge> {
    let mut edges = Vec::new();
    for (path, file) in &context.project.files {
        let Some(import_context) = context
            .contexts
            .iter()
            .find(|context| &context.file_path == path)
        else {
            continue;
        };
        let file_modules: Vec<&SymbolEntry> = context
            .symbols
            .entries
            .iter()
            .filter(|entry| entry.kind == SymbolKind::Mod && &entry.file == path)
            .collect();
        for requirement in &file.requires {
            let matches = reference_candidates(
                &context.symbols,
                SymbolKind::Mod,
                &requirement.target,
                import_context,
            );
            let [to] = matches.as_slice() else {
                continue;
            };
            for from in &file_modules {
                if from.fq_name == to.fq_name && file_modules.len() > 1 {
                    continue;
                }
                edges.push(RequireEdge {
                    from_fq: from.fq_name.clone(),
                    from_display: format!("mod.{}", from.local_name),
                    from_package: from.package.clone(),
                    to_fq: to.fq_name.clone(),
                    to_display: format!("mod.{}", to.local_name),
                    to_package: to.package.clone(),
                });
            }
        }
    }
    edges.sort_by(|left, right| (&left.from_fq, &left.to_fq).cmp(&(&right.from_fq, &right.to_fq)));
    edges.dedup_by(|left, right| left.from_fq == right.from_fq && left.to_fq == right.to_fq);
    edges
}

fn show_package_deps(manifest: &Manifest) {
    println!("Packages:");
    println!();

    if manifest.packages.is_empty() {
        println!("  No packages defined in manifest");
        return;
    }

    for (pkg_name, pkg) in &manifest.packages {
        let namespace = pkg.namespace.as_deref().unwrap_or("<none>");
        println!("  {} (namespace: {})", pkg_name, namespace);
        println!("    root: {}", pkg.root);

        if !pkg.depends.is_empty() {
            println!("    depends:");
            for dep in &pkg.depends {
                println!("      └─> {}", dep);
            }
        }
        println!();
    }
}

fn show_package_modules(context: &ProjectContext, package_name: &str) {
    let all_modules = modules(context);
    let mut modules_in_package: Vec<&ModuleInfo> = all_modules
        .iter()
        .filter(|module| module.package == package_name)
        .collect();
    modules_in_package.sort_by(|left, right| left.display.cmp(&right.display));

    let pkg = context
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.packages.get(package_name));
    println!(
        "Package: {} (namespace: {})",
        package_name,
        pkg.and_then(|pkg| pkg.namespace.as_deref())
            .unwrap_or("<none>")
    );
    println!();

    if modules_in_package.is_empty() {
        println!("  No modules found in this package");
        return;
    }

    println!("Modules ({}):", modules_in_package.len());
    let edges = resolved_edges(context);
    for module in modules_in_package {
        println!("  {}", module.display);
        for edge in edges.iter().filter(|edge| edge.from_fq == module.fq) {
            if edge.to_package == package_name {
                println!("    └─> {}", edge.to_display);
            } else {
                println!(
                    "    └─> {} (from {} package)",
                    edge.to_display, edge.to_package
                );
            }
        }
    }
}

fn show_module_deps(context: &ProjectContext, module_name: &str) -> Result<(), Box<dyn Error>> {
    let all_modules = modules(context);
    let target = find_module(&all_modules, module_name)?;
    let edges = resolved_edges(context);

    println!("{} (in {} package)", target.display, target.package);
    println!();

    let deps: Vec<_> = edges
        .iter()
        .filter(|edge| edge.from_fq == target.fq)
        .collect();
    let dependents: Vec<_> = edges
        .iter()
        .filter(|edge| edge.to_fq == target.fq)
        .collect();

    if !deps.is_empty() {
        println!("Dependencies:");
        for edge in &deps {
            if edge.to_package == target.package {
                println!("  └─> {}", edge.to_display);
            } else {
                println!("  └─> {} (in {} package)", edge.to_display, edge.to_package);
            }
        }
        println!();
    }

    if !dependents.is_empty() {
        println!("Dependents:");
        for edge in &dependents {
            if edge.from_package == target.package {
                println!("  └─> {}", edge.from_display);
            } else {
                println!(
                    "  └─> {} (in {} package)",
                    edge.from_display, edge.from_package
                );
            }
        }
        println!();
    }

    if deps.is_empty() && dependents.is_empty() {
        println!("No dependencies or dependents found");
    }

    Ok(())
}

fn show_cross_package_deps(context: &ProjectContext) {
    println!("Cross-package dependencies:");
    println!();

    let mut edges: Vec<_> = resolved_edges(context)
        .into_iter()
        .filter(|edge| edge.from_package != edge.to_package)
        .collect();
    edges.sort_by(|left, right| {
        (
            &left.from_package,
            &left.from_fq,
            &left.to_package,
            &left.to_fq,
        )
            .cmp(&(
                &right.from_package,
                &right.from_fq,
                &right.to_package,
                &right.to_fq,
            ))
    });
    edges.dedup_by(|left, right| left.from_fq == right.from_fq && left.to_fq == right.to_fq);

    if edges.is_empty() {
        println!("  No cross-package dependencies found");
        return;
    }

    for edge in edges {
        println!(
            "  {}.{} → {}.{}",
            edge.from_package, edge.from_display, edge.to_package, edge.to_display
        );
    }
}

fn export_package_deps_mermaid(manifest: &Manifest) {
    println!("graph TD");

    for (pkg_name, pkg) in &manifest.packages {
        let pkg_id = pkg_name.replace('-', "_");

        for dep in &pkg.depends {
            let dep_id = dep.replace('-', "_");
            println!("  {}[{}] --> {}[{}]", pkg_id, pkg_name, dep_id, dep);
        }
    }
}

fn export_cross_package_mermaid(context: &ProjectContext) {
    println!("graph TD");

    let mut edges: Vec<_> = resolved_edges(context)
        .into_iter()
        .filter(|edge| edge.from_package != edge.to_package)
        .collect();
    edges.sort_by(|left, right| {
        (
            &left.from_package,
            &left.from_fq,
            &left.to_package,
            &left.to_fq,
        )
            .cmp(&(
                &right.from_package,
                &right.from_fq,
                &right.to_package,
                &right.to_fq,
            ))
    });
    edges.dedup_by(|left, right| left.from_fq == right.from_fq && left.to_fq == right.to_fq);

    for edge in edges {
        let from_id =
            format!("{}_{}", edge.from_package, edge.from_display).replace(['.', '-'], "_");
        let to_id = format!("{}_{}", edge.to_package, edge.to_display).replace(['.', '-'], "_");
        println!(
            "  {}[\"{}.{}\"] --> {}[\"{}.{}\"]",
            from_id, edge.from_package, edge.from_display, to_id, edge.to_package, edge.to_display
        );
    }
}

fn print_deps_usage() {
    eprintln!("Usage: surc deps <surv.toml> [options]");
    eprintln!();
    eprintln!("Show package and module dependencies.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --package <name>     Show modules in a specific package");
    eprintln!("  --module <name>      Show dependencies for a specific module");
    eprintln!("  --cross-package      Show only cross-package dependencies");
    eprintln!("  --format <format>    Output format (text, mermaid) [default: text]");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  surc deps surv.toml");
    eprintln!("  surc deps surv.toml --package backend");
    eprintln!("  surc deps surv.toml --module mod.user_api");
    eprintln!("  surc deps surv.toml --cross-package");
    eprintln!("  surc deps surv.toml --format mermaid > deps.md");
}
