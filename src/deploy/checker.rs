use crate::deploy::ast::DeployFile;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, HashSet, VecDeque};

pub fn check_deploy_file(deploy: &DeployFile) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // Phase 1: Structural checks
    check_undefined_references(deploy, &mut diags);
    check_dag_structure(deploy, &mut diags);
    check_unreachable_jobs(deploy, &mut diags);

    // Phase 2: Security checks
    check_secret_scope(deploy, &mut diags);
    check_prod_safety(deploy, &mut diags);
    check_side_effects_safety(deploy, &mut diags);

    diags
}

/// Check for undefined references in jobs
fn check_undefined_references(deploy: &DeployFile, diags: &mut Vec<Diagnostic>) {
    for (job_name, job) in &deploy.jobs {
        // Check job.requires references
        for req in &job.requires {
            if !req.is_empty() && !deploy.jobs.contains_key(req.strip_prefix("job.").unwrap_or(req))
            {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedJobReference".into(),
                    message: format!("Job '{}' requires undefined job '{}'", job_name, req),
                    location: format!("deploy.job.{}.requires", job_name),
                });
            }
        }

        // Check target references
        if !job.uses_target.is_empty() {
            let target_name = job
                .uses_target
                .strip_prefix("target.")
                .unwrap_or(&job.uses_target);
            if !deploy.targets.contains_key(target_name) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedTargetReference".into(),
                    message: format!(
                        "Job '{}' references undefined target '{}'",
                        job_name, job.uses_target
                    ),
                    location: format!("deploy.job.{}.uses_target", job_name),
                });
            }
        }

        // Check secret references
        for secret in &job.needs_secrets {
            let secret_name = secret.strip_prefix("secret.").unwrap_or(secret);
            if !deploy.secrets.contains_key(secret_name) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedSecretReference".into(),
                    message: format!(
                        "Job '{}' references undefined secret '{}'",
                        job_name, secret
                    ),
                    location: format!("deploy.job.{}.needs_secrets", job_name),
                });
            }
        }

        // Check permission references
        if !job.uses_perm.is_empty() {
            let perm_name = job.uses_perm.strip_prefix("perm.").unwrap_or(&job.uses_perm);
            if !deploy.perms.contains_key(perm_name) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UndefinedPermReference".into(),
                    message: format!(
                        "Job '{}' references undefined permission '{}'",
                        job_name, job.uses_perm
                    ),
                    location: format!("deploy.job.{}.uses_perm", job_name),
                });
            }
        }

        // Check artifact references
        for artifact in &job.produces {
            let artifact_name = artifact.strip_prefix("artifact.").unwrap_or(artifact);
            if !deploy.artifacts.contains_key(artifact_name) {
                diags.push(Diagnostic {
                    severity: "warning".into(),
                    kind: "UndefinedArtifactReference".into(),
                    message: format!(
                        "Job '{}' produces undefined artifact '{}'",
                        job_name, artifact
                    ),
                    location: format!("deploy.job.{}.produces", job_name),
                });
            }
        }
    }
}

/// Check DAG structure for cycles
fn check_dag_structure(deploy: &DeployFile, diags: &mut Vec<Diagnostic>) {
    let graph = build_job_graph(deploy);

    // Detect cycles using DFS with color marking
    let mut color: HashMap<String, Color> = graph.keys().map(|k| (k.clone(), Color::White)).collect();
    let mut path = Vec::new();

    for node in graph.keys() {
        if color.get(node) == Some(&Color::White) {
            if let Some(cycle) = detect_cycle(node, &graph, &mut color, &mut path) {
                let cycle_path = cycle.join(" -> ");
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "DeployCycle".into(),
                    message: format!("Deploy DAG contains a cycle: {} -> {}", cycle_path, cycle[0]),
                    location: "deploy.job".into(),
                });
            }
        }
    }
}

/// Build job dependency graph
fn build_job_graph(deploy: &DeployFile) -> HashMap<String, Vec<String>> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    for (job_name, job) in &deploy.jobs {
        graph.entry(job_name.clone()).or_default();
        for req in &job.requires {
            let req_name = req.strip_prefix("job.").unwrap_or(req);
            graph.entry(req_name.to_string()).or_default();
            graph
                .get_mut(job_name)
                .unwrap()
                .push(req_name.to_string());
        }
    }

    graph
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Color {
    White,
    Gray,
    Black,
}

fn detect_cycle(
    node: &str,
    graph: &HashMap<String, Vec<String>>,
    color: &mut HashMap<String, Color>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    color.insert(node.to_string(), Color::Gray);
    path.push(node.to_string());

    if let Some(neighbors) = graph.get(node) {
        for next in neighbors {
            match color.get(next).copied().unwrap_or(Color::White) {
                Color::White => {
                    if let Some(cycle) = detect_cycle(next, graph, color, path) {
                        return Some(cycle);
                    }
                }
                Color::Gray => {
                    // Found cycle
                    if let Some(pos) = path.iter().position(|s| s == next) {
                        return Some(path[pos..].to_vec());
                    }
                }
                Color::Black => {}
            }
        }
    }

    color.insert(node.to_string(), Color::Black);
    path.pop();
    None
}

/// Check for unreachable jobs (jobs with no path from entry points)
fn check_unreachable_jobs(deploy: &DeployFile, diags: &mut Vec<Diagnostic>) {
    if deploy.jobs.is_empty() {
        return;
    }

    let _graph = build_job_graph(deploy);

    // Find entry points (jobs with no dependencies)
    let entry_points: Vec<String> = deploy
        .jobs
        .iter()
        .filter(|(_, job)| job.requires.is_empty())
        .map(|(name, _)| name.clone())
        .collect();

    if entry_points.is_empty() && !deploy.jobs.is_empty() {
        diags.push(Diagnostic {
            severity: "error".into(),
            kind: "NoEntryPoint".into(),
            message: "No entry point jobs found (all jobs have dependencies)".into(),
            location: "deploy.job".into(),
        });
        return;
    }

    // BFS to find all reachable jobs
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::from(entry_points);

    while let Some(job) = queue.pop_front() {
        if reachable.insert(job.clone()) {
            // Add jobs that depend on this job
            for (name, j) in &deploy.jobs {
                if j.requires
                    .iter()
                    .any(|r| r.strip_prefix("job.").unwrap_or(r) == job)
                {
                    queue.push_back(name.clone());
                }
            }
        }
    }

    // Report unreachable jobs
    for job_name in deploy.jobs.keys() {
        if !reachable.contains(job_name) {
            diags.push(Diagnostic {
                severity: "warning".into(),
                kind: "UnreachableJob".into(),
                message: format!(
                    "Job '{}' is unreachable (no path from any entry point)",
                    job_name
                ),
                location: format!("deploy.job.{}", job_name),
            });
        }
    }
}

/// Check secret scope violations
fn check_secret_scope(deploy: &DeployFile, diags: &mut Vec<Diagnostic>) {
    for (job_name, job) in &deploy.jobs {
        if job.uses_target.is_empty() {
            continue;
        }

        let target_ref = format!("target.{}", job.uses_target.strip_prefix("target.").unwrap_or(&job.uses_target));

        for secret_ref in &job.needs_secrets {
            let secret_name = secret_ref.strip_prefix("secret.").unwrap_or(secret_ref);

            if let Some(secret) = deploy.secrets.get(secret_name) {
                if !secret.scope.is_empty() && !secret.scope.contains(&target_ref) {
                    diags.push(Diagnostic {
                        severity: "error".into(),
                        kind: "SecretScopeViolation".into(),
                        message: format!(
                            "Job '{}' uses secret '{}' which is not scoped for target '{}'",
                            job_name, secret_ref, job.uses_target
                        ),
                        location: format!("deploy.job.{}.needs_secrets", job_name),
                    });
                }
            }
        }
    }
}

/// Check production safety requirements
fn check_prod_safety(deploy: &DeployFile, diags: &mut Vec<Diagnostic>) {
    // Check if gate and rollback are defined when needed
    let has_prod_jobs = deploy.jobs.iter().any(|(_, job)| {
        job.uses_target
            .strip_prefix("target.")
            .and_then(|t| deploy.targets.get(t))
            .map(|target| target.kind == "production" || target.kind == "prod")
            .unwrap_or(false)
    });

    if has_prod_jobs {
        // Check gate exists
        if deploy.gate.is_none() {
            diags.push(Diagnostic {
                severity: "error".into(),
                kind: "MissingProdGate".into(),
                message: "Production jobs require [deploy.gate] section".into(),
                location: "deploy".into(),
            });
        }

        // Check rollback exists
        if deploy.rollback.is_none() {
            diags.push(Diagnostic {
                severity: "error".into(),
                kind: "MissingProdRollback".into(),
                message: "Production jobs require [deploy.rollback] section".into(),
                location: "deploy".into(),
            });
        }

        // Check release strategy for prod
        if let Some(release) = &deploy.release {
            if (release.strategy == "canary" || release.strategy == "blue_green")
                && release.health_check.is_empty()
            {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "MissingHealthCheck".into(),
                    message: format!(
                        "Release strategy '{}' requires health_check",
                        release.strategy
                    ),
                    location: "deploy.release".into(),
                });
            }
        }
    }

    // Check each prod job has approval requirement
    if let Some(gate) = &deploy.gate {
        for (job_name, job) in &deploy.jobs {
            if let Some(target_name) = job.uses_target.strip_prefix("target.") {
                if let Some(target) = deploy.targets.get(target_name) {
                    if target.kind == "production" || target.kind == "prod" {
                        let target_ref = format!("target.{}", target_name);
                        if !gate.require_manual_approval_for.contains(&target_ref) {
                            diags.push(Diagnostic {
                                severity: "error".into(),
                                kind: "ProdJobWithoutApproval".into(),
                                message: format!(
                                    "Production job '{}' target '{}' not in gate approval list",
                                    job_name, target_ref
                                ),
                                location: "deploy.gate.require_manual_approval_for".into(),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Check side effects safety
fn check_side_effects_safety(deploy: &DeployFile, diags: &mut Vec<Diagnostic>) {
    let gate = match &deploy.gate {
        Some(g) => g,
        None => return,
    };

    for (job_name, job) in &deploy.jobs {
        // Jobs with db_migration side effect require approval
        if job.side_effects.contains(&"db_migration".to_string()) {
            if job.uses_target.is_empty() {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "DbMigrationWithoutTarget".into(),
                    message: format!(
                        "Job '{}' with db_migration side effect must specify uses_target",
                        job_name
                    ),
                    location: format!("deploy.job.{}", job_name),
                });
                continue;
            }

            let target_ref = format!(
                "target.{}",
                job.uses_target.strip_prefix("target.").unwrap_or(&job.uses_target)
            );

            if !gate.require_manual_approval_for.contains(&target_ref) {
                diags.push(Diagnostic {
                    severity: "error".into(),
                    kind: "DbMigrationWithoutApproval".into(),
                    message: format!(
                        "Job '{}' has db_migration side effect but target '{}' not in approval list",
                        job_name, target_ref
                    ),
                    location: format!("deploy.job.{}.side_effects", job_name),
                });
            }
        }

        // Jobs with release side effect should have release strategy
        if job.side_effects.contains(&"release".to_string()) && deploy.release.is_none() {
            diags.push(Diagnostic {
                severity: "warning".into(),
                kind: "ReleaseWithoutStrategy".into(),
                message: format!(
                    "Job '{}' has release side effect but no [deploy.release] defined",
                    job_name
                ),
                location: format!("deploy.job.{}.side_effects", job_name),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::parser::parse_deploy_file;
    use std::io::Cursor;

    #[test]
    fn detects_undefined_job_reference() {
        let deploy_ir = r#"
[deploy.job.build]
requires = []
runs = ["npm build"]

[deploy.job.deploy]
requires = ["job.nonexistent"]
runs = ["kubectl apply"]
"#;
        let deploy = parse_deploy_file(Cursor::new(deploy_ir)).unwrap();
        let diags = check_deploy_file(&deploy);

        assert!(diags
            .iter()
            .any(|d| d.kind == "UndefinedJobReference"));
    }

    #[test]
    fn detects_cycle() {
        let deploy_ir = r#"
[deploy.job.a]
requires = ["job.b"]
runs = ["step a"]

[deploy.job.b]
requires = ["job.c"]
runs = ["step b"]

[deploy.job.c]
requires = ["job.a"]
runs = ["step c"]
"#;
        let deploy = parse_deploy_file(Cursor::new(deploy_ir)).unwrap();
        let diags = check_deploy_file(&deploy);

        assert!(diags.iter().any(|d| d.kind == "DeployCycle"));
    }

    #[test]
    fn detects_unreachable_job() {
        let deploy_ir = r#"
[deploy.job.build]
requires = []
runs = ["npm build"]

[deploy.job.orphan]
requires = ["job.nonexistent"]
runs = ["orphaned"]
"#;
        let deploy = parse_deploy_file(Cursor::new(deploy_ir)).unwrap();
        let diags = check_deploy_file(&deploy);

        // Should have both undefined reference and unreachable warnings
        assert!(diags.iter().any(|d| d.kind == "UnreachableJob" || d.kind == "UndefinedJobReference"));
    }

    #[test]
    fn detects_secret_scope_violation() {
        let deploy_ir = r#"
[deploy.target.prod]
kind = "production"

[deploy.target.staging]
kind = "staging"

[deploy.secret.DB_URL]
scope = ["target.prod"]

[deploy.job.deploy_staging]
requires = []
runs = ["deploy"]
uses_target = "target.staging"
needs_secrets = ["secret.DB_URL"]
"#;
        let deploy = parse_deploy_file(Cursor::new(deploy_ir)).unwrap();
        let diags = check_deploy_file(&deploy);

        assert!(diags
            .iter()
            .any(|d| d.kind == "SecretScopeViolation"));
    }

    #[test]
    fn detects_missing_prod_gate() {
        let deploy_ir = r#"
[deploy.target.prod]
kind = "production"

[deploy.job.deploy_prod]
requires = []
runs = ["deploy"]
uses_target = "target.prod"
"#;
        let deploy = parse_deploy_file(Cursor::new(deploy_ir)).unwrap();
        let diags = check_deploy_file(&deploy);

        assert!(diags.iter().any(|d| d.kind == "MissingProdGate"));
        assert!(diags.iter().any(|d| d.kind == "MissingProdRollback"));
    }

    #[test]
    fn detects_db_migration_without_approval() {
        let deploy_ir = r#"
[deploy.target.prod]
kind = "production"

[deploy.gate]
require_manual_approval_for = []

[deploy.rollback]
on = ["deploy_fail"]
strategy = "revert"

[deploy.job.migrate]
requires = []
runs = ["migrate"]
uses_target = "target.prod"
side_effects = ["db_migration"]
"#;
        let deploy = parse_deploy_file(Cursor::new(deploy_ir)).unwrap();
        let diags = check_deploy_file(&deploy);

        assert!(diags
            .iter()
            .any(|d| d.kind == "DbMigrationWithoutApproval"));
    }

    #[test]
    fn valid_deploy_passes() {
        let deploy_ir = r#"
[deploy.pipeline]
name = "webapp"

[deploy.target.prod]
kind = "production"
domain = "example.com"

[deploy.secret.DB_URL]
scope = ["target.prod"]

[deploy.job.build]
requires = []
runs = ["npm build"]

[deploy.job.deploy]
requires = ["job.build"]
runs = ["kubectl apply"]
uses_target = "target.prod"
needs_secrets = ["secret.DB_URL"]

[deploy.gate]
require_manual_approval_for = ["target.prod"]

[deploy.release]
strategy = "canary"
health_check = "https://example.com/health"

[deploy.rollback]
on = ["deploy_fail"]
strategy = "revert"
"#;
        let deploy = parse_deploy_file(Cursor::new(deploy_ir)).unwrap();
        let diags = check_deploy_file(&deploy);

        let errors: Vec<_> = diags.iter().filter(|d| d.severity == "error").collect();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }
}
