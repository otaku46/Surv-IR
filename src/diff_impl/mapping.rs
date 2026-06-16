use super::types::{MappingEntry, MappingFile};
use std::error::Error;
use std::fs;
use std::path::Path;

impl Default for MappingFile {
    fn default() -> Self {
        Self {
            version: "0.1".to_string(),
            entries: Vec::new(),
        }
    }
}

pub fn load_mapping(path: &Path) -> Result<MappingFile, Box<dyn Error>> {
    if !path.exists() {
        return Ok(MappingFile::default());
    }

    let content = fs::read_to_string(path)?;
    let mapping: MappingFile = toml::from_str(&content)?;
    Ok(mapping)
}

pub fn find_by_surv_ref<'a>(mapping: &'a MappingFile, surv_ref: &str) -> Option<&'a MappingEntry> {
    mapping
        .entries
        .iter()
        .find(|entry| entry.surv_ref == surv_ref)
}
