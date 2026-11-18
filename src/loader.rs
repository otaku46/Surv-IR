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

pub(crate) fn collect_ir_files(ir_root: &Path) -> io::Result<Vec<PathBuf>> {
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
    result.sort();
    Ok(result)
}

fn manifest_files(
    manifest_dir: &Path,
    ir_root: &Path,
    manifest: &Manifest,
) -> io::Result<Vec<PathBuf>> {
    if manifest.files.is_empty() {
        return collect_ir_files(ir_root);
    }
    let mut files: Vec<PathBuf> = manifest
        .files
        .keys()
        .map(|name| {
            let relative = ir_root.join(name);
            if relative.is_file() {
                relative
            } else {
                manifest_dir.join(name)
            }
        })
        .collect();
    files.sort();
    Ok(files)
}

fn is_non_ir_source(path: &Path) -> bool {
    let Ok(source) = fs::read_to_string(path) else {
        return false;
    };
    if source.contains("surv-behavior") && crate::behavior::loader::has_behavior_metadata(&source) {
        return true;
    }
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return trimmed == "[deploy]"
            || trimmed.starts_with("[deploy.")
            || trimmed == "[behavior]"
            || trimmed.starts_with("[behavior.")
            || trimmed == "[project]"
            || trimmed == "[paths]"
            || trimmed.starts_with("[packages.");
    }
    false
}

pub fn load_project(manifest_path: &Path) -> Result<ProjectAST, Box<dyn std::error::Error>> {
    let manifest = load_manifest(manifest_path)?;
    load_project_with_manifest(manifest_path, &manifest)
}

pub(crate) fn load_project_with_manifest(
    manifest_path: &Path,
    manifest: &Manifest,
) -> Result<ProjectAST, Box<dyn std::error::Error>> {
    load_project_with_parser(manifest_path, manifest, |path| Ok(parse_file(path)?))
}

pub(crate) fn load_project_with_parser(
    manifest_path: &Path,
    manifest: &Manifest,
    parse: impl Fn(&Path) -> Result<crate::SurvFile, Box<dyn std::error::Error>>,
) -> Result<ProjectAST, Box<dyn std::error::Error>> {
    let manifest_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let ir_root = manifest_dir.join(&manifest.paths.ir_root);

    let files = manifest_files(&manifest_dir, &ir_root, manifest)?;
    let mut parsed_files = Vec::new();

    for path in files {
        if path.file_name().is_some_and(|name| name == "surv.toml")
            || crate::behavior::loader::has_behavior_suffix(&path)
            || is_non_ir_source(&path)
        {
            continue;
        }
        let file = parse(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        parsed_files.push((path, file));
    }

    Ok(ProjectAST::from_files(parsed_files))
}
