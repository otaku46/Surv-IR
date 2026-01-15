use survibe_parser_rs::{load_project, Manifest, ProjectAST, Section};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

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

pub fn run_deps(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() {
        print_deps_usage();
        std::process::exit(1);
    }

    let manifest_path = Path::new(&args[0]);
    let options = parse_options(&args[1..])?;

    // Load manifest
    let manifest_content = fs::read_to_string(manifest_path)?;
    let manifest: Manifest = toml::from_str(&manifest_content)?;

    // Load project
    let project = load_project(manifest_path)?;

    // Execute based on scope
    match options.format {
        OutputFormat::Text => match options.scope {
            Scope::Packages => show_package_deps(&manifest, &project),
            Scope::Package(ref name) => show_package_modules(&manifest, &project, name)?,
            Scope::Module(ref name) => show_module_deps(&manifest, &project, name)?,
            Scope::CrossPackage => show_cross_package_deps(&manifest, &project)?,
        },
        OutputFormat::Mermaid => match options.scope {
            Scope::Packages => export_package_deps_mermaid(&manifest),
            Scope::CrossPackage => export_cross_package_mermaid(&manifest, &project)?,
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
                    let module_name = args[i + 1].strip_prefix("mod.").unwrap_or(&args[i + 1]);
                    scope = Scope::Module(format!("mod.{}", module_name));
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

fn show_package_deps(manifest: &Manifest, _project: &ProjectAST) {
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

fn show_package_modules(
    manifest: &Manifest,
    project: &ProjectAST,
    package_name: &str,
) -> Result<(), Box<dyn Error>> {
    let pkg = manifest
        .packages
        .get(package_name)
        .ok_or_else(|| format!("Package '{}' not found", package_name))?;

    println!("Package: {} (namespace: {})", package_name, pkg.namespace.as_deref().unwrap_or("<none>"));
    println!();

    // Build module to package mapping
    let module_to_package = build_module_to_package_map(manifest, project)?;

    // Find modules in this package
    let mut modules_in_package: Vec<String> = project
        .mods
        .keys()
        .filter(|mod_id| {
            module_to_package
                .get(*mod_id)
                .map(|p| p == package_name)
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    modules_in_package.sort();

    if modules_in_package.is_empty() {
        println!("  No modules found in this package");
        return Ok(());
    }

    println!("Modules ({}):", modules_in_package.len());

    // Get all dependencies
    let normalized_reqs = project.collect_normalized_requires();

    for mod_id in modules_in_package {
        println!("  {}", mod_id);

        // Find dependencies
        let deps: Vec<_> = normalized_reqs
            .iter()
            .filter(|req| req.from_mod == mod_id)
            .collect();

        if !deps.is_empty() {
            for dep in deps {
                let dep_package = module_to_package.get(&dep.to_mod);
                match dep_package {
                    Some(dep_pkg) if dep_pkg == package_name => {
                        println!("    └─> {}", dep.to_mod);
                    }
                    Some(dep_pkg) => {
                        println!("    └─> {} (from {} package)", dep.to_mod, dep_pkg);
                    }
                    None => {
                        println!("    └─> {} (package unknown)", dep.to_mod);
                    }
                }
            }
        }
    }

    Ok(())
}

fn show_module_deps(
    manifest: &Manifest,
    project: &ProjectAST,
    module_name: &str,
) -> Result<(), Box<dyn Error>> {
    // Verify module exists
    if !project.mods.contains_key(module_name) {
        return Err(format!("Module '{}' not found", module_name).into());
    }

    // Build module to package mapping
    let module_to_package = build_module_to_package_map(manifest, project)?;

    let pkg = module_to_package.get(module_name);

    print!("{}", module_name);
    if let Some(pkg_name) = pkg {
        println!(" (in {} package)", pkg_name);
    } else {
        println!(" (package unknown)");
    }
    println!();

    // Get all dependencies
    let normalized_reqs = project.collect_normalized_requires();

    // Find dependencies (modules this module requires)
    let deps: Vec<_> = normalized_reqs
        .iter()
        .filter(|req| req.from_mod == module_name)
        .collect();

    // Find dependents (modules that require this module)
    let dependents: Vec<_> = normalized_reqs
        .iter()
        .filter(|req| req.to_mod == module_name)
        .collect();

    if !deps.is_empty() {
        println!("Dependencies:");
        for dep in &deps {
            let dep_package = module_to_package.get(&dep.to_mod);
            match dep_package {
                Some(dep_pkg) if pkg.map(|p| dep_pkg == p).unwrap_or(false) => {
                    println!("  └─> {}", dep.to_mod);
                }
                Some(dep_pkg) => {
                    println!("  └─> {} (in {} package)", dep.to_mod, dep_pkg);
                }
                None => {
                    println!("  └─> {} (package unknown)", dep.to_mod);
                }
            }
        }
        println!();
    }

    if !dependents.is_empty() {
        println!("Dependents:");
        for dependent in &dependents {
            let dependent_package = module_to_package.get(&dependent.from_mod);
            match dependent_package {
                Some(dependent_pkg) if pkg.map(|p| dependent_pkg == p).unwrap_or(false) => {
                    println!("  └─> {}", dependent.from_mod);
                }
                Some(dependent_pkg) => {
                    println!("  └─> {} (in {} package)", dependent.from_mod, dependent_pkg);
                }
                None => {
                    println!("  └─> {} (package unknown)", dependent.from_mod);
                }
            }
        }
        println!();
    }

    if deps.is_empty() && dependents.is_empty() {
        println!("No dependencies or dependents found");
    }

    Ok(())
}

fn show_cross_package_deps(
    manifest: &Manifest,
    project: &ProjectAST,
) -> Result<(), Box<dyn Error>> {
    println!("Cross-package dependencies:");
    println!();

    // Build module to package mapping
    let module_to_package = build_module_to_package_map(manifest, project)?;

    // Get all dependencies
    let normalized_reqs = project.collect_normalized_requires();

    let mut cross_package_edges = Vec::new();

    for req in &normalized_reqs {
        let from_pkg = module_to_package.get(&req.from_mod);
        let to_pkg = module_to_package.get(&req.to_mod);

        if from_pkg != to_pkg && from_pkg.is_some() && to_pkg.is_some() {
            cross_package_edges.push((
                from_pkg.unwrap(),
                &req.from_mod,
                to_pkg.unwrap(),
                &req.to_mod,
            ));
        }
    }

    if cross_package_edges.is_empty() {
        println!("  No cross-package dependencies found");
        return Ok(());
    }

    cross_package_edges.sort();

    for (from_pkg, from_mod, to_pkg, to_mod) in cross_package_edges {
        println!("  {}.{} → {}.{}", from_pkg, from_mod, to_pkg, to_mod);
    }

    Ok(())
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

fn export_cross_package_mermaid(
    manifest: &Manifest,
    project: &ProjectAST,
) -> Result<(), Box<dyn Error>> {
    println!("graph TD");

    // Build module to package mapping
    let module_to_package = build_module_to_package_map(manifest, project)?;

    // Get all dependencies
    let normalized_reqs = project.collect_normalized_requires();

    let mut cross_package_edges = Vec::new();

    for req in &normalized_reqs {
        let from_pkg = module_to_package.get(&req.from_mod);
        let to_pkg = module_to_package.get(&req.to_mod);

        if from_pkg != to_pkg && from_pkg.is_some() && to_pkg.is_some() {
            cross_package_edges.push((
                from_pkg.unwrap(),
                &req.from_mod,
                to_pkg.unwrap(),
                &req.to_mod,
            ));
        }
    }

    cross_package_edges.sort();
    cross_package_edges.dedup();

    for (from_pkg, from_mod, to_pkg, to_mod) in cross_package_edges {
        let from_id = format!("{}_{}", from_pkg, from_mod).replace(['.', '-'], "_");
        let to_id = format!("{}_{}", to_pkg, to_mod).replace(['.', '-'], "_");

        println!(
            "  {}[\"{}.{}\"] --> {}[\"{}.{}\"]",
            from_id, from_pkg, from_mod, to_id, to_pkg, to_mod
        );
    }

    Ok(())
}

fn build_module_to_package_map(
    manifest: &Manifest,
    project: &ProjectAST,
) -> Result<HashMap<String, String>, Box<dyn Error>> {
    let mut module_to_package: HashMap<String, String> = HashMap::new();

    for (pkg_name, _pkg) in &manifest.packages {
        // Find all modules in this package's files
        for (_file_path, file_ast) in &project.files {
            // Check if this file belongs to this package
            // (Simple heuristic: check if file's package header matches)
            if file_ast.package.as_ref() == Some(pkg_name) {
                for section in &file_ast.sections {
                    if let Section::Mod(m) = section {
                        module_to_package.insert(format!("mod.{}", m.name), pkg_name.clone());
                    }
                }
            }
        }
    }

    Ok(module_to_package)
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
