use std::error::Error;
use std::fs::{self, File};
use std::path::Path;
use survibe_parser_rs::ast::Section;
use survibe_parser_rs::parser::parse_surv_file;
use survibe_parser_rs::status_edit::{
    set_status_value, status_module_mut, write_atomic, StatusDocument,
};

pub fn run_status(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.is_empty() {
        print_status_usage();
        std::process::exit(1);
    }

    match args[0].as_str() {
        "init" => {
            if args.len() < 2 {
                eprintln!("Usage: surc status init <file.toml>");
                std::process::exit(1);
            }
            run_status_init(&args[1])
        }
        "sync" => {
            if args.len() < 2 {
                eprintln!("Usage: surc status sync <file.toml>");
                std::process::exit(1);
            }
            run_status_sync(&args[1])
        }
        "set" => {
            if args.len() < 3 {
                eprintln!("Usage: surc status set <module> <file.toml> [--state <state>] [--coverage <n>] [--notes <text>]");
                std::process::exit(1);
            }
            run_status_set(&args[1], &args[2], &args[3..])
        }
        "list" => {
            if args.len() < 2 {
                eprintln!("Usage: surc status list <file.toml>");
                std::process::exit(1);
            }
            run_status_list(&args[1])
        }
        "show" => {
            if args.len() < 3 {
                eprintln!("Usage: surc status show <module> <file.toml>");
                std::process::exit(1);
            }
            run_status_show(&args[1], &args[2])
        }
        other => {
            eprintln!("Unknown status subcommand: {}", other);
            eprintln!();
            print_status_usage();
            std::process::exit(1);
        }
    }
}

fn print_status_usage() {
    eprintln!("Usage: surc status <subcommand> [options]");
    eprintln!();
    eprintln!("Manage implementation status for modules in Surv IR files.");
    eprintln!();
    eprintln!("Subcommands:");
    eprintln!("  init <file.toml>");
    eprintln!("      Initialize [status] section if not present");
    eprintln!("      Lists all modules with state = 'todo'");
    eprintln!();
    eprintln!("  sync <file.toml>");
    eprintln!("      Sync status section with current modules");
    eprintln!("      Adds missing modules with state = 'todo'");
    eprintln!();
    eprintln!("  set <module> <file.toml> [options]");
    eprintln!("      Update status for a specific module");
    eprintln!("      Options:");
    eprintln!("        --state <state>      Set state (todo, skeleton, partial, done, blocked)");
    eprintln!("        --coverage <0.0-1.0> Set coverage (0.0 to 1.0)");
    eprintln!("        --notes <text>       Set notes");
    eprintln!();
    eprintln!("  list <file.toml>");
    eprintln!("      List all modules with their status");
    eprintln!();
    eprintln!("  show <module> <file.toml>");
    eprintln!("      Show detailed status for a specific module");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  surc status init examples/todo_api.toml");
    eprintln!("  surc status sync examples/todo_api.toml");
    eprintln!("  surc status set mod.book_api api.toml --state partial");
    eprintln!("  surc status set mod.book_api api.toml --coverage 0.6 --notes \"create/get done\"");
    eprintln!("  surc status list examples/todo_api.toml");
    eprintln!("  surc status show mod.todo_api examples/todo_api.toml");
}

fn run_status_init(filename: &str) -> Result<(), Box<dyn Error>> {
    let content = fs::read_to_string(filename)?;
    let parsed = parse_surv_file(content.as_bytes())?;

    // Check if status section already exists
    let has_status = parsed
        .sections
        .iter()
        .any(|s| matches!(s, Section::Status(_)));

    if has_status {
        println!("Status section already exists in {}", filename);
        return Ok(());
    }

    // Collect all modules
    let modules: Vec<String> = parsed
        .sections
        .iter()
        .filter_map(|s| {
            if let Section::Mod(m) = s {
                Some(m.name.clone())
            } else {
                None
            }
        })
        .collect();

    if modules.is_empty() {
        eprintln!("No modules found in {}", filename);
        std::process::exit(1);
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut document = StatusDocument::parse(&content)?;
    {
        let status = document.status(true).ok_or("status must be a table")?;
        set_status_value(status, "updated_at", today);
        for module in &modules {
            let entry = status_module_mut(status, module, true)
                .ok_or_else(|| format!("Invalid status entry for mod.{module}"))?;
            set_status_value(entry, "state", "todo");
            set_status_value(entry, "coverage", 0.0);
            set_status_value(entry, "notes", "");
        }
    }
    write_atomic(Path::new(filename), &content, &document.finish())?;

    println!("✓ Initialized status section in {}", filename);
    println!("  Modules added: {}", modules.len());
    for module in modules {
        println!("    - mod.{} (todo)", module);
    }

    Ok(())
}

fn run_status_set(
    module_name: &str,
    filename: &str,
    args: &[String],
) -> Result<(), Box<dyn Error>> {
    let module_name = module_name.strip_prefix("mod.").unwrap_or(module_name);

    // Parse options
    let mut state: Option<String> = None;
    let mut coverage: Option<f64> = None;
    let mut notes: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--state" => {
                if i + 1 < args.len() {
                    state = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --state requires a value");
                    std::process::exit(1);
                }
            }
            "--coverage" => {
                if i + 1 < args.len() {
                    let value: f64 = args[i + 1]
                        .parse()
                        .map_err(|_| "Error: --coverage must be a number")?;
                    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                        return Err("Error: --coverage must be between 0.0 and 1.0".into());
                    }
                    coverage = Some(value);
                    i += 2;
                } else {
                    eprintln!("Error: --coverage requires a value");
                    std::process::exit(1);
                }
            }
            "--notes" => {
                if i + 1 < args.len() {
                    notes = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --notes requires a value");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

    if state.is_none() && coverage.is_none() && notes.is_none() {
        eprintln!("Error: At least one of --state, --coverage, or --notes is required");
        std::process::exit(1);
    }
    if let Some(state) = &state {
        if !matches!(
            state.as_str(),
            "todo" | "skeleton" | "partial" | "done" | "blocked"
        ) {
            return Err(format!(
                "Error: --state must be todo, skeleton, partial, done, or blocked (got '{state}')"
            )
            .into());
        }
    }

    let content = fs::read_to_string(filename)?;
    let mut document = StatusDocument::parse(&content)?;
    let status = document.status(false).ok_or("No [status] section found")?;
    let module = status_module_mut(status, module_name, false)
        .ok_or_else(|| format!("Module 'mod.{module_name}' not found in status section"))?;
    if let Some(state) = state {
        set_status_value(module, "state", state);
    }
    if let Some(coverage) = coverage {
        set_status_value(module, "coverage", coverage);
    }
    if let Some(notes) = notes {
        set_status_value(module, "notes", notes);
    }
    set_status_value(
        status,
        "updated_at",
        chrono::Local::now().format("%Y-%m-%d").to_string(),
    );
    write_atomic(Path::new(filename), &content, &document.finish())?;

    println!("✓ Updated status for mod.{}", module_name);

    Ok(())
}

fn run_status_list(filename: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(filename)?;
    let parsed = parse_surv_file(file)?;

    let status = parsed.sections.iter().find_map(|section| {
        if let Section::Status(s) = section {
            Some(s)
        } else {
            None
        }
    });

    let modules: Vec<_> = parsed
        .sections
        .iter()
        .filter_map(|s| {
            if let Section::Mod(m) = s {
                Some(m)
            } else {
                None
            }
        })
        .collect();

    if modules.is_empty() {
        println!("No modules found in {}", filename);
        return Ok(());
    }

    println!("Modules in {}:", filename);
    println!();

    for module in modules {
        let module_status = status.and_then(|s| s.modules.get(&module.name));

        print!("  mod.{:<20}", module.name);

        if let Some(ms) = module_status {
            let state_display = match ms.state.as_str() {
                "done" => "✓ done",
                "partial" => "◐ partial",
                "skeleton" => "◯ skeleton",
                "blocked" => "✗ blocked",
                "todo" => "☐ todo",
                _ => &ms.state,
            };

            print!(" {:<12}", state_display);

            if ms.coverage > 0.0 {
                print!(" {:>3.0}%", ms.coverage * 100.0);
            } else {
                print!("     ");
            }

            if !ms.notes.is_empty() {
                print!("  {}", ms.notes);
            }
        } else {
            print!(" (no status)");
        }

        println!();
    }

    if let Some(status_section) = status {
        if !status_section.updated_at.is_empty() {
            println!();
            println!("Last updated: {}", status_section.updated_at);
        }
    }

    Ok(())
}

fn run_status_show(module_name: &str, filename: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(filename)?;
    let parsed = parse_surv_file(file)?;

    let module_name = module_name.strip_prefix("mod.").unwrap_or(module_name);

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
            eprintln!(
                "Error: Module 'mod.{}' not found in {}",
                module_name, filename
            );
            std::process::exit(1);
        }
    };

    let status = parsed.sections.iter().find_map(|section| {
        if let Section::Status(s) = section {
            Some(s)
        } else {
            None
        }
    });

    println!("Module: mod.{}", module.name);
    println!("Purpose: {}", module.purpose);
    println!();

    if let Some(status_section) = status {
        if let Some(module_status) = status_section.modules.get(module_name) {
            println!("Status:");
            println!("  State: {}", module_status.state);
            if module_status.coverage > 0.0 {
                println!("  Coverage: {:.0}%", module_status.coverage * 100.0);
            }
            if !module_status.notes.is_empty() {
                println!("  Notes: {}", module_status.notes);
            }
            if !status_section.updated_at.is_empty() {
                println!("  Updated: {}", status_section.updated_at);
            }
        } else {
            println!("Status: (not set)");
        }
    } else {
        println!("Status: (no status section)");
    }

    Ok(())
}
fn run_status_sync(filename: &str) -> Result<(), Box<dyn Error>> {
    let content = fs::read_to_string(filename)?;
    let parsed = parse_surv_file(content.as_bytes())?;

    let status_section = parsed
        .sections
        .iter()
        .find_map(|section| {
            if let Section::Status(s) = section {
                Some(s)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            format!(
                "No [status] section found in {filename}; run 'surc status init {filename}' first"
            )
        })?;

    let all_modules: Vec<String> = parsed
        .sections
        .iter()
        .filter_map(|section| {
            if let Section::Mod(module) = section {
                Some(module.name.clone())
            } else {
                None
            }
        })
        .collect();

    let missing_modules: Vec<String> = all_modules
        .iter()
        .filter(|module_name| !status_section.modules.contains_key(*module_name))
        .cloned()
        .collect();

    if missing_modules.is_empty() {
        println!("✓ All modules already have status entries");
        return Ok(());
    }

    let mut document = StatusDocument::parse(&content)?;
    {
        let status = document.status(false).ok_or("No [status] section found")?;
        for module in &missing_modules {
            let entry = status_module_mut(status, module, true)
                .ok_or_else(|| format!("Invalid status entry for mod.{module}"))?;
            set_status_value(entry, "state", "todo");
            set_status_value(entry, "coverage", 0.0);
            set_status_value(entry, "notes", "");
        }
        set_status_value(
            status,
            "updated_at",
            chrono::Local::now().format("%Y-%m-%d").to_string(),
        );
    }
    write_atomic(Path::new(filename), &content, &document.finish())?;

    println!("✓ Synced status section in {}", filename);
    println!("  Added modules: {}", missing_modules.len());
    for module in missing_modules {
        println!("    - mod.{} (todo)", module);
    }

    Ok(())
}
