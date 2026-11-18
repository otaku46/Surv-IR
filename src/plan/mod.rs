pub mod graph;
pub mod loader;
pub mod overlay;
pub mod reporter;
pub mod scc;
pub mod scheduler;
pub mod status_sync;
pub mod types;

pub use reporter::PlanReport;
pub use types::PlanRequest;

use crate::Diagnostic;
use std::error::Error;
use std::io::{self, Write};
use std::path::Path;

const USAGE: &str = "Usage: surc plan <design.toml|surv.toml> [workspace_root] [options]\n\
  --mod <module>           Restrict to a module and its dependencies\n\
  --lang <lang>            rust, ts, c, zig, both (default), or all\n\
  --strategy <strategy>    static (default) or lsp\n\
  --mapping <file>         Use an explicit symbol mapping\n\
  --sync-status            Update module state and coverage (requires a workspace)\n\
  --format <format>        text (default), json, or md";

pub fn run_plan(args: &[String]) -> Result<bool, Box<dyn Error>> {
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        println!("{USAGE}");
        return Ok(true);
    }
    let (report, json) = match PlanRequest::parse(args) {
        Ok(request) => {
            let format = request.format.clone();
            (execute_plan(request), format == "json")
        }
        Err(message) => {
            let format = args
                .windows(2)
                .rev()
                .find(|args| args[0] == "--format")
                .map(|args| args[1].as_str())
                .unwrap_or("text");
            let mut report = PlanReport::new("surc plan".into());
            report
                .diagnostics
                .push(error("InvalidPlanRequest", message, Path::new("surc plan")));
            (report.finish(format), format == "json")
        }
    };
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, &report)?;
        writeln!(stdout)?;
    } else {
        write!(stdout, "{}", report.formatted_output)?;
    }
    Ok(report.status == "OK")
}

pub fn execute_plan(request: PlanRequest) -> PlanReport {
    // 1. loadPlanTarget
    let mut loaded = loader::load_plan_target(&request);
    if loaded.has_errors {
        return reporter::fail_plan_on_invalid_input(&loaded);
    }

    // 2. buildDependencyGraph
    let graph = graph::build_plan_dependency_graph(&loaded);
    loaded.diagnostics.extend(graph.diagnostics.clone());
    if has_errors(&loaded.diagnostics) {
        return reporter::fail_plan_on_invalid_input(&loaded);
    }

    // 3. condenseSccAndRank
    let dag = scc::condense_scc_and_rank(&loaded, &graph);
    if dag.has_errors {
        let overlay = overlay::OverlayedPlanDag {
            dag: None,
            node_statuses: Vec::new(),
            workspace_checked: request.workspace_root.is_some(),
            has_errors: true,
            diagnostics: dag.diagnostics,
        };
        return reporter::fail_plan_on_overlay_error(&loaded, &overlay);
    }

    // 4. Overlay
    let overlay = if request.workspace_root.is_some() {
        overlay::apply_workspace_diff_overlay(&loaded, &dag)
    } else {
        overlay::apply_design_only_overlay(&loaded, &dag)
    };
    if overlay.has_errors {
        return reporter::fail_plan_on_overlay_error(&loaded, &overlay);
    }

    // 5. scheduleWavesAndTracks
    let scheduled = scheduler::schedule_waves_and_tracks(&loaded, &overlay);

    // 6. syncStatusSections / skipStatusSync
    let sync = if request.sync_status {
        status_sync::sync_plan_status_sections(&loaded, &scheduled)
    } else {
        status_sync::skip_plan_status_sync(&loaded, &scheduled)
    };

    // 7. buildPlanReport
    reporter::build_plan_report(&loaded, &scheduled, &sync)
}

pub(crate) fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|diag| diag.severity == "error")
}

pub(crate) fn error(kind: &str, message: String, path: &Path) -> Diagnostic {
    Diagnostic {
        severity: "error".into(),
        kind: kind.into(),
        message,
        location: path.display().to_string(),
    }
}
