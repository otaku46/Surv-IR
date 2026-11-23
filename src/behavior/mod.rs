mod explorer;
mod expr;
mod lean;
pub(crate) mod loader;
mod model;
mod reporter;
mod smt;
mod symbolic;
mod tla;
pub use lean::{
    generate_lean_spec, generate_lean_spec_with_proof, LeanProof, LeanProofMode, LeanSpec,
};
pub use tla::{generate_tla_spec, TlaPlusSpec};

pub use explorer::{
    CounterexampleTrace, ExplorationOutcome, ModelState, OverflowAttempt, ReplayStep, ReplayTrace,
    StateVarBinding, TraceStep,
};
pub use expr::{ExprAST, ReplayValue, ReplayValues, StateValue, TypedExpr};
pub use loader::{looks_like_behavior, BehaviorSource, BehaviorVerificationConfig, ProjectContext};
pub use model::VerificationBackend;
pub use model::{
    BehaviorDiagnostic, BehaviorFileAST, ExplorerConfig, FrontendOutcome, NormalizedBehaviorModel,
};
pub use reporter::{verify, VerificationReport};

use std::path::Path;

pub fn compile(path: &Path, manifest: Option<&Path>) -> Vec<FrontendOutcome> {
    let source = match read_behavior_source(path, manifest) {
        Ok(source) => source,
        Err(issue) => return vec![issue],
    };
    let project = match loader::load_behavior_project_context(&source) {
        Ok(project) => project,
        Err(issue) => return vec![fail_on_project_load(&source, issue)],
    };
    if project
        .diagnostics
        .iter()
        .any(|issue| issue.severity == "error")
    {
        return vec![FrontendOutcome::error(
            path.display().to_string(),
            project.diagnostics,
        )];
    }
    compile_source(&source, &project)
}

fn read_behavior_source(
    path: &Path,
    manifest: Option<&Path>,
) -> Result<BehaviorSource, FrontendOutcome> {
    let content = std::fs::read_to_string(path).map_err(|err| {
        let target = path.display().to_string();
        let diagnostic = BehaviorDiagnostic::error("E_BEHAVIOR_SOURCE", &target, err.to_string());
        FrontendOutcome::error(target, vec![diagnostic])
    })?;
    Ok(BehaviorSource {
        path: path.to_owned(),
        content,
        manifest_path: manifest.map(Path::to_owned),
    })
}

fn compile_source(source: &BehaviorSource, project: &ProjectContext) -> Vec<FrontendOutcome> {
    let (ast, context, mut diagnostics) = match loader::parse_behavior_file(source, project) {
        Ok(result) => result,
        Err(err) => {
            let diagnostic = BehaviorDiagnostic::error(
                "E_BEHAVIOR_TOML",
                source.path.display().to_string(),
                err,
            );
            return vec![fail_on_toml_parse(source, diagnostic)];
        }
    };
    diagnostics.extend(project.diagnostics.iter().cloned());
    let (expressions, diagnostics) = match model::parse_expressions(source, &ast, diagnostics) {
        Ok(parsed) => parsed,
        Err(diagnostics) => return vec![fail_on_expr_parse(source, diagnostics)],
    };
    model::check_behavior_semantics(&ast, expressions, project, &context, &diagnostics)
        .into_iter()
        .map(model::build_frontend_outcome)
        .collect()
}

fn compile_with_project(path: &Path, project: &ProjectContext) -> Vec<FrontendOutcome> {
    match read_behavior_source(path, project.manifest_path.as_deref()) {
        Ok(source) => compile_source(&source, project),
        Err(issue) => vec![issue],
    }
}

pub fn compile_project(manifest: &Path) -> Vec<FrontendOutcome> {
    let target = manifest.display().to_string();
    let error = |message| {
        let diagnostic = BehaviorDiagnostic::error("E_BEHAVIOR_PROJECT", &target, message);
        vec![FrontendOutcome::error(target.clone(), vec![diagnostic])]
    };
    let project = match loader::load_project_context(Some(manifest)) {
        Ok(project) => project,
        Err(err) => return error(err),
    };
    if project
        .diagnostics
        .iter()
        .any(|diag| diag.severity == "error")
    {
        return vec![FrontendOutcome::error(target, project.diagnostics)];
    }
    match loader::behavior_paths(&project) {
        Ok(paths) => paths
            .iter()
            .flat_map(|path| compile_with_project(path, &project))
            .collect(),
        Err(err) => error(err),
    }
}

fn fail_on_project_load(
    source: &BehaviorSource,
    diagnostic: BehaviorDiagnostic,
) -> FrontendOutcome {
    FrontendOutcome::error(source.path.display().to_string(), vec![diagnostic])
}

fn fail_on_toml_parse(source: &BehaviorSource, diagnostic: BehaviorDiagnostic) -> FrontendOutcome {
    FrontendOutcome::error(source.path.display().to_string(), vec![diagnostic])
}

fn fail_on_expr_parse(
    source: &BehaviorSource,
    diagnostics: Vec<BehaviorDiagnostic>,
) -> FrontendOutcome {
    FrontendOutcome::error(source.path.display().to_string(), diagnostics)
}

pub struct BehaviorReference {
    pub kind: &'static str,
    pub name: String,
    pub source: std::path::PathBuf,
}

pub fn reverse_references(
    manifest: &Path,
    reference: &str,
    kind: crate::SymbolKind,
) -> Result<Vec<BehaviorReference>, String> {
    use std::collections::BTreeSet;
    let prefix = match kind {
        crate::SymbolKind::Func => "func",
        crate::SymbolKind::Mod => "mod",
        crate::SymbolKind::Schema => return Ok(Vec::new()),
    };
    let project = loader::load_project_context(Some(manifest))?;
    let ids: BTreeSet<_> = project
        .symbol_table
        .entries
        .iter()
        .filter(|entry| {
            if entry.kind != kind {
                return false;
            }
            let local = format!("{prefix}.{}", entry.local_name);
            reference == entry.fq_name
                || reference == local
                || reference == format!("{}.{local}", entry.package)
                || entry
                    .namespace
                    .as_ref()
                    .is_some_and(|ns| reference == format!("{ns}.{local}"))
        })
        .map(|entry| entry.fq_name.as_str())
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for source in loader::behavior_paths(&project)? {
        for model in compile_with_project(&source, &project)
            .into_iter()
            .filter_map(|outcome| outcome.model)
        {
            if kind == crate::SymbolKind::Mod {
                if model
                    .resolved_owner_fqn
                    .as_ref()
                    .is_some_and(|owner| ids.contains(owner.as_str()))
                {
                    found.push(BehaviorReference {
                        kind: "behavior",
                        name: format!("behavior.{}", model.name),
                        source: source.clone(),
                    });
                }
            } else {
                for action in model.actions {
                    if action
                        .resolved_func_fqns
                        .iter()
                        .any(|id| ids.contains(id.as_str()))
                    {
                        found.push(BehaviorReference {
                            kind: "action",
                            name: format!("behavior.{}.action.{}", model.name, action.name),
                            source: source.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(found)
}
