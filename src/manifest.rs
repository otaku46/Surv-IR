use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub project: ProjectSection,
    pub paths: PathsSection,
    #[serde(default)]
    pub packages: HashMap<String, PackageSection>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectSection {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct PathsSection {
    pub ir_root: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PackageSection {
    pub root: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
}
