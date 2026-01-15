use crate::deploy::ast::DeployFile;
use std::collections::HashMap;

pub struct GitHubActionsGenerator;

impl GitHubActionsGenerator {
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

        output.push_str("name: Deploy Pipeline\n\n");
        output.push_str("on:\n");
        output.push_str("  push:\n");
        output.push_str("    branches: [main]\n");
        output.push_str("  workflow_dispatch:\n\n");

        // Collect all secrets referenced
        let mut all_secrets: Vec<String> = Vec::new();
        for job in deploy.jobs.values() {
            for secret in &job.needs_secrets {
                let secret_name = secret.strip_prefix("secret.").unwrap_or(secret);
                if !all_secrets.contains(&secret_name.to_string()) {
                    all_secrets.push(secret_name.to_string());
                }
            }
        }

        output.push_str("jobs:\n");

        // Generate jobs in topological order
        for (job_name, job) in &deploy.jobs {
            output.push_str(&format!("  {}:\n", Self::sanitize_job_name(job_name)));

            // Determine runner based on target
            let runner = if !job.uses_target.is_empty() {
                let target_name = job.uses_target.strip_prefix("target.").unwrap_or(&job.uses_target);
                if let Some(target) = deploy.targets.get(target_name) {
                    match target.kind.as_str() {
                        "production" => "ubuntu-latest",
                        "staging" => "ubuntu-latest",
                        _ => "ubuntu-latest",
                    }
                } else {
                    "ubuntu-latest"
                }
            } else {
                "ubuntu-latest"
            };
            output.push_str(&format!("    runs-on: {}\n", runner));

            // Add environment for production/staging
            if !job.uses_target.is_empty() {
                let target_name = job.uses_target.strip_prefix("target.").unwrap_or(&job.uses_target);
                if let Some(target) = deploy.targets.get(target_name) {
                    output.push_str(&format!("    environment: {}\n", target.kind));
                }
            }

            // Add dependencies (needs)
            if !job.requires.is_empty() {
                output.push_str("    needs:");
                if job.requires.len() == 1 {
                    let dep = job.requires[0].strip_prefix("job.").unwrap_or(&job.requires[0]);
                    output.push_str(&format!(" {}\n", Self::sanitize_job_name(dep)));
                } else {
                    output.push_str("\n");
                    for req in &job.requires {
                        let dep = req.strip_prefix("job.").unwrap_or(req);
                        output.push_str(&format!("      - {}\n", Self::sanitize_job_name(dep)));
                    }
                }
            }

            // Steps
            output.push_str("    steps:\n");
            output.push_str("      - name: Checkout code\n");
            output.push_str("        uses: actions/checkout@v4\n\n");

            // Add each command as a step
            for (i, cmd) in job.runs.iter().enumerate() {
                output.push_str(&format!("      - name: {}\n", Self::generate_step_name(cmd, i)));
                output.push_str("        run: |\n");
                output.push_str(&format!("          {}\n", cmd));

                // Add environment variables for secrets
                if !job.needs_secrets.is_empty() {
                    output.push_str("        env:\n");
                    for secret in &job.needs_secrets {
                        let secret_name = secret.strip_prefix("secret.").unwrap_or(secret);
                        output.push_str(&format!(
                            "          {}: ${{{{ secrets.{} }}}}\n",
                            secret_name.to_uppercase(),
                            secret_name.to_uppercase()
                        ));
                    }
                }
                output.push_str("\n");
            }

            // Add approval requirement for production jobs
            if !job.uses_target.is_empty() {
                let target_name = job.uses_target.strip_prefix("target.").unwrap_or(&job.uses_target);
                if let Some(target) = deploy.targets.get(target_name) {
                    if target.kind == "production" && deploy.gate.is_some() {
                        output.push_str("      # Production deployment requires manual approval via GitHub environment protection rules\n");
                    }
                }
            }

            output.push_str("\n");
        }

        output
    }

    fn build_job_graph(&self, deploy: &DeployFile) -> HashMap<String, Vec<String>> {
        let mut graph = HashMap::new();
        for (job_name, job) in &deploy.jobs {
            let deps: Vec<String> = job
                .requires
                .iter()
                .map(|r| r.strip_prefix("job.").unwrap_or(r).to_string())
                .collect();
            graph.insert(job_name.clone(), deps);
        }
        graph
    }

    fn sanitize_job_name(name: &str) -> String {
        name.replace('-', "_").replace('.', "_")
    }

    fn generate_step_name(cmd: &str, index: usize) -> String {
        // Try to extract a meaningful name from the command
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if !parts.is_empty() {
            match parts[0] {
                "npm" if parts.len() > 1 => format!("Run npm {}", parts[1]),
                "docker" if parts.len() > 1 => format!("Run docker {}", parts[1]),
                "kubectl" if parts.len() > 1 => format!("Run kubectl {}", parts[1]),
                "cargo" if parts.len() > 1 => format!("Run cargo {}", parts[1]),
                "go" if parts.len() > 1 => format!("Run go {}", parts[1]),
                cmd => format!("Run {}", cmd),
            }
        } else {
            format!("Step {}", index + 1)
        }
    }
}

impl Default for GitHubActionsGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::ast::*;
    use std::collections::BTreeMap;

    #[test]
    fn generates_basic_workflow() {
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

        let generator = GitHubActionsGenerator::new();
        let yaml = generator.generate(&deploy);

        assert!(yaml.contains("name: Deploy Pipeline"));
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

        let generator = GitHubActionsGenerator::new();
        let yaml = generator.generate(&deploy);

        assert!(yaml.contains("needs: build"));
    }
}
