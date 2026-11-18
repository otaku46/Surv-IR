use crate::deploy::ast::DeployFile;
use std::collections::{HashMap, HashSet};

pub struct GitLabCIGenerator;

impl GitLabCIGenerator {
    pub fn new() -> Self {
        Self
    }

    pub fn generate(&self, deploy: &DeployFile) -> String {
        let mut output = String::new();

        // Header
        output.push_str("# Generated from Deploy IR by surc\n");
        if let Some(pipeline) = &deploy.pipeline {
            output.push_str(&format!("# Pipeline: {}\n", pipeline.name));
            if !pipeline.description.is_empty() {
                output.push_str(&format!("# {}\n", pipeline.description));
            }
        }
        output.push_str("\n");

        // Determine stages from job graph
        let stages = self.determine_stages(deploy);
        output.push_str("stages:\n");
        for stage in &stages {
            output.push_str(&format!("  - {}\n", stage));
        }
        output.push_str("\n");

        // Global variables for common settings
        output.push_str("variables:\n");
        output.push_str("  GIT_DEPTH: 1\n");
        output.push_str("\n");

        // Build stage-to-jobs mapping
        let job_stages = self.assign_job_stages(deploy);

        // Generate jobs
        for (job_name, job) in &deploy.jobs {
            let default_stage = "deploy".to_string();
            let stage = job_stages.get(job_name).unwrap_or(&default_stage);

            output.push_str(&format!("{}:\n", Self::sanitize_job_name(job_name)));
            output.push_str(&format!("  stage: {}\n", stage));

            // Determine image/runner based on target
            if !job.uses_target.is_empty() {
                let target_name = job
                    .uses_target
                    .strip_prefix("target.")
                    .unwrap_or(&job.uses_target);
                if let Some(target) = deploy.targets.get(target_name) {
                    match target.kind.as_str() {
                        "production" => {
                            output.push_str("  image: ubuntu:latest\n");
                            output.push_str("  tags:\n");
                            output.push_str("    - production\n");
                        }
                        "staging" => {
                            output.push_str("  image: ubuntu:latest\n");
                            output.push_str("  tags:\n");
                            output.push_str("    - staging\n");
                        }
                        _ => {
                            output.push_str("  image: ubuntu:latest\n");
                        }
                    }
                }
            } else {
                output.push_str("  image: ubuntu:latest\n");
            }

            // Add dependencies (needs)
            if !job.requires.is_empty() {
                output.push_str("  needs:\n");
                for req in &job.requires {
                    let dep = req.strip_prefix("job.").unwrap_or(req);
                    output.push_str(&format!("    - {}\n", Self::sanitize_job_name(dep)));
                }
            }

            // Add environment variables for secrets
            if !job.needs_secrets.is_empty() {
                output.push_str("  variables:\n");
                for secret in &job.needs_secrets {
                    let secret_name =
                        Self::sanitize_env_name(secret.strip_prefix("secret.").unwrap_or(secret));
                    output.push_str(&format!("    {}: ${}\n", secret_name, secret_name));
                }
            }

            // Script. Block scalars keep colons, quotes, comments and newlines
            // literal instead of turning them into YAML structure.
            output.push_str("  script:\n");
            for cmd in &job.runs {
                output.push_str("    - |\n");
                if cmd.is_empty() {
                    output.push_str("      \n");
                }
                for line in cmd.lines() {
                    output.push_str(&format!("      {line}\n"));
                }
            }

            // Add manual approval for production
            if !job.uses_target.is_empty() {
                let target_name = job
                    .uses_target
                    .strip_prefix("target.")
                    .unwrap_or(&job.uses_target);
                if let Some(target) = deploy.targets.get(target_name) {
                    if target.kind == "production" && deploy.gate.is_some() {
                        output.push_str("  when: manual\n");
                    }
                }
            }

            // Only run on main branch for production
            if !job.uses_target.is_empty() {
                let target_name = job
                    .uses_target
                    .strip_prefix("target.")
                    .unwrap_or(&job.uses_target);
                if let Some(target) = deploy.targets.get(target_name) {
                    if target.kind == "production" {
                        output.push_str("  only:\n");
                        output.push_str("    - main\n");
                    }
                }
            }

            // Add artifacts if job produces any
            if !job.produces.is_empty() {
                output.push_str("  artifacts:\n");
                output.push_str("    paths:\n");
                for artifact in &job.produces {
                    let artifact_name = artifact.strip_prefix("artifact.").unwrap_or(artifact);
                    output.push_str(&format!("      - build/{}\n", artifact_name));
                }
            }

            output.push_str("\n");
        }

        output
    }

    fn determine_stages(&self, deploy: &DeployFile) -> Vec<String> {
        let depths = job_depths(deploy);
        let max_depth = depths.values().copied().max().unwrap_or(0);
        (0..=max_depth).map(stage_name).collect()
    }

    fn assign_job_stages(&self, deploy: &DeployFile) -> HashMap<String, String> {
        job_depths(deploy)
            .into_iter()
            .map(|(job, depth)| (job, stage_name(depth)))
            .collect()
    }

    fn sanitize_job_name(name: &str) -> String {
        name.replace('-', "_").replace('.', "_")
    }

    fn sanitize_env_name(name: &str) -> String {
        let mut name = name.to_uppercase().replace('-', "_").replace('.', "_");
        if name.is_empty() {
            name.push_str("SECRET");
        }
        if name.starts_with(|ch: char| ch.is_ascii_digit()) {
            name.insert_str(0, "SECRET_");
        }
        name
    }
}

impl Default for GitLabCIGenerator {
    fn default() -> Self {
        Self::new()
    }
}

fn job_depths(deploy: &DeployFile) -> HashMap<String, usize> {
    let mut memo = HashMap::new();
    let mut visiting = HashSet::new();
    for name in deploy.jobs.keys() {
        depth_of(name, deploy, &mut memo, &mut visiting);
    }
    memo
}

fn depth_of(
    name: &str,
    deploy: &DeployFile,
    memo: &mut HashMap<String, usize>,
    visiting: &mut HashSet<String>,
) -> usize {
    if let Some(depth) = memo.get(name) {
        return *depth;
    }
    if !visiting.insert(name.to_string()) {
        return 0;
    }
    let depth = deploy
        .jobs
        .get(name)
        .map(|job| {
            job.requires
                .iter()
                .map(|requirement| requirement.strip_prefix("job.").unwrap_or(requirement))
                .filter(|dependency| deploy.jobs.contains_key(*dependency))
                .map(|dependency| 1 + depth_of(dependency, deploy, memo, visiting))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    visiting.remove(name);
    memo.insert(name.to_string(), depth);
    depth
}

fn stage_name(depth: usize) -> String {
    match depth {
        0 => "build".to_string(),
        1 => "test".to_string(),
        2 => "deploy".to_string(),
        depth => format!("deploy_{depth}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::ast::*;
    use std::collections::BTreeMap;

    #[test]
    fn generates_basic_pipeline() {
        let mut deploy = DeployFile::default();
        deploy.pipeline = Some(Pipeline {
            name: "test-pipeline".to_string(),
            description: "Test deployment".to_string(),
        });

        let mut jobs = BTreeMap::new();
        jobs.insert(
            "build".to_string(),
            Job {
                name: "build".to_string(),
                requires: Vec::new(),
                runs: vec!["npm ci".to_string(), "npm run build".to_string()],
                uses_target: String::new(),
                needs_secrets: Vec::new(),
                uses_perm: String::new(),
                produces: Vec::new(),
                side_effects: Vec::new(),
            },
        );
        deploy.jobs = jobs;

        let generator = GitLabCIGenerator::new();
        let yaml = generator.generate(&deploy);

        assert!(yaml.contains("stages:"));
        assert!(yaml.contains("build:"));
        assert!(yaml.contains("npm ci"));
        assert!(yaml.contains("npm run build"));
    }

    #[test]
    fn generates_job_dependencies() {
        let mut deploy = DeployFile::default();

        let mut jobs = BTreeMap::new();
        jobs.insert(
            "build".to_string(),
            Job {
                name: "build".to_string(),
                requires: Vec::new(),
                runs: vec!["npm run build".to_string()],
                uses_target: String::new(),
                needs_secrets: Vec::new(),
                uses_perm: String::new(),
                produces: Vec::new(),
                side_effects: Vec::new(),
            },
        );
        jobs.insert(
            "deploy".to_string(),
            Job {
                name: "deploy".to_string(),
                requires: vec!["job.build".to_string()],
                runs: vec!["kubectl apply -f deploy.yaml".to_string()],
                uses_target: String::new(),
                needs_secrets: Vec::new(),
                uses_perm: String::new(),
                produces: Vec::new(),
                side_effects: Vec::new(),
            },
        );
        deploy.jobs = jobs;

        let generator = GitLabCIGenerator::new();
        let yaml = generator.generate(&deploy);

        assert!(yaml.contains("needs:"));
        assert!(yaml.contains("- build"));
    }

    #[test]
    fn adds_manual_approval_for_production() {
        let mut deploy = DeployFile::default();
        deploy.gate = Some(crate::deploy::ast::Gate {
            require_manual_approval_for: vec!["target.prod".to_string()],
        });

        let mut targets = BTreeMap::new();
        targets.insert(
            "prod".to_string(),
            crate::deploy::ast::Target {
                name: "prod".to_string(),
                kind: "production".to_string(),
                domain: "example.com".to_string(),
            },
        );
        deploy.targets = targets;

        let mut jobs = BTreeMap::new();
        jobs.insert(
            "deploy_prod".to_string(),
            Job {
                name: "deploy_prod".to_string(),
                requires: Vec::new(),
                runs: vec!["kubectl apply".to_string()],
                uses_target: "target.prod".to_string(),
                needs_secrets: Vec::new(),
                uses_perm: String::new(),
                produces: Vec::new(),
                side_effects: Vec::new(),
            },
        );
        deploy.jobs = jobs;

        let generator = GitLabCIGenerator::new();
        let yaml = generator.generate(&deploy);

        assert!(yaml.contains("when: manual"));
        assert!(yaml.contains("only:"));
        assert!(yaml.contains("- main"));
    }
}
