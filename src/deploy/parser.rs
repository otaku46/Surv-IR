use crate::deploy::ast::*;
use crate::simple_toml::{parse_toml, TomlTable, TomlValue};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

#[derive(Debug)]
pub enum ParseError {
    Io(io::Error),
    InvalidFormat(String),
}

impl From<io::Error> for ParseError {
    fn from(err: io::Error) -> Self {
        ParseError::Io(err)
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Io(err) => write!(f, "IO error: {}", err),
            ParseError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

pub fn parse_deploy_file<R: Read>(reader: R) -> Result<DeployFile, ParseError> {
    let raw = parse_toml(BufReader::new(reader))?;

    let mut deploy = DeployFile::default();

    // Parse [deploy.pipeline]
    if let Some(pipeline_table) = get_nested_table(&raw, "deploy", "pipeline") {
        deploy.pipeline = Some(parse_pipeline(pipeline_table));
    }

    // Parse [deploy.target.*]
    if let Some(deploy_table) = raw.get("deploy").and_then(|v| v.as_table()) {
        if let Some(target_table) = deploy_table.get("target").and_then(|v| v.as_table()) {
            for (name, value) in target_table {
                if let Some(table) = value.as_table() {
                    deploy.targets.insert(name.clone(), parse_target(name, table));
                }
            }
        }
    }

    // Parse [deploy.job.*]
    if let Some(deploy_table) = raw.get("deploy").and_then(|v| v.as_table()) {
        if let Some(job_table) = deploy_table.get("job").and_then(|v| v.as_table()) {
            for (name, value) in job_table {
                if let Some(table) = value.as_table() {
                    deploy.jobs.insert(name.clone(), parse_job(name, table));
                }
            }
        }
    }

    // Parse [deploy.artifact.*]
    if let Some(deploy_table) = raw.get("deploy").and_then(|v| v.as_table()) {
        if let Some(artifact_table) = deploy_table.get("artifact").and_then(|v| v.as_table()) {
            for (name, value) in artifact_table {
                if let Some(table) = value.as_table() {
                    deploy
                        .artifacts
                        .insert(name.clone(), parse_artifact(name, table));
                }
            }
        }
    }

    // Parse [deploy.secret.*]
    if let Some(deploy_table) = raw.get("deploy").and_then(|v| v.as_table()) {
        if let Some(secret_table) = deploy_table.get("secret").and_then(|v| v.as_table()) {
            for (name, value) in secret_table {
                if let Some(table) = value.as_table() {
                    deploy.secrets.insert(name.clone(), parse_secret(name, table));
                }
            }
        }
    }

    // Parse [deploy.perm.*]
    if let Some(deploy_table) = raw.get("deploy").and_then(|v| v.as_table()) {
        if let Some(perm_table) = deploy_table.get("perm").and_then(|v| v.as_table()) {
            for (name, value) in perm_table {
                if let Some(table) = value.as_table() {
                    deploy
                        .perms
                        .insert(name.clone(), parse_permission(name, table));
                }
            }
        }
    }

    // Parse [deploy.release]
    if let Some(release_table) = get_nested_table(&raw, "deploy", "release") {
        deploy.release = Some(parse_release(release_table));
    }

    // Parse [deploy.gate]
    if let Some(gate_table) = get_nested_table(&raw, "deploy", "gate") {
        deploy.gate = Some(parse_gate(gate_table));
    }

    // Parse [deploy.rollback]
    if let Some(rollback_table) = get_nested_table(&raw, "deploy", "rollback") {
        deploy.rollback = Some(parse_rollback(rollback_table));
    }

    Ok(deploy)
}

pub fn parse_deploy_file_from_path(path: &Path) -> Result<DeployFile, ParseError> {
    let file = File::open(path)?;
    parse_deploy_file(file)
}

fn get_nested_table<'a>(root: &'a TomlTable, key1: &str, key2: &str) -> Option<&'a TomlTable> {
    root.get(key1)?
        .as_table()?
        .get(key2)?
        .as_table()
}

fn get_string(table: &TomlTable, key: &str) -> String {
    table
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn get_string_array(table: &TomlTable, key: &str) -> Vec<String> {
    match table.get(key) {
        Some(TomlValue::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_pipeline(table: &TomlTable) -> Pipeline {
    Pipeline {
        name: get_string(table, "name"),
        description: get_string(table, "description"),
    }
}

fn parse_target(name: &str, table: &TomlTable) -> Target {
    Target {
        name: name.to_string(),
        kind: get_string(table, "kind"),
        domain: get_string(table, "domain"),
    }
}

fn parse_job(name: &str, table: &TomlTable) -> Job {
    Job {
        name: name.to_string(),
        requires: get_string_array(table, "requires"),
        runs: get_string_array(table, "runs"),
        uses_target: get_string(table, "uses_target"),
        needs_secrets: get_string_array(table, "needs_secrets"),
        uses_perm: get_string(table, "uses_perm"),
        produces: get_string_array(table, "produces"),
        side_effects: get_string_array(table, "side_effects"),
    }
}

fn parse_artifact(name: &str, table: &TomlTable) -> Artifact {
    Artifact {
        name: name.to_string(),
        artifact_type: get_string(table, "type"),
        repo: get_string(table, "repo"),
        tag: get_string(table, "tag"),
    }
}

fn parse_secret(name: &str, table: &TomlTable) -> Secret {
    Secret {
        name: name.to_string(),
        scope: get_string_array(table, "scope"),
    }
}

fn parse_permission(name: &str, table: &TomlTable) -> Permission {
    Permission {
        name: name.to_string(),
        role: get_string(table, "role"),
        allows: get_string_array(table, "allows"),
    }
}

fn parse_release(table: &TomlTable) -> Release {
    Release {
        strategy: get_string(table, "strategy"),
        health_check: get_string(table, "health_check"),
    }
}

fn parse_gate(table: &TomlTable) -> Gate {
    Gate {
        require_manual_approval_for: get_string_array(table, "require_manual_approval_for"),
    }
}

fn parse_rollback(table: &TomlTable) -> Rollback {
    Rollback {
        on: get_string_array(table, "on"),
        strategy: get_string(table, "strategy"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SAMPLE_DEPLOY: &str = r#"
[deploy.pipeline]
name = "webapp"
description = "production deploy pipeline"

[deploy.target.prod]
kind = "production"
domain = "example.com"

[deploy.job.build]
requires = []
runs = ["npm ci", "npm run build"]
produces = ["artifact.image"]

[deploy.job.deploy]
requires = ["job.build"]
runs = ["kubectl apply -f deploy.yaml"]
uses_target = "target.prod"
needs_secrets = ["secret.DB_URL"]
side_effects = ["release"]

[deploy.artifact.image]
type = "docker"
repo = "ghcr.io/acme/app"
tag = "git_sha"

[deploy.secret.DB_URL]
scope = ["target.prod"]

[deploy.release]
strategy = "canary"
health_check = "https://{domain}/healthz"

[deploy.gate]
require_manual_approval_for = ["target.prod"]

[deploy.rollback]
on = ["health_fail", "deploy_fail"]
strategy = "revert_traffic"
"#;

    #[test]
    fn parses_sample_deploy() {
        let deploy = parse_deploy_file(Cursor::new(SAMPLE_DEPLOY)).expect("parse");

        assert!(deploy.pipeline.is_some());
        assert_eq!(deploy.pipeline.unwrap().name, "webapp");

        assert_eq!(deploy.targets.len(), 1);
        assert!(deploy.targets.contains_key("prod"));

        assert_eq!(deploy.jobs.len(), 2);
        assert!(deploy.jobs.contains_key("build"));
        assert!(deploy.jobs.contains_key("deploy"));

        let deploy_job = &deploy.jobs["deploy"];
        assert_eq!(deploy_job.requires, vec!["job.build"]);
        assert_eq!(deploy_job.uses_target, "target.prod");
        assert_eq!(deploy_job.needs_secrets, vec!["secret.DB_URL"]);

        assert!(deploy.release.is_some());
        assert!(deploy.gate.is_some());
        assert!(deploy.rollback.is_some());
    }
}
