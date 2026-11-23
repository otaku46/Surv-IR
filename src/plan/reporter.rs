use super::loader::LoadedPlanContext;
use super::overlay::{ImplState, NodeImplStatus, OverlayedPlanDag};
use super::scheduler::{ModulePlanSummary, ParallelTrack, PlanWave, Readiness, ScheduledPlan};
use super::status_sync::StatusSyncOutcome;
use crate::Diagnostic;
use serde::Serialize;
use std::fmt::Write;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct PlanReport {
    pub status: String,
    pub project_name: String,
    pub workspace_checked: bool,
    pub total_symbols: usize,
    pub done_symbols: usize,
    pub drifted_symbols: usize,
    pub unverified_symbols: usize,
    pub ready_unit_count: usize,
    pub blocked_unit_count: usize,
    pub unverified_unit_count: usize,
    pub waves: Vec<PlanWave>,
    pub parallel_tracks: Vec<ParallelTrack>,
    pub module_summaries: Vec<ModulePlanSummary>,
    pub status_files_updated: Vec<PathBuf>,
    pub formatted_output: String,
    pub diagnostics: Vec<Diagnostic>,
    pub node_statuses: Vec<NodeImplStatus>,
}

impl PlanReport {
    pub fn new(project_name: String) -> Self {
        Self {
            status: "OK".to_string(),
            project_name,
            workspace_checked: false,
            total_symbols: 0,
            done_symbols: 0,
            drifted_symbols: 0,
            unverified_symbols: 0,
            ready_unit_count: 0,
            blocked_unit_count: 0,
            unverified_unit_count: 0,
            waves: Vec::new(),
            parallel_tracks: Vec::new(),
            module_summaries: Vec::new(),
            status_files_updated: Vec::new(),
            formatted_output: String::new(),
            diagnostics: Vec::new(),
            node_statuses: Vec::new(),
        }
    }

    pub fn finish(mut self, format: &str) -> Self {
        self.diagnostics.sort_by(|a, b| {
            (&a.location, &a.kind, &a.message).cmp(&(&b.location, &b.kind, &b.message))
        });
        self.diagnostics.dedup_by(|a, b| {
            (&a.severity, &a.kind, &a.location, &a.message)
                == (&b.severity, &b.kind, &b.location, &b.message)
        });
        if self.diagnostics.iter().any(|diag| diag.severity == "error") {
            self.status = "ERROR".to_string();
        }
        let symbols = self
            .node_statuses
            .iter()
            .filter(|status| !status.is_module && status.impl_state != ImplState::OutOfScope);
        for status in symbols {
            self.total_symbols += 1;
            self.done_symbols += usize::from(status.impl_state == ImplState::Done);
            self.drifted_symbols += usize::from(status.impl_state == ImplState::Drifted);
            self.unverified_symbols += usize::from(status.impl_state == ImplState::Unverified);
        }
        for unit in self.waves.iter().flat_map(|wave| &wave.units) {
            self.ready_unit_count += usize::from(unit.readiness == Readiness::Ready);
            self.blocked_unit_count += usize::from(unit.readiness == Readiness::Blocked);
            self.unverified_unit_count += usize::from(unit.readiness == Readiness::Unverified);
        }
        self.formatted_output = self.render(format == "md");
        self
    }

    fn render(&self, markdown: bool) -> String {
        let mut text = String::new();
        let heading = if markdown { "## " } else { "" };
        let title = if markdown { "# " } else { "" };
        let node_states: std::collections::HashMap<_, _> = self
            .node_statuses
            .iter()
            .map(|status| (status.fq_name.as_str(), status))
            .collect();
        writeln!(
            text,
            "{title}Plan for {} ({})",
            self.project_name, self.status
        )
        .unwrap();
        let mode = if self.workspace_checked {
            "Workspace checked"
        } else {
            "Design only"
        };
        writeln!(
            text,
            "\n{mode}. Symbols: {}/{} done, {} drifted, {} unverified. Units: {} ready, {} blocked, {} unverified.",
            self.done_symbols,
            self.total_symbols,
            self.drifted_symbols,
            self.unverified_symbols,
            self.ready_unit_count,
            self.blocked_unit_count,
            self.unverified_unit_count,
        )
        .unwrap();

        for wave in &self.waves {
            writeln!(text, "\n{heading}{}", wave.label).unwrap();
            for item in &wave.units {
                let node = &item.unit.member_nodes[0];
                let shared = if item.unit.is_shared_boundary {
                    " [shared]"
                } else {
                    ""
                };
                writeln!(
                    text,
                    "- {}{shared}: {} ({})",
                    item.readiness.describe(),
                    node.display_ref,
                    item.unit.unit_id
                )
                .unwrap();
                for member in &item.unit.member_nodes {
                    let status = &node_states[member.fq_name.as_str()];
                    let location = status
                        .matched_location
                        .as_deref()
                        .map(|loc| format!(" — {loc}"))
                        .unwrap_or_default();
                    writeln!(
                        text,
                        "  - {}: {}{location}",
                        member.display_ref,
                        status.impl_state.describe()
                    )
                    .unwrap();
                    for drift in &status.drift_problems {
                        writeln!(text, "  - {}: {drift}", member.display_ref).unwrap();
                    }
                    for problem in &status.unverified_problems {
                        writeln!(text, "  - {}: unverified: {problem}", member.display_ref)
                            .unwrap();
                    }
                }
                if !item.blocked_by_units.is_empty() {
                    writeln!(text, "  - Blocked by {}", item.blocked_by_units.join(", ")).unwrap();
                }
            }
        }

        if !self.parallel_tracks.is_empty() {
            writeln!(text, "\n{heading}Parallel tracks").unwrap();
            for track in &self.parallel_tracks {
                let units = track
                    .ordered_units
                    .iter()
                    .map(|unit| unit.unit.unit_id.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ");
                writeln!(text, "- {}: {units}", track.owner_mod).unwrap();
            }
        }

        if !self.module_summaries.is_empty() {
            writeln!(text, "\n{heading}Module coverage").unwrap();
            for summary in &self.module_summaries {
                let percent = (summary.derived_coverage * 100.0).round() as usize;
                writeln!(
                    text,
                    "- mod.{}: {}, {}% ({}/{}) — {}",
                    summary.mod_name,
                    summary.derived_state,
                    percent,
                    summary.done_symbols,
                    summary.total_symbols,
                    summary.source_file.display()
                )
                .unwrap();
            }
        }

        if !self.status_files_updated.is_empty() {
            writeln!(text, "\n{heading}Updated status files").unwrap();
            for path in &self.status_files_updated {
                writeln!(text, "- {}", path.display()).unwrap();
            }
        }

        if !self.diagnostics.is_empty() {
            writeln!(text, "\n{heading}Diagnostics").unwrap();
            for diag in &self.diagnostics {
                writeln!(
                    text,
                    "- {}: {} [{}] {}",
                    diag.severity, diag.location, diag.kind, diag.message
                )
                .unwrap();
            }
        }

        text
    }
}

pub fn fail_plan_on_invalid_input(context: &LoadedPlanContext) -> PlanReport {
    let mut report = PlanReport::new(context.project_name.clone());
    report.diagnostics = context.diagnostics.clone();
    report.finish(&context.request.format)
}

pub fn fail_plan_on_overlay_error(
    context: &LoadedPlanContext,
    overlay: &OverlayedPlanDag,
) -> PlanReport {
    let mut report = PlanReport::new(context.project_name.clone());
    report.workspace_checked = overlay.workspace_checked;
    report.diagnostics = context
        .diagnostics
        .iter()
        .chain(&overlay.diagnostics)
        .cloned()
        .collect();
    report.finish(&context.request.format)
}

pub fn build_plan_report(
    context: &LoadedPlanContext,
    plan: &ScheduledPlan,
    sync: &StatusSyncOutcome,
) -> PlanReport {
    let mut report = PlanReport::new(plan.project_name.clone());
    report.workspace_checked = plan.workspace_checked;
    report.waves = plan.waves.clone();
    report.parallel_tracks = plan.parallel_tracks.clone();
    report.module_summaries = plan.module_summaries.clone();
    report.node_statuses = plan.node_statuses.clone();
    report.status_files_updated = sync.updated_files.clone();
    report.diagnostics = context
        .diagnostics
        .iter()
        .chain(&plan.diagnostics)
        .chain(&sync.sync_diagnostics)
        .cloned()
        .collect();
    if sync.has_errors {
        report.status = "ERROR".to_string();
    }
    report.finish(&context.request.format)
}
