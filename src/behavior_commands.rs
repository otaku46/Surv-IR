use std::error::Error;
use std::path::{Path, PathBuf};
use survibe_parser_rs::behavior::{self, BehaviorDiagnostic, FrontendOutcome, VerificationBackend};

struct VerifyOptions {
    path: PathBuf,
    manifest: Option<PathBuf>,
    target: Option<String>,
    json: bool,
    max_states: Option<usize>,
    max_depth: Option<usize>,
    backend: Option<VerificationBackend>,
}

impl VerifyOptions {
    fn parse(args: &[String]) -> Result<Self, Box<dyn Error>> {
        let mut options = Self {
            path: PathBuf::new(),
            manifest: None,
            target: None,
            json: false,
            max_states: None,
            max_depth: None,
            backend: None,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if arg.starts_with('-') {
                if !matches!(
                    arg.as_str(),
                    "--manifest"
                        | "--behavior"
                        | "--format"
                        | "--max-states"
                        | "--max-depth"
                        | "--backend"
                ) {
                    return Err(format!("unknown Behavior option '{arg}'").into());
                }
                let value = args
                    .next()
                    .filter(|item| !item.starts_with('-'))
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                match arg.as_str() {
                    "--manifest" => options.manifest = Some(value.into()),
                    "--behavior" => {
                        options.target =
                            Some(value.strip_prefix("behavior.").unwrap_or(value).into())
                    }
                    "--format" if matches!(value.as_str(), "json" | "text") => {
                        options.json = value == "json"
                    }
                    "--format" => return Err("--format must be text or json".into()),
                    "--max-states" => {
                        let limit = value.parse()?;
                        if limit == 0 {
                            return Err("--max-states must be positive".into());
                        }
                        options.max_states = Some(limit);
                    }
                    "--max-depth" => options.max_depth = Some(value.parse()?),
                    "--backend" => {
                        options.backend = Some(match value.as_str() {
                            "internal" => VerificationBackend::Internal,
                            "smt" => VerificationBackend::Smt,
                            _ => return Err("--backend must be internal or smt".into()),
                        });
                    }
                    _ => unreachable!(),
                }
            } else if options.path.as_os_str().is_empty() {
                options.path = arg.into();
            } else {
                return Err(format!("unexpected argument '{arg}'").into());
            }
        }
        if options.path.as_os_str().is_empty() {
            return Err("expected a Behavior IR file or surv.toml".into());
        }
        Ok(options)
    }

    fn load(&self) -> Vec<FrontendOutcome> {
        let is_manifest = self
            .path
            .file_name()
            .is_some_and(|name| name == "surv.toml");
        let mut outcomes = if is_manifest {
            behavior::compile_project(&self.path)
        } else {
            behavior::compile(&self.path, self.manifest.as_deref())
        };
        if let Some(name) = &self.target {
            // Keep file/project diagnostics even when a target cannot be selected.
            outcomes.retain(|outcome| outcome.target_name == *name || outcome.has_errors);
        }
        if outcomes.is_empty() {
            let target = self.target.as_deref().map_or_else(
                || "no Behavior models found".into(),
                |name| format!("unknown Behavior '{name}'"),
            );
            outcomes.push(FrontendOutcome {
                target_name: self.path.display().to_string(),
                has_errors: true,
                model: None,
                diagnostics: vec![BehaviorDiagnostic {
                    code: "E_BEHAVIOR_TARGET".into(),
                    severity: "error".into(),
                    message: target,
                    location: self.path.display().to_string(),
                    related_symbol: None,
                }],
            });
        }
        for outcome in &mut outcomes {
            if let Some(model) = &mut outcome.model {
                if let Some(backend) = self.backend {
                    model.config.backend = backend;
                }
                if let Some(limit) = self.max_states {
                    model.config.max_states = limit;
                }
                if let Some(limit) = self.max_depth {
                    model.config.max_depth = limit;
                }
            }
        }
        outcomes
    }
}

pub fn run_verify(args: &[String]) -> Result<i32, Box<dyn Error>> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        println!("Usage: surc verify <file.behavior.toml|surv.toml> [--manifest <path>] [--behavior <name>] [--format text|json] [--backend internal|smt] [--max-states <n>] [--max-depth <n>]");
        return Ok(0);
    }
    let options = VerifyOptions::parse(args)?;
    let reports: Vec<_> = options.load().into_iter().map(behavior::verify).collect();
    let exit_code = if reports.iter().any(|report| report.exit_code() == 1) {
        1
    } else if reports.iter().any(|report| report.exit_code() == 2) {
        2
    } else {
        0
    };
    if options.json {
        if let [report] = reports.as_slice() {
            println!("{}", serde_json::to_string_pretty(report)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&reports)?);
        }
    } else {
        for report in reports {
            print!("{}", report.text());
        }
    }
    Ok(exit_code)
}

pub fn run_behavior_check(
    path: &Path,
    manifest: Option<&Path>,
    ast: bool,
) -> Result<(), Box<dyn Error>> {
    let outcomes = behavior::compile(path, manifest);
    if ast {
        if let [outcome] = outcomes.as_slice() {
            println!("{}", serde_json::to_string_pretty(outcome)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&outcomes)?);
        }
    } else {
        let diagnostics = diagnostics(outcomes.iter().flat_map(|outcome| &outcome.diagnostics));
        super::print_diagnostics(
            &diagnostics,
            "No Behavior issues found",
            "Behavior diagnostics",
        )?;
    }
    if outcomes.iter().any(|outcome| outcome.has_errors) {
        Err("Behavior diagnostics reported errors".into())
    } else {
        Ok(())
    }
}

pub fn project_diagnostics(path: &Path) -> Vec<survibe_parser_rs::Diagnostic> {
    let outcomes = behavior::compile_project(path);
    diagnostics(outcomes.iter().flat_map(|outcome| &outcome.diagnostics))
}

fn diagnostics<'a>(
    items: impl Iterator<Item = &'a BehaviorDiagnostic>,
) -> Vec<survibe_parser_rs::Diagnostic> {
    let mut issues: Vec<_> = items
        .map(|diag| survibe_parser_rs::Diagnostic {
            kind: diag.code.clone(),
            severity: diag.severity.clone(),
            message: diag.message.clone(),
            location: diag.location.clone(),
        })
        .collect();
    issues.sort_by(|a, b| {
        (&a.location, &a.kind, &a.message).cmp(&(&b.location, &b.kind, &b.message))
    });
    issues.dedup_by(|a, b| a.location == b.location && a.kind == b.kind && a.message == b.message);
    issues
}

pub fn run_tla_codegen(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        println!("Usage: surc codegen tla+ <file.behavior.toml|surv.toml> [--manifest <path>] [--behavior <name>] [--format tla|cfg|json]");
        return Ok(());
    }
    let mut format = "tla";
    let mut args = args.iter();
    let mut common = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--format" {
            format = args.next().ok_or("--format requires a value")?;
            if !matches!(format, "tla" | "cfg" | "json") {
                return Err("--format must be tla, cfg or json".into());
            }
        } else {
            common.push(arg.clone());
        }
    }
    let options = VerifyOptions::parse(&common)?;
    let outcomes = options.load();
    let issues = diagnostics(outcomes.iter().flat_map(|outcome| &outcome.diagnostics));
    if issues.iter().any(|issue| issue.severity == "error") {
        super::print_diagnostics(&issues, "No Behavior issues found", "Behavior diagnostics")?;
    }
    let mut specs: Vec<_> = outcomes
        .into_iter()
        .filter_map(|outcome| outcome.model)
        .map(|model| behavior::generate_tla_spec(&model))
        .collect::<Result<Vec<_>, _>>()?;
    if format == "json" && specs.len() != 1 {
        println!("{}", serde_json::to_string_pretty(&specs)?);
        return Ok(());
    }
    if specs.len() != 1 {
        return Err("select one Behavior using --behavior, or request --format json".into());
    }
    let spec = specs.pop().unwrap();
    match format {
        "cfg" => print!("{}", spec.cfg_source),
        "json" => println!("{}", serde_json::to_string_pretty(&spec)?),
        _ => print!("{}", spec.tla_source),
    }
    Ok(())
}

pub fn run_lean_codegen(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        println!("Usage: surc codegen lean <file.behavior.toml|surv.toml> [--manifest <path>] [--behavior <name>] [--format text|json] [--proof auto|obligations|none] [--max-states <n>] [--max-depth <n>]");
        return Ok(());
    }
    let mut proof = behavior::LeanProofMode::Auto;
    let mut common = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--proof" {
            proof = match args.next().map(String::as_str) {
                Some("auto") => behavior::LeanProofMode::Auto,
                Some("obligations") => behavior::LeanProofMode::Obligations,
                Some("none") => behavior::LeanProofMode::None,
                _ => return Err("--proof must be auto, obligations, or none".into()),
            };
        } else {
            common.push(arg.clone());
        }
    }
    let options = VerifyOptions::parse(&common)?;
    let outcomes = options.load();
    let issues = diagnostics(outcomes.iter().flat_map(|outcome| &outcome.diagnostics));
    if issues.iter().any(|issue| issue.severity == "error") {
        return super::print_diagnostics(
            &issues,
            "No Behavior issues found",
            "Behavior diagnostics",
        );
    }
    let specs: Vec<_> = outcomes
        .into_iter()
        .filter_map(|outcome| outcome.model)
        .map(|model| behavior::generate_lean_spec_with_proof(&model, proof))
        .collect::<Result<_, _>>()?;
    match specs.as_slice() {
        [spec] if options.json => println!("{}", serde_json::to_string_pretty(spec)?),
        [spec] => print!("{}", spec.lean_source),
        _ if options.json => println!("{}", serde_json::to_string_pretty(&specs)?),
        _ => return Err("select one Behavior using --behavior, or request --format json".into()),
    }
    Ok(())
}
