use super::loader::LoadedPlanContext;
use super::scheduler::{ModulePlanSummary, ScheduledPlan};
use crate::status_edit::{set_status_value, status_module_mut, write_atomic, StatusDocument};
use crate::Diagnostic;
use serde::Serialize;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct StatusSyncOutcome {
    pub has_errors: bool,
    pub updated_files: Vec<PathBuf>,
    pub sync_diagnostics: Vec<Diagnostic>,
}

pub fn sync_plan_status_sections(
    _context: &LoadedPlanContext,
    plan: &ScheduledPlan,
) -> StatusSyncOutcome {
    let mut diagnostics = Vec::new();
    let updated_files = sync_plan_status_sections_inner(&plan.module_summaries, &mut diagnostics);
    let has_errors = super::has_errors(&diagnostics);
    StatusSyncOutcome {
        has_errors,
        updated_files,
        sync_diagnostics: diagnostics,
    }
}

pub fn skip_plan_status_sync(
    _context: &LoadedPlanContext,
    _plan: &ScheduledPlan,
) -> StatusSyncOutcome {
    StatusSyncOutcome {
        has_errors: false,
        updated_files: Vec::new(),
        sync_diagnostics: Vec::new(),
    }
}

pub(super) fn sync_plan_status_sections_inner(
    summaries: &[ModulePlanSummary],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<PathBuf> {
    let mut files = BTreeMap::<&Path, Vec<_>>::new();
    for summary in summaries {
        files.entry(&summary.source_file).or_default().push(summary);
    }
    let mut updated = Vec::new();
    for (path, modules) in files {
        match sync_file(path, &modules) {
            Ok(true) => updated.push(path.to_path_buf()),
            Ok(false) => {}
            Err(err) => diagnostics.push(super::error("StatusSyncFailed", err.to_string(), path)),
        }
    }
    updated
}

fn sync_file(path: &Path, modules: &[&ModulePlanSummary]) -> Result<bool, Box<dyn Error>> {
    let path = path.canonicalize()?;
    let original = fs::read_to_string(&path)?;
    let mut document = StatusDocument::parse(&original)?;
    let status = document.status(true).ok_or("status must be a table")?;
    let mut synced = false;
    for summary in modules {
        if summary.out_of_scope {
            continue;
        }
        let module = status_module_mut(status, &summary.mod_name, true)
            .ok_or_else(|| format!("Invalid status entry for mod.{}", summary.mod_name))?;
        if module.get("state").and_then(toml_edit::Item::as_str) != Some("blocked") {
            set_status_value(module, "state", &summary.derived_state);
        }
        set_status_value(module, "coverage", summary.derived_coverage);
        synced = true;
    }
    if !synced {
        return Ok(false);
    }
    let now = chrono::Local::now().format("%Y-%m-%d").to_string();
    set_status_value(status, "updated_at", now);
    let updated = document.finish();
    if original == updated {
        return Ok(false);
    }
    write_atomic(&path, &original, &updated)?;
    Ok(true)
}
