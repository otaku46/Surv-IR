use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub project: ProjectSection,
    pub paths: PathsSection,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub files: HashMap<String, FileEntry>,
    #[serde(default)]
    pub packages: HashMap<String, PackageSection>,
    #[serde(default)]
    pub diff: DiffConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct DiffConfig {
    /// Explicit implementation types for semantic IR types.
    ///
    /// Example:
    /// ```toml
    /// [diff.type_map]
    /// any = ["Any", "serde_json::Value"]
    /// ```
    #[serde(default)]
    pub type_map: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FileEntry {
    #[serde(default)]
    pub description: String,
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
