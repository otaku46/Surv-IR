use super::types::PlanRequest;
use crate::loader::load_project_with_manifest;
use crate::project_checker::{check_with_contexts, prepare_project};
use crate::{
    check_surv_file, load_manifest, parse_surv_file, Diagnostic, FileImportContext, Manifest,
    NormalizedRequire, ProjectAST, Section, SymbolTable,
};
use std::error::Error;
use std::path::Path;

pub struct LoadedPlanContext {
    pub request: PlanRequest,
    pub project_name: String,
    pub manifest: Option<Manifest>,
    pub project: Option<ProjectAST>,
    pub symbol_table: Option<SymbolTable>,
    pub import_contexts: Vec<FileImportContext>,
    pub normalized_requires: Vec<NormalizedRequire>,
    pub has_errors: bool,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn load_plan_target(request: &PlanRequest) -> LoadedPlanContext {
    let mut context = LoadedPlanContext {
        request: request.clone(),
        project_name: target_name(&request.ir_path),
        manifest: None,
        project: None,
        symbol_table: None,
        import_contexts: Vec::new(),
        normalized_requires: Vec::new(),
        has_errors: false,
        diagnostics: Vec::new(),
    };

    if let Err(message) = request.validate() {
        context.diagnostics.push(super::error(
            "InvalidPlanRequest",
            message,
            &request.ir_path,
        ));
        context.has_errors = true;
        return context;
    }

    let (manifest, mut project) = match load_ir_target(&request.ir_path) {
        Ok(target) => target,
        Err(err) => {
            context.diagnostics.push(super::error(
                "InvalidPlanTarget",
                err.to_string(),
                &request.ir_path,
            ));
            context.has_errors = true;
            return context;
        }
    };

    if let Some(manifest_ref) = &manifest {
        context.project_name = manifest_ref.project.name.clone();
    } else if let Some(name) =
        project.files[0]
            .1
            .sections
            .iter()
            .find_map(|section| match section {
                Section::Meta(meta) if !meta.name.is_empty() => Some(&meta.name),
                _ => None,
            })
    {
        context.project_name = name.clone();
    }

    let root = request.ir_path.parent().unwrap_or_else(|| Path::new("."));
    let (contexts, symbols, diagnostics) = prepare_project(&mut project, manifest.as_ref(), root);
    context.diagnostics.extend(diagnostics);

    let checks = if manifest.is_some() {
        check_with_contexts(&project, &contexts, &symbols, manifest.as_ref())
    } else {
        check_surv_file(&project.files[0].1)
    };
    context.diagnostics.extend(checks);

    if super::has_errors(&context.diagnostics) {
        context.has_errors = true;
        return context;
    }

    let normalized_requires = project
        .files
        .iter()
        .flat_map(|(path, file)| {
            file.requires.iter().map(move |req| NormalizedRequire {
                from_mod: file
                    .sections
                    .iter()
                    .find_map(|sec| match sec {
                        Section::Mod(m) => Some(m.name.clone()),
                        _ => None,
                    })
                    .unwrap_or_default(),
                to_mod: req.target.clone(),
                raw: req.target.clone(),
                file: path.clone(),
            })
        })
        .collect();

    context.manifest = manifest;
    context.project = Some(project);
    context.symbol_table = Some(symbols);
    context.import_contexts = contexts;
    context.normalized_requires = normalized_requires;
    context
}

fn load_ir_target(path: &Path) -> Result<(Option<Manifest>, ProjectAST), Box<dyn Error>> {
    let source = std::fs::read_to_string(path)?;
    let _: toml::Value = toml::from_str(&source)?;
    let headers: Vec<_> = source.lines().map(str::trim).collect();
    let manifest_header = headers
        .iter()
        .any(|line| matches!(*line, "[project]" | "[paths]") || line.starts_with("[packages."));
    if path.file_name().is_some_and(|name| name == "surv.toml") || manifest_header {
        let manifest = load_manifest(path)?;
        let project = load_project_with_manifest(path, &manifest)?;
        return Ok((Some(manifest), project));
    }
    if headers.iter().any(|line| {
        matches!(*line, "[deploy]" | "[behavior]")
            || line.starts_with("[deploy.")
            || line.starts_with("[behavior.")
    }) {
        return Err("Expected Structural IR or a surv.toml project".into());
    }
    let file = parse_surv_file(source.as_bytes())?;
    Ok((None, ProjectAST::from_files(vec![(path.to_owned(), file)])))
}

fn target_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "surc plan".into())
}
