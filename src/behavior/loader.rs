use super::model::{BehaviorDiagnostic, BehaviorFileAST};
use crate::imports::parse_imports_with_alias;
use crate::loader::{collect_ir_files, load_project_with_manifest};
use crate::project::ProjectAST;
use crate::project_checker::prepare_project;
use crate::symbol::{check_package_dependency, reference_candidates};
use crate::{FileImportContext, ImportDecl, Manifest, PackageAssignment, SurvFile, SymbolTable};
use crate::{Section, SymbolKind};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct BehaviorSource {
    pub path: PathBuf,
    pub content: String,
    pub manifest_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BehaviorVerificationConfig {
    #[serde(skip)]
    pub behavior_root: Option<PathBuf>,
    #[serde(rename = "backend")]
    pub default_backend: String,
    pub default_max_states: usize,
    pub default_max_depth: usize,
}

impl Default for BehaviorVerificationConfig {
    fn default() -> Self {
        Self {
            behavior_root: None,
            default_backend: "internal".into(),
            default_max_states: 50_000,
            default_max_depth: 100,
        }
    }
}

pub struct ProjectContext {
    pub manifest: Option<Manifest>,
    pub verification: BehaviorVerificationConfig,
    pub project_ast_loaded: bool,
    pub symbol_table: SymbolTable,
    structural: Option<ProjectAST>,
    structural_contexts: Vec<FileImportContext>,
    pub(super) manifest_path: Option<PathBuf>,
    pub(super) diagnostics: Vec<BehaviorDiagnostic>,
}

impl ProjectContext {
    pub(super) fn check_field_path(&self, schema: &str, path: &str) -> Result<(), String> {
        let mut entry = self
            .symbol_table
            .entries
            .iter()
            .find(|entry| entry.fq_name == schema)
            .ok_or("missing schema")?;
        let mut parts = path.split('.').peekable();
        while let Some(part) = parts.next() {
            let (file, context) = self
                .structural
                .as_ref()
                .and_then(|project| {
                    project
                        .files
                        .iter()
                        .zip(&self.structural_contexts)
                        .find(|((file, _), _)| *file == entry.file)
                })
                .ok_or("missing Structural IR schema source")?;
            let ty = file
                .1
                .sections
                .iter()
                .find_map(|section| match section {
                    Section::Schema(schema) if schema.name == entry.local_name => {
                        schema.fields.get(part)
                    }
                    _ => None,
                })
                .ok_or_else(|| format!("unknown Structural IR field '{part}' in path '{path}'"))?;
            if parts.peek().is_some() {
                let matches =
                    reference_candidates(&self.symbol_table, SymbolKind::Schema, ty, context);
                let [nested] = matches.as_slice() else {
                    return Err(format!(
                        "field '{part}' in '{path}' must reference a record schema"
                    ));
                };
                if let Some(err) =
                    check_package_dependency(self.manifest.as_ref(), context, &nested.package)
                {
                    return Err(err.message);
                }
                entry = nested;
            }
        }
        Ok(())
    }
}

pub(crate) fn has_behavior_suffix(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".behavior.toml"))
}

pub(crate) fn has_behavior_metadata(source: &str) -> bool {
    let Ok(table) = source.parse::<toml::Table>() else {
        return false;
    };
    table
        .get("meta")
        .and_then(|meta| meta.get("ir"))
        .and_then(toml::Value::as_str)
        == Some("surv-behavior")
}

pub fn looks_like_behavior(path: &Path) -> bool {
    has_behavior_suffix(path)
        || std::fs::read_to_string(path).is_ok_and(|content| {
            content.contains("surv-behavior") && has_behavior_metadata(&content)
        })
}

pub(super) fn find_manifest(source: &Path) -> Option<PathBuf> {
    source
        .ancestors()
        .skip(1)
        .map(|parent| parent.join("surv.toml"))
        .find(|candidate| candidate.is_file())
}

pub(super) fn load_behavior_project_context(
    source: &BehaviorSource,
) -> Result<ProjectContext, BehaviorDiagnostic> {
    let manifest_path = source
        .manifest_path
        .clone()
        .or_else(|| find_manifest(&source.path));
    load_project_context(manifest_path.as_deref()).map_err(|message| {
        let location = manifest_path
            .as_deref()
            .unwrap_or(&source.path)
            .display()
            .to_string();
        BehaviorDiagnostic::error("E_BEHAVIOR_PROJECT", location, message)
    })
}

pub(super) fn load_project_context(path: Option<&Path>) -> Result<ProjectContext, String> {
    let Some(path) = path else {
        return Ok(ProjectContext {
            manifest: None,
            verification: BehaviorVerificationConfig::default(),
            project_ast_loaded: false,
            symbol_table: SymbolTable::default(),
            structural: None,
            structural_contexts: Vec::new(),
            manifest_path: None,
            diagnostics: Vec::new(),
        });
    };
    let canonical_path = path
        .canonicalize()
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let content = std::fs::read_to_string(&canonical_path).map_err(|err| err.to_string())?;
    let mut table = content
        .parse::<toml::Table>()
        .map_err(|err| err.to_string())?;
    let mut verification: BehaviorVerificationConfig = table
        .remove("verification")
        .map(toml::Value::try_into)
        .transpose()
        .map_err(|err| err.to_string())?
        .unwrap_or_default();
    if let Some(root) = table
        .get_mut("paths")
        .and_then(toml::Value::as_table_mut)
        .and_then(|paths| paths.remove("behavior_root"))
    {
        verification.behavior_root = Some(PathBuf::from(
            root.as_str()
                .ok_or("paths.behavior_root must be a string")?,
        ));
    }
    if !matches!(
        verification.default_backend.as_str(),
        "internal" | "tla+" | "smt"
    ) {
        return Err("verification.backend must be internal, smt, or tla+".into());
    }
    if verification.default_max_states == 0 {
        return Err("verification.default_max_states must be positive".into());
    }
    let manifest: Manifest = table.try_into().map_err(|err| err.to_string())?;
    let mut project =
        load_project_with_manifest(&canonical_path, &manifest).map_err(|err| err.to_string())?;
    let root = canonical_path.parent().unwrap();
    let (structural_contexts, symbol_table, diagnostics) =
        prepare_project(&mut project, Some(&manifest), root);
    Ok(ProjectContext {
        manifest: Some(manifest),
        verification,
        project_ast_loaded: true,
        symbol_table,
        structural: Some(project),
        structural_contexts,
        manifest_path: Some(canonical_path),
        diagnostics: diagnostics.into_iter().map(structural_diag).collect(),
    })
}

pub(super) fn behavior_paths(project: &ProjectContext) -> Result<Vec<PathBuf>, String> {
    let (Some(manifest), Some(path)) = (&project.manifest, &project.manifest_path) else {
        return Ok(Vec::new());
    };
    let root = path.parent().unwrap();
    let mut files =
        collect_ir_files(&root.join(&manifest.paths.ir_root)).map_err(|err| err.to_string())?;
    if let Some(behavior_root) = &project.verification.behavior_root {
        files.extend(collect_ir_files(&root.join(behavior_root)).map_err(|err| err.to_string())?);
    }
    files.retain(|path| looks_like_behavior(path));
    files.sort();
    files.dedup();
    Ok(files)
}

fn structural_diag(diag: crate::Diagnostic) -> BehaviorDiagnostic {
    BehaviorDiagnostic {
        code: diag.kind,
        severity: diag.severity,
        message: diag.message,
        location: diag.location,
        related_symbol: None,
    }
}

pub(super) fn parse_behavior_file(
    source: &BehaviorSource,
    project: &ProjectContext,
) -> Result<(BehaviorFileAST, FileImportContext, Vec<BehaviorDiagnostic>), String> {
    let ast: BehaviorFileAST = toml::from_str(&source.content).map_err(|err| err.to_string())?;
    if ast.raw_behaviors.is_empty() {
        return Err("expected at least one [behavior.<name>] table".into());
    }
    let path = source
        .path
        .canonicalize()
        .unwrap_or_else(|_| source.path.clone());
    let manifest = project.manifest.as_ref();
    let package = if let Some(declared) = &ast.package {
        if manifest.is_some_and(|manifest| {
            !manifest.packages.contains_key(declared)
                && !(manifest.packages.is_empty() && declared == "default")
        }) {
            return Err(format!("unknown package '{declared}'"));
        }
        declared.clone()
    } else if let (Some(manifest), Some(location)) = (manifest, &project.manifest_path) {
        let root = location.parent().unwrap();
        let matching: Vec<_> = manifest
            .packages
            .iter()
            .filter(|(_, package)| path.starts_with(root.join(&package.root)))
            .map(|(name, _)| name)
            .collect();
        match matching.as_slice() {
            [name] => (*name).clone(),
            [] if manifest.packages.len() == 1 => manifest.packages.keys().next().unwrap().clone(),
            [] if manifest.packages.is_empty() => "default".into(),
            [] if ast.imports.is_empty()
                && !ast.raw_behaviors.values().any(|model| {
                    model.owner.is_some()
                        || model
                            .action
                            .values()
                            .any(|action| action.reference.is_some() || !action.refs.is_empty())
                }) =>
            {
                "default".into()
            }
            [] => return Err(
                "set package explicitly to resolve references in a multi-package Behavior project"
                    .into(),
            ),
            _ => {
                return Err(
                    "multiple package roots match Behavior file; set package explicitly".into(),
                )
            }
        }
    } else {
        "default".into()
    };
    let mut context = FileImportContext {
        file_path: path.clone(),
        self_package: package.clone(),
        namespace: ast.namespace.clone().or_else(|| {
            manifest
                .and_then(|m| m.packages.get(&package))
                .and_then(|p| p.namespace.clone())
        }),
        imports: Vec::new(),
    };
    let mut diagnostics = Vec::new();
    if let Some(manifest) = manifest {
        let assignment = [PackageAssignment {
            file_path: path.clone(),
            package,
        }];
        let stub = SurvFile {
            package: Some(context.self_package.clone()),
            namespace: context.namespace.clone(),
            imports: ast
                .imports
                .iter()
                .map(|target| ImportDecl {
                    target: target.clone(),
                    alias: None,
                })
                .collect(),
            ..Default::default()
        };
        let (contexts, issues) = parse_imports_with_alias(manifest, &assignment, &[(path, stub)]);
        context = contexts.into_iter().next().unwrap();
        diagnostics.extend(issues.into_iter().map(structural_diag));
        for import in &context.imports {
            if let Some(issue) =
                crate::symbol::check_package_dependency(Some(manifest), &context, &import.package)
            {
                diagnostics.push(structural_diag(issue));
            }
        }
    } else {
        for import in &ast.imports {
            crate::imports::parse_import_entry(import)
                .map_err(|_| format!("invalid import '{import}'"))?;
            diagnostics.push(BehaviorDiagnostic::error(
                "E_BEHAVIOR_MANIFEST_REQUIRED",
                source.path.display().to_string(),
                format!("import '{import}' requires a surv.toml project"),
            ));
        }
    }
    Ok((ast, context, diagnostics))
}
