use super::explorer::{
    explore_and_check_bfs, CounterexampleTrace, ExplorationOutcome, ModelState, ReplayTrace,
};
use super::model::{BehaviorDiagnostic, FrontendOutcome, VerificationBackend};
use super::smt;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Debug, Serialize)]
pub struct VerificationReport {
    pub behavior_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<VerificationBackend>,
    pub status: &'static str,
    pub stop_reason: &'static str,
    pub explored_states: usize,
    pub pending_states: usize,
    pub max_depth_reached: usize,
    pub diagnostics: Vec<BehaviorDiagnostic>,
    pub trace: Option<CounterexampleTrace>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub reached: BTreeMap<String, ReplayTrace>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unreached: Vec<String>,
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
        if self.backend == Some(VerificationBackend::Smt) && self.status != "ERROR" {
            let _ = writeln!(
                text,
                "  SMT reached depth {} [{}]",
                self.max_depth_reached, self.stop_reason
            );
        } else if self.status != "ERROR" {
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
        for (name, trace) in &self.reached {
            let _ = writeln!(text, "  Reached {name} ({} steps)", trace.steps.len());
        }
        for name in &self.unreached {
            let _ = writeln!(text, "  No witness for {name}");
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
            if trace.violated_state.is_none() && trace.overflow_attempt.is_none() {
                if let Some(step) = trace.replay.steps.last() {
                    let _ = writeln!(
                        text,
                        "  Step {} --[action.{}{}]--> {}",
                        trace.replay.steps.len(),
                        step.action,
                        refs(&step.refs),
                        trace.violated_expr
                    );
                }
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
        backend: None,
        status: "ERROR",
        stop_reason: "static_error",
        explored_states: 0,
        pending_states: 0,
        max_depth_reached: 0,
        diagnostics: frontend.diagnostics,
        trace: None,
        reached: BTreeMap::new(),
        unreached: Vec::new(),
    }
}

pub(super) fn build_verification_report(
    frontend: FrontendOutcome,
    outcome: ExplorationOutcome,
) -> VerificationReport {
    let status = match outcome.stop_reason {
        "exhausted" => "PASS",
        "violation_found" | "unreachable_goal" => "FAIL",
        _ => "UNKNOWN",
    };
    VerificationReport {
        behavior_name: outcome.behavior_name,
        backend: None,
        status,
        stop_reason: outcome.stop_reason,
        explored_states: outcome.explored_states,
        pending_states: outcome.pending_states,
        max_depth_reached: outcome.max_depth_reached,
        diagnostics: frontend.diagnostics,
        trace: outcome.counterexample,
        reached: outcome.reached,
        unreached: outcome.unreached,
    }
}

pub fn verify(mut frontend: FrontendOutcome) -> VerificationReport {
    if frontend.has_errors || frontend.model.is_none() {
        return build_static_error_report(frontend);
    }
    let model = frontend.model.as_ref().unwrap();
    let backend = model.config.backend;
    let result = match backend {
        VerificationBackend::Internal => {
            explore_and_check_bfs(&mut frontend).map_err(smt::Error::Evaluation)
        }
        VerificationBackend::Smt => smt::verify(model, &mut frontend.diagnostics),
    };
    let mut report = match result {
        Ok(result) => build_verification_report(frontend, result),
        Err(err) => {
            let (code, reason, message) = match err {
                smt::Error::Solver(message) => ("E_BEHAVIOR_SMT", "solver_error", message),
                smt::Error::Evaluation(message) => ("E_BEHAVIOR_EVAL", "evaluation_error", message),
            };
            frontend.diagnostics.push(BehaviorDiagnostic::error(
                code,
                &frontend.target_name,
                message,
            ));
            let mut report = build_static_error_report(frontend);
            report.stop_reason = reason;
            report
        }
    };
    if backend == VerificationBackend::Smt {
        report.backend = Some(backend);
    }
    report
}
