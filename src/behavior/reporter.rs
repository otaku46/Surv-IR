use super::explorer::{explore_and_check_bfs, CounterexampleTrace, ExplorationOutcome, ModelState};
use super::model::{BehaviorDiagnostic, FrontendOutcome};
use serde::Serialize;
use std::fmt::Write;

#[derive(Debug, Serialize)]
pub struct VerificationReport {
    pub behavior_name: String,
    pub status: &'static str,
    pub stop_reason: &'static str,
    pub explored_states: usize,
    pub pending_states: usize,
    pub max_depth_reached: usize,
    pub diagnostics: Vec<BehaviorDiagnostic>,
    pub trace: Option<CounterexampleTrace>,
}

impl VerificationReport {
    pub fn exit_code(&self) -> i32 {
        match self.status {
            "PASS" => 0,
            "UNKNOWN" => 2,
            _ => 1,
        }
    }

    pub fn text(&self) -> String {
        let mut text = format!("{}: {}\n", self.status, self.behavior_name);
        if self.status != "ERROR" {
            let _ = writeln!(
                text,
                "  Explored: {} states (depth {}), {} pending [{}]",
                self.explored_states, self.max_depth_reached, self.pending_states, self.stop_reason
            );
        }
        for diag in &self.diagnostics {
            let _ = writeln!(
                text,
                "  {} [{}] {} at {}",
                diag.severity, diag.code, diag.message, diag.location
            );
        }
        if let Some(trace) = &self.trace {
            let _ = writeln!(
                text,
                "  {}: {} ({})\n\nCounterexample Trace:",
                trace.violation_kind, trace.violated_name, trace.violated_expr
            );
            let _ = writeln!(
                text,
                "  State {} (init): {}",
                trace.initial_state.state_id,
                values(&trace.initial_state)
            );
            for step in &trace.steps {
                let _ = writeln!(
                    text,
                    "  Step {} --[action.{}{}]--> State {}: {}",
                    step.step_index,
                    step.action_name,
                    refs(&step.surv_func_refs),
                    step.to_state.state_id,
                    values(&step.to_state)
                );
            }
            if let Some(attempt) = &trace.overflow_attempt {
                let _ = writeln!(
                    text,
                    "  Step {} --[action.{}{}]--> {} = {} outside {}..{}",
                    attempt.step_index,
                    attempt.action_name,
                    refs(&attempt.surv_func_refs),
                    attempt.target_var,
                    attempt.attempted_value,
                    attempt.min_bound,
                    attempt.max_bound
                );
            }
        }
        text
    }
}

fn values(state: &ModelState) -> String {
    state
        .valuations
        .iter()
        .map(|binding| format!("{} = {}", binding.var_name, binding.value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn refs(references: &[String]) -> String {
    if references.is_empty() {
        String::new()
    } else {
        format!(" (ref: {})", references.join(", "))
    }
}

pub(super) fn build_static_error_report(frontend: FrontendOutcome) -> VerificationReport {
    VerificationReport {
        behavior_name: frontend.target_name,
        status: "ERROR",
        stop_reason: "static_error",
        explored_states: 0,
        pending_states: 0,
        max_depth_reached: 0,
        diagnostics: frontend.diagnostics,
        trace: None,
    }
}

pub(super) fn build_verification_report(
    frontend: FrontendOutcome,
    outcome: ExplorationOutcome,
) -> VerificationReport {
    let status = match outcome.stop_reason {
        "exhausted" => "PASS",
        "violation_found" => "FAIL",
        _ => "UNKNOWN",
    };
    VerificationReport {
        behavior_name: outcome.behavior_name,
        status,
        stop_reason: outcome.stop_reason,
        explored_states: outcome.explored_states,
        pending_states: outcome.pending_states,
        max_depth_reached: outcome.max_depth_reached,
        diagnostics: frontend.diagnostics,
        trace: outcome.counterexample,
    }
}

pub fn verify(mut frontend: FrontendOutcome) -> VerificationReport {
    if frontend.has_errors || frontend.model.is_none() {
        return build_static_error_report(frontend);
    }
    match explore_and_check_bfs(&mut frontend) {
        Ok(result) => build_verification_report(frontend, result),
        Err(err) => {
            frontend.diagnostics.push(BehaviorDiagnostic::error(
                "E_BEHAVIOR_EVAL",
                &frontend.target_name,
                err,
            ));
            let mut report = build_static_error_report(frontend);
            report.stop_reason = "evaluation_error";
            report
        }
    }
}
