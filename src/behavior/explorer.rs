use super::expr::{StateValue, TypedExpr};
use super::model::{BehaviorDiagnostic, FrontendOutcome, NormalizedBehaviorModel};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize)]
pub struct StateVarBinding {
    pub var_name: String,
    pub value: StateValue,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelState {
    pub state_id: usize,
    pub depth: usize,
    pub valuations: Vec<StateVarBinding>,
    pub is_terminal: bool,
}

#[derive(Debug, Serialize)]
pub struct TraceStep {
    pub step_index: usize,
    pub action_name: String,
    pub surv_func_refs: Vec<String>,
    pub from_state: ModelState,
    pub to_state: ModelState,
}

#[derive(Debug, Serialize)]
pub struct OverflowAttempt {
    pub step_index: usize,
    pub from_state: ModelState,
    pub action_name: String,
    pub surv_func_refs: Vec<String>,
    pub target_var: String,
    pub attempted_value: i128,
    pub min_bound: i128,
    pub max_bound: i128,
}

#[derive(Debug, Serialize)]
pub struct CounterexampleTrace {
    pub violation_kind: &'static str,
    pub violated_name: String,
    pub violated_expr: String,
    pub initial_state: ModelState,
    pub steps: Vec<TraceStep>,
    pub violated_state: Option<ModelState>,
    pub overflow_attempt: Option<OverflowAttempt>,
}

#[derive(Debug, Serialize)]
pub struct ExplorationOutcome {
    pub behavior_name: String,
    pub stop_reason: &'static str,
    pub explored_states: usize,
    pub pending_states: usize,
    pub max_depth_reached: usize,
    pub counterexample: Option<CounterexampleTrace>,
}

struct State {
    values: Vec<StateValue>,
    parent: Option<(usize, usize)>,
    depth: usize,
    terminal: bool,
}

struct Explorer<'a> {
    model: &'a NormalizedBehaviorModel,
    positions: HashMap<&'a str, usize>,
    states: Vec<State>,
    seen: HashMap<Vec<StateValue>, usize>,
    explored: usize,
    max_depth: usize,
}

pub(super) fn explore_and_check_bfs(
    frontend: &mut FrontendOutcome,
) -> Result<ExplorationOutcome, String> {
    let model = frontend
        .model
        .as_ref()
        .ok_or("validated model is required for exploration")?;
    let positions = model
        .domains
        .iter()
        .enumerate()
        .map(|(index, domain)| (domain.var_name.as_str(), index))
        .collect();
    let initial = model
        .init_assignments
        .iter()
        .map(|assignment| assignment.value_expr.eval(&|_| None))
        .collect::<Result<Vec<_>, _>>()?;
    let mut explorer = Explorer {
        model,
        positions,
        states: Vec::new(),
        seen: HashMap::new(),
        explored: 0,
        max_depth: 0,
    };
    explorer.insert(initial, None, 0)?;
    explorer.run(&mut frontend.diagnostics)
}

impl Explorer<'_> {
    fn insert(
        &mut self,
        values: Vec<StateValue>,
        parent: Option<(usize, usize)>,
        depth: usize,
    ) -> Result<(), String> {
        let terminal = self
            .model
            .terminal
            .as_ref()
            .map(|expr| self.predicate(expr, &values))
            .transpose()?
            .unwrap_or(false);
        self.seen.insert(values.clone(), self.states.len());
        self.states.push(State {
            values,
            parent,
            depth,
            terminal,
        });
        Ok(())
    }

    fn value(&self, expr: &TypedExpr, values: &[StateValue]) -> Result<StateValue, String> {
        expr.eval(&|name| {
            self.positions
                .get(name)
                .and_then(|index| values.get(*index))
                .cloned()
        })
    }

    fn predicate(&self, expr: &TypedExpr, values: &[StateValue]) -> Result<bool, String> {
        self.value(expr, values)?.boolean()
    }

    fn enabled(&self, action: usize, values: &[StateValue]) -> Result<bool, String> {
        self.model.actions[action]
            .guard
            .as_ref()
            .map(|guard| self.predicate(guard, values))
            .transpose()
            .map(|result| result.unwrap_or(true))
    }

    fn outcome(
        &self,
        stop_reason: &'static str,
        pending: usize,
        counterexample: Option<CounterexampleTrace>,
    ) -> ExplorationOutcome {
        ExplorationOutcome {
            behavior_name: self.model.name.clone(),
            stop_reason,
            explored_states: self.explored,
            pending_states: pending,
            max_depth_reached: self.max_depth,
            counterexample,
        }
    }

    fn run(
        &mut self,
        diagnostics: &mut Vec<BehaviorDiagnostic>,
    ) -> Result<ExplorationOutcome, String> {
        let mut start = 0;
        while start < self.states.len() {
            let end = self.states.len();
            let depth = self.states[start].depth;

            // State violations cost d steps. Check the whole layer before an
            // overflow in one of its outgoing transitions can end the search.
            for id in start..end {
                self.explored += 1;
                self.max_depth = depth;
                let state = &self.states[id];
                for invariant in &self.model.invariants {
                    if self.predicate(&invariant.expr, &state.values)? {
                        continue;
                    }
                    diagnostics.push(BehaviorDiagnostic {
                        code: "E_BEHAVIOR_INVARIANT".into(),
                        severity: invariant.severity.clone(),
                        message: format!(
                            "invariant '{}' is false at depth {depth}: {}",
                            invariant.name, invariant.source
                        ),
                        location: format!(
                            "behavior.{}.invariant.{}",
                            self.model.name, invariant.name
                        ),
                        related_symbol: None,
                    });
                    let trace = self.trace(
                        id,
                        "invariant",
                        invariant.name.clone(),
                        invariant.source.clone(),
                        None,
                    );
                    return Ok(self.outcome("violation_found", end - id - 1, Some(trace)));
                }
                if self.model.config.check_deadlock && !state.terminal {
                    let mut has_action = false;
                    for action in 0..self.model.actions.len() {
                        if self.enabled(action, &state.values)? {
                            has_action = true;
                            break;
                        }
                    }
                    if !has_action {
                        let trace = self.trace(
                            id,
                            "deadlock",
                            "non_terminal_deadlock".into(),
                            "no guard is enabled and terminal is false".into(),
                            None,
                        );
                        return Ok(self.outcome("violation_found", end - id - 1, Some(trace)));
                    }
                }
            }

            for id in start..end {
                for (action_index, action) in self.model.actions.iter().enumerate() {
                    let before = &self.states[id].values;
                    if !self.enabled(action_index, before)? {
                        continue;
                    }
                    let mut after = before.clone();
                    for assignment in &action.updates {
                        let index = self.positions[assignment.target_var.as_str()];
                        let domain = &self.model.domains[index];
                        let value = self.value(&assignment.value_expr, before)?;
                        if !domain.contains(&value) {
                            if depth == self.model.config.max_depth {
                                return Ok(self.outcome("max_depth_reached", 1, None));
                            }
                            let StateValue::Int(attempted_value) = value else {
                                return Err("invalid bounds on a non-integer update".into());
                            };
                            let min_bound = domain.min_val.unwrap();
                            let max_bound = domain.max_val.unwrap();
                            let attempt = OverflowAttempt {
                                step_index: depth + 1,
                                from_state: self.snapshot(id),
                                action_name: action.name.clone(),
                                surv_func_refs: action.surv_func_refs.clone(),
                                target_var: assignment.target_var.clone(),
                                attempted_value,
                                min_bound,
                                max_bound,
                            };
                            let trace = self.trace(
                                id,
                                "overflow",
                                assignment.target_var.clone(),
                                format!("{min_bound} <= {} <= {max_bound}", assignment.target_var),
                                Some(attempt),
                            );
                            return Ok(self.outcome(
                                "violation_found",
                                self.states.len() - end,
                                Some(trace),
                            ));
                        }
                        after[index] = value;
                    }
                    if self.seen.contains_key(&after) {
                        continue;
                    }
                    if depth == self.model.config.max_depth {
                        return Ok(self.outcome("max_depth_reached", 1, None));
                    }
                    if self.states.len() == self.model.config.max_states {
                        return Ok(self.outcome(
                            "max_states_reached",
                            self.states.len() - end + 1,
                            None,
                        ));
                    }
                    self.insert(after, Some((id, action_index)), depth + 1)?;
                }
            }
            start = end;
        }
        Ok(self.outcome("exhausted", 0, None))
    }

    fn snapshot(&self, id: usize) -> ModelState {
        let state = &self.states[id];
        ModelState {
            state_id: id,
            depth: state.depth,
            is_terminal: state.terminal,
            valuations: self
                .model
                .domains
                .iter()
                .zip(&state.values)
                .map(|(domain, value)| StateVarBinding {
                    var_name: domain.var_name.clone(),
                    value: value.clone(),
                })
                .collect(),
        }
    }

    fn trace(
        &self,
        id: usize,
        violation_kind: &'static str,
        violated_name: String,
        violated_expr: String,
        overflow_attempt: Option<OverflowAttempt>,
    ) -> CounterexampleTrace {
        let violated_state = overflow_attempt.is_none().then(|| self.snapshot(id));
        let mut current = id;
        let mut steps = Vec::new();
        while let Some((parent, action)) = self.states[current].parent {
            let action = &self.model.actions[action];
            steps.push(TraceStep {
                step_index: self.states[current].depth,
                action_name: action.name.clone(),
                surv_func_refs: action.surv_func_refs.clone(),
                from_state: self.snapshot(parent),
                to_state: self.snapshot(current),
            });
            current = parent;
        }
        steps.reverse();
        CounterexampleTrace {
            violation_kind,
            violated_name,
            violated_expr,
            initial_state: self.snapshot(current),
            steps,
            violated_state,
            overflow_attempt,
        }
    }
}
