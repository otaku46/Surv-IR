use survibe_parser_rs::ast::Section;
use survibe_parser_rs::parser::parse_surv_file;
use std::error::Error;
use std::fs::{self, File};
use std::io::Write as IoWrite;

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
    let file = File::open(filename)?;
    let parsed = parse_surv_file(file)?;

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

    // Generate status section
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut status_text = String::from("\n# ============================================================================\n");
    status_text.push_str("# IMPLEMENTATION STATUS\n");
    status_text.push_str("# ============================================================================\n\n");
    status_text.push_str("[status]\n");
    status_text.push_str(&format!("updated_at = \"{}\"\n", today));

    for module in &modules {
        status_text.push_str(&format!("\n[status.mod.{}]\n", module));
        status_text.push_str("state = \"todo\"\n");
        status_text.push_str("coverage = 0.0\n");
        status_text.push_str("notes = \"\"\n");
    }

    // Append to file
    let mut output = fs::OpenOptions::new().append(true).open(filename)?;

    output.write_all(status_text.as_bytes())?;

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
                    coverage = Some(args[i + 1].parse()?);
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

    let content = fs::read_to_string(filename)?;
    let mut new_content = content.clone();

    // Find the module status section
    let section_header = format!("[status.mod.{}]", module_name);

    if !content.contains(&section_header) {
        eprintln!(
            "Error: Module 'mod.{}' not found in status section",
            module_name
        );
        eprintln!("Run 'surc status init {}' first", filename);
        std::process::exit(1);
    }

    // Update fields
    if let Some(new_state) = state {
        let pattern = format!(
            r#"(\[status\.mod\.{}\][^\[]*state = )"[^"]*""#,
            regex::escape(module_name)
        );
        let re = regex::Regex::new(&pattern)?;
        new_content = re
            .replace(&new_content, format!("${{1}}\"{}\"", new_state))
            .to_string();
    }

    if let Some(new_coverage) = coverage {
        let pattern = format!(
            r#"(\[status\.mod\.{}\][^\[]*coverage = )[0-9.]*"#,
            regex::escape(module_name)
        );
        let re = regex::Regex::new(&pattern)?;
        new_content = re
            .replace(&new_content, format!("${{1}}{}", new_coverage))
            .to_string();
    }

    if let Some(new_notes) = notes {
        let pattern = format!(
            r#"(\[status\.mod\.{}\][^\[]*notes = )"[^"]*""#,
            regex::escape(module_name)
        );
        let re = regex::Regex::new(&pattern)?;
        new_content = re
            .replace(&new_content, format!("${{1}}\"{}\"", new_notes))
            .to_string();
    }

    // Update timestamp
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let timestamp_pattern = r#"(\[status\][^\[]*updated_at = )"[^"]*""#;
    let re = regex::Regex::new(timestamp_pattern)?;
    new_content = re
        .replace(&new_content, format!("${{1}}\"{}\"", today))
        .to_string();

    fs::write(filename, new_content)?;

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
    let file = File::open(filename)?;
    let parsed = parse_surv_file(file)?;

    // Check if status section exists
    let has_status = parsed
        .sections
        .iter()
        .any(|s| matches!(s, Section::Status(_)));

    if !has_status {
        eprintln!("No [status] section found in {}", filename);
        eprintln!("Run 'surc status init {}' first", filename);
        std::process::exit(1);
    }

    // Get existing status section
    let status_section = parsed.sections.iter().find_map(|section| {
        if let Section::Status(s) = section {
            Some(s)
        } else {
            None
        }
    }).unwrap();

    // Collect all modules
    let all_modules: Vec<String> = parsed
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

    // Find modules without status
    let missing_modules: Vec<String> = all_modules
        .iter()
        .filter(|module_name| !status_section.modules.contains_key(*module_name))
        .cloned()
        .collect();

    if missing_modules.is_empty() {
        println!("✓ All modules already have status entries");
        return Ok(());
    }

    // Generate status entries for missing modules
    let mut new_status_text = String::new();
    for module in &missing_modules {
        new_status_text.push_str(&format!("\n[status.mod.{}]\n", module));
        new_status_text.push_str("state = \"todo\"\n");
        new_status_text.push_str("coverage = 0.0\n");
        new_status_text.push_str("notes = \"\"\n");
    }

    // Append to file
    let mut output = fs::OpenOptions::new().append(true).open(filename)?;
    output.write_all(new_status_text.as_bytes())?;

    // Update timestamp
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let new_content = fs::read_to_string(filename)?;
    let timestamp_pattern = r#"(\[status\][^\[]*updated_at = )"[^"]*""#;
    let re = regex::Regex::new(timestamp_pattern)?;
    let updated_content = re
        .replace(&new_content, format!("${{1}}\"{}\"", today))
        .to_string();
    fs::write(filename, updated_content)?;

    println!("✓ Synced status section in {}", filename);
    println!("  Added modules: {}", missing_modules.len());
    for module in missing_modules {
        println!("    - mod.{} (todo)", module);
    }

    Ok(())
}
