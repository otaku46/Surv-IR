// surc split implementation
// Phase 1: Basic split with shared_symbols=copy

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use survibe_parser_rs::{parse_surv_file, SurvFile, Section, SchemaSection, FuncSection, ModSection};

#[derive(Debug)]
pub struct SplitConfig {
    pub output_dir: PathBuf,
    pub manifest: String,
    pub project_name: String,
    pub ir_root: String,
    pub shared_symbols: SharedSymbolsPolicy,
    pub run_project_check: bool,
    pub packages: Vec<PackageConfig>,
}

#[derive(Debug, Clone)]
pub struct PackageConfig {
    pub name: String,
    pub root: PathBuf,
    pub namespace: String,
    pub depends: Vec<String>,
    pub modules: Vec<ModuleAssignment>,
}

#[derive(Debug, Clone)]
pub struct ModuleAssignment {
    pub mod_name: String,  // "mod.user_api"
    pub file_path: String,  // "user.toml"
}

#[derive(Debug, Clone)]
pub enum SharedSymbolsPolicy {
    Copy,
    // Hoist, Error  // Future phases
}

pub struct SplitContext {
    pub input_ast: SurvFile,
    pub config: SplitConfig,
    pub warnings: Vec<String>,
}

pub struct DependencyClosure {
    pub schemas: HashSet<String>,
    pub funcs: HashSet<String>,
}

pub fn run_split(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() < 3 {
        eprintln!("Usage: surc split <input.toml> --config <split_config.toml>");
        std::process::exit(1);
    }

    let input_path = &args[0];
    let config_path = parse_split_args(&args[1..])?;

    println!("Splitting {} using config {}", input_path, config_path);

    // 1. Parse input IR
    let input_content = fs::read_to_string(input_path)?;
    let input_ast = parse_surv_file(input_content.as_bytes())?;

    // 2. Parse split config
    let config = parse_split_config(&config_path)?;

    // 3. Validate config against input
    validate_config(&input_ast, &config)?;

    // 4. Build split context
    let mut ctx = SplitContext {
        input_ast,
        config,
        warnings: Vec::new(),
    };

    // 5. Execute split
    execute_split(&mut ctx)?;

    // 6. Generate surv.toml
    generate_manifest(&ctx)?;

    // 7. Show warnings
    if !ctx.warnings.is_empty() {
        println!("\nWarnings:");
        for warning in &ctx.warnings {
            println!("  {}", warning);
        }
    }

    // 8. Run project-check if enabled
    if ctx.config.run_project_check {
        println!("\nRunning project-check...");
        let manifest_path = ctx.config.output_dir.join(&ctx.config.manifest);
        // TODO: Call project-check
        println!("  (project-check not yet integrated)");
    }

    println!("\n✓ Split completed successfully");
    Ok(())
}

fn parse_split_args(args: &[String]) -> Result<String, Box<dyn Error>> {
    for i in 0..args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            return Ok(args[i + 1].clone());
        }
    }
    Err("Missing --config argument".into())
}

fn parse_split_config(path: &str) -> Result<SplitConfig, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let doc = content.parse::<toml::Value>()?;

    let split_section = doc.get("split")
        .ok_or("Missing [split] section")?;

    let output_dir = split_section.get("output_dir")
        .and_then(|v| v.as_str())
        .ok_or("Missing split.output_dir")?;

    let manifest = split_section.get("manifest")
        .and_then(|v| v.as_str())
        .ok_or("Missing split.manifest")?;

    let project_name = split_section.get("project_name")
        .and_then(|v| v.as_str())
        .ok_or("Missing split.project_name")?;

    let ir_root = split_section.get("ir_root")
        .and_then(|v| v.as_str())
        .unwrap_or(output_dir);

    // Parse behavior
    let behavior = doc.get("split").and_then(|s| s.get("behavior"));
    let shared_symbols = behavior
        .and_then(|b| b.get("shared_symbols"))
        .and_then(|v| v.as_str())
        .unwrap_or("copy");

    let shared_symbols_policy = match shared_symbols {
        "copy" => SharedSymbolsPolicy::Copy,
        _ => return Err(format!("Unsupported shared_symbols: {}", shared_symbols).into()),
    };

    let run_project_check = behavior
        .and_then(|b| b.get("run_project_check"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // Parse packages
    let packages_section = doc.get("split")
        .and_then(|s| s.get("packages"))
        .ok_or("Missing [split.packages]")?;

    let mut packages = Vec::new();

    if let Some(packages_table) = packages_section.as_table() {
        for (pkg_name, pkg_value) in packages_table {
            let root = pkg_value.get("root")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("Missing root for package {}", pkg_name))?;

            let namespace = pkg_value.get("namespace")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("Missing namespace for package {}", pkg_name))?;

            let depends = pkg_value.get("depends")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let modules_arr = pkg_value.get("modules")
                .and_then(|v| v.as_array())
                .ok_or_else(|| format!("Missing modules for package {}", pkg_name))?;

            let mut modules = Vec::new();
            for mod_entry in modules_arr {
                let mod_name = mod_entry.get("mod")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'mod' in module entry")?;

                let file_path = mod_entry.get("file")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'file' in module entry")?;

                modules.push(ModuleAssignment {
                    mod_name: mod_name.to_string(),
                    file_path: file_path.to_string(),
                });
            }

            packages.push(PackageConfig {
                name: pkg_name.clone(),
                root: PathBuf::from(root),
                namespace: namespace.to_string(),
                depends,
                modules,
            });
        }
    }

    Ok(SplitConfig {
        output_dir: PathBuf::from(output_dir),
        manifest: manifest.to_string(),
        project_name: project_name.to_string(),
        ir_root: ir_root.to_string(),
        shared_symbols: shared_symbols_policy,
        run_project_check,
        packages,
    })
}

fn validate_config(ast: &SurvFile, config: &SplitConfig) -> Result<(), Box<dyn Error>> {
    // E_MOD_NOT_FOUND: Check all referenced modules exist
    for pkg in &config.packages {
        for mod_assignment in &pkg.modules {
            let mod_key = mod_assignment.mod_name.strip_prefix("mod.")
                .ok_or_else(|| format!("Module name must start with 'mod.': {}", mod_assignment.mod_name))?;

            let found = ast.sections.iter().any(|sec| {
                matches!(sec, Section::Mod(mod_sec) if mod_sec.name == mod_key)
            });

            if !found {
                return Err(format!("E_MOD_NOT_FOUND: Module '{}' not found in input IR", mod_assignment.mod_name).into());
            }
        }
    }

    // E_DUP_OUTPUT: Check no duplicate file paths
    let mut file_paths = HashSet::new();
    for pkg in &config.packages {
        for mod_assignment in &pkg.modules {
            let full_path = pkg.root.join(&mod_assignment.file_path);
            if !file_paths.insert(full_path.clone()) {
                return Err(format!("E_DUP_OUTPUT: Duplicate output path: {:?}", full_path).into());
            }
        }
    }

    Ok(())
}

fn execute_split(ctx: &mut SplitContext) -> Result<(), Box<dyn Error>> {
    // Create output directory
    fs::create_dir_all(&ctx.config.output_dir)?;

    // Track shared symbols for warnings
    let mut symbol_usage: HashMap<String, Vec<String>> = HashMap::new();

    // Process each package
    for pkg in &ctx.config.packages {
        let pkg_dir = ctx.config.output_dir.join(&pkg.root);
        fs::create_dir_all(&pkg_dir)?;

        // Process each module in this package
        for mod_assignment in &pkg.modules {
            let output_path = pkg_dir.join(&mod_assignment.file_path);

            // Check for existing file
            if output_path.exists() {
                return Err(format!("E_WRITE_CONFLICT: File already exists: {:?}", output_path).into());
            }

            // Compute dependency closure for this module
            let closure = compute_closure(&ctx.input_ast, &mod_assignment.mod_name)?;

            // Track symbol usage
            for schema in &closure.schemas {
                symbol_usage.entry(schema.clone())
                    .or_insert_with(Vec::new)
                    .push(output_path.to_string_lossy().to_string());
            }
            for func in &closure.funcs {
                symbol_usage.entry(func.clone())
                    .or_insert_with(Vec::new)
                    .push(output_path.to_string_lossy().to_string());
            }

            // Generate file content
            let content = generate_file_content(
                &ctx.input_ast,
                pkg,
                &mod_assignment.mod_name,
                &closure,
            )?;

            // Write file
            fs::write(&output_path, content)?;
            println!("  ✓ Created {:?} ({})", output_path, mod_assignment.mod_name);
        }
    }

    // Generate W_SHARED_SYMBOL_COPIED warnings
    for (symbol, files) in &symbol_usage {
        if files.len() > 1 {
            ctx.warnings.push(format!(
                "W_SHARED_SYMBOL_COPIED: {} copied to {} files",
                symbol,
                files.len()
            ));
        }
    }

    Ok(())
}

fn compute_closure(ast: &SurvFile, mod_name: &str) -> Result<DependencyClosure, Box<dyn Error>> {
    let mod_key = mod_name.strip_prefix("mod.")
        .ok_or_else(|| format!("Invalid module name: {}", mod_name))?;

    // Find the module
    let mod_section = ast.sections.iter()
        .find_map(|sec| {
            if let Section::Mod(mod_sec) = sec {
                if mod_sec.name == mod_key {
                    return Some(mod_sec);
                }
            }
            None
        })
        .ok_or_else(|| format!("Module not found: {}", mod_name))?;

    let mut closure = DependencyClosure {
        schemas: HashSet::new(),
        funcs: HashSet::new(),
    };

    // Collect direct references from module
    for schema_ref in &mod_section.schemas {
        if let Some(schema_name) = schema_ref.strip_prefix("schema.") {
            closure.schemas.insert(schema_name.to_string());
        }
    }

    for func_ref in &mod_section.funcs {
        if let Some(func_name) = func_ref.strip_prefix("func.") {
            closure.funcs.insert(func_name.to_string());
        }
    }

    for pipeline_item in &mod_section.pipeline {
        if let Some(func_name) = pipeline_item.strip_prefix("func.") {
            closure.funcs.insert(func_name.to_string());
        }
    }

    // Expand closure: funcs -> schemas
    let initial_funcs = closure.funcs.clone();
    for func_name in &initial_funcs {
        expand_func_closure(ast, func_name, &mut closure);
    }

    // Expand closure: schemas -> schemas (recursive references)
    let mut changed = true;
    while changed {
        changed = false;
        let current_schemas: Vec<_> = closure.schemas.iter().cloned().collect();
        for schema_name in &current_schemas {
            if expand_schema_closure(ast, schema_name, &mut closure) {
                changed = true;
            }
        }
    }

    Ok(closure)
}

fn expand_func_closure(ast: &SurvFile, func_name: &str, closure: &mut DependencyClosure) {
    // Find func section
    for sec in &ast.sections {
        if let Section::Func(func_sec) = sec {
            if func_sec.name == func_name {
                // Add input schemas
                for input_ref in &func_sec.input {
                    if let Some(schema_name) = input_ref.strip_prefix("schema.") {
                        closure.schemas.insert(schema_name.to_string());
                    }
                }

                // Add output schemas
                for output_ref in &func_sec.output {
                    if let Some(schema_name) = output_ref.strip_prefix("schema.") {
                        closure.schemas.insert(schema_name.to_string());
                    }
                }
                break;
            }
        }
    }
}

fn expand_schema_closure(ast: &SurvFile, schema_name: &str, closure: &mut DependencyClosure) -> bool {
    let mut added = false;

    for sec in &ast.sections {
        if let Section::Schema(schema_sec) = sec {
            if schema_sec.name == schema_name {
                // Handle edge schemas (from/to)
                if schema_sec.kind == "edge" {
                    if !schema_sec.from.is_empty() {
                        if let Some(from_name) = schema_sec.from.strip_prefix("schema.") {
                            if closure.schemas.insert(from_name.to_string()) {
                                added = true;
                            }
                        }
                    }

                    if !schema_sec.to.is_empty() {
                        if let Some(to_name) = schema_sec.to.strip_prefix("schema.") {
                            if closure.schemas.insert(to_name.to_string()) {
                                added = true;
                            }
                        }
                    }
                }
                break;
            }
        }
    }

    added
}

fn generate_file_content(
    ast: &SurvFile,
    pkg: &PackageConfig,
    mod_name: &str,
    closure: &DependencyClosure,
) -> Result<String, Box<dyn Error>> {
    let mut output = String::new();

    // Header
    output.push_str(&format!("package = \"{}\"\n", pkg.name));
    output.push_str(&format!("namespace = \"{}\"\n", pkg.namespace));

    // require (get from original file - we'll use the file-level requires if they exist)
    // For now, we'll extract requires from the original module if it had any
    // Note: In Surv IR v1.1, require is at file level, not in mod section
    // We'll copy from the original input file's requires
    if !ast.requires.is_empty() {
        let require_strings: Vec<String> = ast.requires.iter()
            .map(|r| format!("\"{}\"", r.target))
            .collect();
        output.push_str(&format!("require = [{}]\n", require_strings.join(", ")));
    }

    output.push('\n');

    // Schemas (sorted)
    let mut schema_names: Vec<_> = closure.schemas.iter().collect();
    schema_names.sort();

    for schema_name in schema_names {
        if let Some(Section::Schema(schema_sec)) = ast.sections.iter().find(|s| {
            matches!(s, Section::Schema(sc) if sc.name == *schema_name)
        }) {
            output.push_str(&format!("[schema.{}]\n", schema_sec.name));
            output.push_str(&format!("kind = \"{}\"\n", schema_sec.kind));
            if !schema_sec.role.is_empty() {
                output.push_str(&format!("role = \"{}\"\n", schema_sec.role));
            }
            if !schema_sec.from.is_empty() {
                output.push_str(&format!("from = \"{}\"\n", schema_sec.from));
            }
            if !schema_sec.to.is_empty() {
                output.push_str(&format!("to = \"{}\"\n", schema_sec.to));
            }
            if !schema_sec.fields.is_empty() {
                // Format fields as inline TOML map
                let fields_vec: Vec<String> = schema_sec.fields.iter()
                    .map(|(k, v)| format!("{} = \"{}\"", k, v))
                    .collect();
                output.push_str(&format!("fields = {{{}}}\n", fields_vec.join(", ")));
            }
            output.push('\n');
        }
    }

    // Funcs (sorted)
    let mut func_names: Vec<_> = closure.funcs.iter().collect();
    func_names.sort();

    for func_name in func_names {
        if let Some(Section::Func(func_sec)) = ast.sections.iter().find(|s| {
            matches!(s, Section::Func(fc) if fc.name == *func_name)
        }) {
            output.push_str(&format!("[func.{}]\n", func_sec.name));
            if !func_sec.intent.is_empty() {
                output.push_str(&format!("intent = \"{}\"\n", func_sec.intent));
            }
            if !func_sec.input.is_empty() {
                output.push_str(&format!("input = {:?}\n", func_sec.input));
            }
            if !func_sec.output.is_empty() {
                output.push_str(&format!("output = {:?}\n", func_sec.output));
            }
            output.push('\n');
        }
    }

    // Module
    let mod_key = mod_name.strip_prefix("mod.").unwrap();
    if let Some(Section::Mod(mod_sec)) = ast.sections.iter().find(|s| {
        matches!(s, Section::Mod(ms) if ms.name == mod_key)
    }) {
        output.push_str(&format!("[mod.{}]\n", mod_sec.name));
        if !mod_sec.purpose.is_empty() {
            output.push_str(&format!("purpose = \"{}\"\n", mod_sec.purpose));
        }
        if !mod_sec.schemas.is_empty() {
            output.push_str(&format!("schemas = {:?}\n", mod_sec.schemas));
        }
        if !mod_sec.funcs.is_empty() {
            output.push_str(&format!("funcs = {:?}\n", mod_sec.funcs));
        }
        if !mod_sec.pipeline.is_empty() {
            output.push_str(&format!("pipeline = {:?}\n", mod_sec.pipeline));
        }
        output.push('\n');
    }

    Ok(output)
}

fn generate_manifest(ctx: &SplitContext) -> Result<(), Box<dyn Error>> {
    let manifest_path = ctx.config.output_dir.join(&ctx.config.manifest);

    let mut output = String::new();

    // Project section
    output.push_str("[project]\n");
    output.push_str(&format!("name = \"{}\"\n\n", ctx.config.project_name));

    // Paths section
    output.push_str("[paths]\n");
    output.push_str(&format!("ir_root = \"{}\"\n\n", ctx.config.ir_root));

    // Package sections (sorted)
    let mut packages = ctx.config.packages.clone();
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    for pkg in &packages {
        output.push_str(&format!("[packages.{}]\n", pkg.name));
        output.push_str(&format!("root = \"{}\"\n", pkg.root.to_string_lossy()));
        output.push_str(&format!("namespace = \"{}\"\n", pkg.namespace));

        if !pkg.depends.is_empty() {
            output.push_str(&format!("depends = {:?}\n", pkg.depends));
        }

        output.push('\n');
    }

    fs::write(&manifest_path, output)?;
    println!("  ✓ Created {:?}", manifest_path);

    Ok(())
}
