use crate::manifest::Manifest;
use crate::parser::parse_file;
use crate::project::ProjectAST;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn load_manifest(path: &Path) -> Result<Manifest, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)?;
    let manifest: Manifest = toml::from_str(&text)?;
    Ok(manifest)
}

fn collect_ir_files(ir_root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for entry in WalkDir::new(ir_root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "toml" {
                    result.push(path.to_path_buf());
                }
            }
        }
    }
    Ok(result)
}

pub fn load_project(manifest_path: &Path) -> Result<ProjectAST, Box<dyn std::error::Error>> {
    let manifest = load_manifest(manifest_path)?;
    let manifest_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let ir_root = manifest_dir.join(&manifest.paths.ir_root);

    let files = collect_ir_files(&ir_root)?;
    let mut parsed_files = Vec::new();

    for path in files {
        match parse_file(&path) {
            Ok(file) => parsed_files.push((path, file)),
            Err(err) => return Err(Box::new(err)),
        }
    }

    Ok(ProjectAST::from_files(parsed_files))
}
