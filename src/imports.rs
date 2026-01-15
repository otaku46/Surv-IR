use crate::ast::SurvFile;
use crate::diagnostic::Diagnostic;
use crate::manifest::Manifest;
use crate::package::PackageAssignment;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ImportEntry {
    pub package: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FileImportContext {
    pub file_path: PathBuf,
    pub self_package: String,
    pub namespace: Option<String>,
    pub imports: Vec<ImportEntry>,
}

pub fn parse_imports_with_alias(
    manifest: &Manifest,
    assignments: &[PackageAssignment],
    files: &[(PathBuf, SurvFile)],
) -> (Vec<FileImportContext>, Vec<Diagnostic>) {
    let assignment_map: HashMap<_, _> = assignments
        .iter()
        .map(|assign| (assign.file_path.clone(), assign.package.clone()))
        .collect();
    let known_packages: HashMap<_, _> = manifest
        .packages
        .iter()
        .map(|(name, _)| (name.clone(), ()))
        .collect();

    let mut contexts = Vec::new();
    let mut diagnostics = Vec::new();

    for (path, file) in files {
        let self_package = assignment_map
            .get(path)
            .cloned()
            .or_else(|| file.package.clone())
            .unwrap_or_else(|| "default".to_string());
        let mut imports = Vec::new();

        for raw in &file.imports {
            match parse_import_entry(&raw.target) {
                Ok((package_name, alias)) => {
                    if !known_packages.is_empty() && !known_packages.contains_key(&package_name) {
                        diagnostics.push(Diagnostic {
                            severity: "error".into(),
                            kind: "E_IMPORT_UNKNOWN_PACKAGE".into(),
                            message: format!("Unknown import package '{}'", package_name),
                            location: path.display().to_string(),
                        });
                        continue;
                    }
                    imports.push(ImportEntry {
                        package: package_name,
                        alias: alias.or_else(|| raw.alias.clone()),
                    });
                }
                Err(kind) => diagnostics.push(Diagnostic {
                    severity: "error".into(),
                    kind,
                    message: format!("Invalid import syntax '{}'", raw.target),
                    location: path.display().to_string(),
                }),
            }
        }

        contexts.push(FileImportContext {
            file_path: path.clone(),
            self_package,
            namespace: file.namespace.clone(),
            imports,
        });
    }

    (contexts, diagnostics)
}

fn parse_import_entry(raw: &str) -> Result<(String, Option<String>), String> {
    let parts: Vec<_> = raw.split_whitespace().collect();
    match parts.as_slice() {
        [package] => Ok((package.to_string(), None)),
        [package, keyword, alias] if keyword.eq_ignore_ascii_case("as") => {
            Ok((package.to_string(), Some(alias.to_string())))
        }
        _ => Err("E_IMPORT_SYNTAX".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, PathsSection, ProjectSection};
    use crate::parser::parse_surv_file;
    use std::io::Cursor;

    fn manifest_with_packages(packages: &[&str]) -> Manifest {
        let mut map = HashMap::new();
        for name in packages {
            map.insert(
                name.to_string(),
                crate::manifest::PackageSection {
                    root: "dummy".into(),
                    namespace: None,
                    depends: Vec::new(),
                },
            );
        }
        Manifest {
            project: ProjectSection {
                name: "test".into(),
            },
            paths: PathsSection {
                ir_root: "ir".into(),
            },
            packages: map,
        }
    }

    fn file(path: &str, text: &str) -> (PathBuf, SurvFile) {
        let parsed = parse_surv_file(Cursor::new(text)).expect("parse");
        (PathBuf::from(path), parsed)
    }

    #[test]
    fn parses_simple_import() {
        let manifest = manifest_with_packages(&["auth"]);
        let files = vec![file(
            "a.toml",
            r#"
import = ["auth"]
[schema.user]
kind = "node"
type = "User"
"#,
        )];
        let assignments = vec![PackageAssignment {
            file_path: PathBuf::from("a.toml"),
            package: "users".into(),
        }];
        let (contexts, diags) = parse_imports_with_alias(&manifest, &assignments, &files);
        assert!(diags.is_empty());
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].imports.len(), 1);
        assert_eq!(contexts[0].imports[0].package, "auth");
    }

    #[test]
    fn rejects_unknown_package() {
        let manifest = manifest_with_packages(&["users"]);
        let files = vec![file(
            "a.toml",
            r#"
import = ["auth"]
[schema.user]
kind = "node"
type = "User"
"#,
        )];
        let assignments = vec![PackageAssignment {
            file_path: PathBuf::from("a.toml"),
            package: "users".into(),
        }];
        let (_contexts, diags) = parse_imports_with_alias(&manifest, &assignments, &files);
        assert!(diags.iter().any(|d| d.kind == "E_IMPORT_UNKNOWN_PACKAGE"));
    }

    #[test]
    fn parses_alias_import() {
        let manifest = manifest_with_packages(&["users"]);
        let files = vec![file(
            "a.toml",
            r#"
import = ["users as u"]
[schema.user]
kind = "node"
type = "User"
"#,
        )];
        let assignments = vec![PackageAssignment {
            file_path: PathBuf::from("a.toml"),
            package: "core".into(),
        }];
        let (contexts, diags) = parse_imports_with_alias(&manifest, &assignments, &files);
        assert!(diags.is_empty());
        assert_eq!(contexts[0].imports[0].alias.as_deref(), Some("u"));
    }
}
