use crate::ast::SurvFile;
use crate::diagnostic::Diagnostic;
use crate::manifest::{Manifest, PackageSection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PackageAssignment {
    pub file_path: PathBuf,
    pub package: String,
}

pub fn assign_packages_to_files(
    manifest: &Manifest,
    project_root: &Path,
    files: &[(PathBuf, SurvFile)],
) -> (Vec<PackageAssignment>, Vec<Diagnostic>) {
    let mut assignments = Vec::new();
    let mut diagnostics = Vec::new();

    if manifest.packages.is_empty() {
        for (path, _) in files {
            assignments.push(PackageAssignment {
                file_path: path.clone(),
                package: "default".to_string(),
            });
        }
        return (assignments, diagnostics);
    }

    let mut package_roots: HashMap<String, PathBuf> = HashMap::new();
    for (name, pkg) in &manifest.packages {
        let root = resolve_package_root(project_root, pkg);
        package_roots.insert(name.clone(), root);
    }

    for (path, file) in files {
        if let Some(declared) = &file.package {
            if let Some(root) = package_roots.get(declared) {
                if path.starts_with(root) {
                    assignments.push(PackageAssignment {
                        file_path: path.clone(),
                        package: declared.clone(),
                    });
                } else {
                    diagnostics.push(Diagnostic {
                        severity: "error".into(),
                        kind: "E_PACKAGE_ROOT_MISMATCH".into(),
                        message: format!(
                            "file {} is not inside the root of package '{}'",
                            path.display(),
                            declared
                        ),
                        location: path.display().to_string(),
                    });
                }
            } else {
                diagnostics.push(Diagnostic {
                    severity: "error".into(),
                    kind: "E_PACKAGE_UNKNOWN".into(),
                    message: format!(
                        "file {} declares unknown package '{}'",
                        path.display(),
                        declared
                    ),
                    location: path.display().to_string(),
                });
            }
            continue;
        }

        let matching: Vec<_> = package_roots
            .iter()
            .filter(|(_, root)| path.starts_with(root))
            .map(|(name, _)| name.clone())
            .collect();

        match matching.len() {
            0 => diagnostics.push(Diagnostic {
                severity: "error".into(),
                kind: "E_PACKAGE_UNASSIGNED".into(),
                message: format!(
                    "file {} does not fall under any package root and has no package header",
                    path.display()
                ),
                location: path.display().to_string(),
            }),
            1 => assignments.push(PackageAssignment {
                file_path: path.clone(),
                package: matching[0].clone(),
            }),
            _ => diagnostics.push(Diagnostic {
                severity: "error".into(),
                kind: "E_PACKAGE_AMBIGUOUS".into(),
                message: format!(
                    "file {} matches multiple package roots: {}",
                    path.display(),
                    matching.join(", ")
                ),
                location: path.display().to_string(),
            }),
        }
    }

    (assignments, diagnostics)
}

fn resolve_package_root(project_root: &Path, pkg: &PackageSection) -> PathBuf {
    let path = Path::new(&pkg.root);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_surv_file;
    use std::io::Cursor;

    fn file(path: &str, text: &str) -> (PathBuf, SurvFile) {
        let parsed = parse_surv_file(Cursor::new(text)).expect("parse");
        (PathBuf::from(path), parsed)
    }

    fn manifest_with_packages(packages: &[(&str, &str)]) -> Manifest {
        let mut map = HashMap::new();
        for (name, root) in packages {
            map.insert(
                name.to_string(),
                PackageSection {
                    root: root.to_string(),
                    namespace: None,
                    depends: Vec::new(),
                },
            );
        }
        Manifest {
            project: crate::manifest::ProjectSection {
                name: "test".into(),
            },
            paths: crate::manifest::PathsSection {
                ir_root: "ir".into(),
            },
            packages: map,
        }
    }

    #[test]
    fn assigns_declared_package() {
        let manifest = manifest_with_packages(&[("users", "src/users")]);
        let files = vec![file(
            "project/src/users/a.toml",
            r#"
package = "users"
[schema.a]
kind = "node"
type = "A"
"#,
        )];
        let (assignments, diags) =
            assign_packages_to_files(&manifest, Path::new("project"), &files);
        assert!(diags.is_empty());
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].package, "users");
    }

    #[test]
    fn reports_unknown_package() {
        let manifest = manifest_with_packages(&[("users", "src/users")]);
        let files = vec![file(
            "project/src/users/a.toml",
            r#"
package = "auth"
[schema.a]
kind = "node"
type = "A"
"#,
        )];
        let (_assignments, diags) =
            assign_packages_to_files(&manifest, Path::new("project"), &files);
        assert!(diags.iter().any(|d| d.kind == "E_PACKAGE_UNKNOWN"));
    }

    #[test]
    fn assigns_by_root() {
        let manifest = manifest_with_packages(&[("users", "src/users"), ("auth", "src/auth")]);
        let files = vec![
            file(
                "project/src/users/a.toml",
                r#"
[schema.a]
kind = "node"
type = "A"
"#,
            ),
            file(
                "project/src/auth/b.toml",
                r#"
[schema.b]
kind = "node"
type = "B"
"#,
            ),
        ];
        let (assignments, diags) =
            assign_packages_to_files(&manifest, Path::new("project"), &files);
        assert!(diags.is_empty());
        assert_eq!(assignments.len(), 2);
    }
}
