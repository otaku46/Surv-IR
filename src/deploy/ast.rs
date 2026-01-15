use serde::Serialize;
use std::collections::BTreeMap;

/// Deploy IR file representation
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeployFile {
    pub pipeline: Option<Pipeline>,
    pub targets: BTreeMap<String, Target>,
    pub jobs: BTreeMap<String, Job>,
    pub artifacts: BTreeMap<String, Artifact>,
    pub secrets: BTreeMap<String, Secret>,
    pub perms: BTreeMap<String, Permission>,
    pub release: Option<Release>,
    pub gate: Option<Gate>,
    pub rollback: Option<Rollback>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pipeline {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub name: String,
    pub kind: String,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub name: String,
    pub requires: Vec<String>,
    pub runs: Vec<String>,
    pub uses_target: String,
    pub needs_secrets: Vec<String>,
    pub uses_perm: String,
    pub produces: Vec<String>,
    pub side_effects: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Artifact {
    pub name: String,
    pub artifact_type: String,
    pub repo: String,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Secret {
    pub name: String,
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Permission {
    pub name: String,
    pub role: String,
    pub allows: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Release {
    pub strategy: String,
    pub health_check: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Gate {
    pub require_manual_approval_for: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rollback {
    pub on: Vec<String>,
    pub strategy: String,
}

impl Default for Job {
    fn default() -> Self {
        Self {
            name: String::new(),
            requires: Vec::new(),
            runs: Vec::new(),
            uses_target: String::new(),
            needs_secrets: Vec::new(),
            uses_perm: String::new(),
            produces: Vec::new(),
            side_effects: Vec::new(),
        }
    }
}
