use super::expr::{replay_values, ReplayValues, Row, StateValue, TypedExpr};
use super::model::{
    BehaviorDiagnostic, FrontendOutcome, NormalizedAction, NormalizedBehaviorModel,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

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
    pub replay: ReplayTrace,
}

#[derive(Debug, Serialize)]
pub struct ReplayTrace {
    pub version: u8,
    pub initial: ReplayValues,
    pub steps: Vec<ReplayStep>,
}

#[derive(Debug, Serialize)]
pub struct ReplayStep {
    pub action: String,
    pub operation: String,
    pub refs: Vec<String>,
    pub args: ReplayValues,
    pub observations: Option<ReplayValues>,
}

#[derive(Debug, Serialize)]
pub struct ExplorationOutcome {
    pub behavior_name: String,
    pub stop_reason: &'static str,
    pub explored_states: usize,
    pub pending_states: usize,
    pub max_depth_reached: usize,
    pub counterexample: Option<CounterexampleTrace>,
    pub reached: BTreeMap<String, ReplayTrace>,
    pub unreached: Vec<String>,
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
    reached: BTreeMap<String, usize>,
}

pub(super) fn explore_and_check_bfs(
    frontend: &mut FrontendOutcome,
) -> Result<ExplorationOutcome, String> {
    let model = frontend
        .model
        .as_ref()
        .ok_or("validated model is required for exploration")?;
    Explorer::new(model)?.run(&mut frontend.diagnostics)
}

pub(super) struct ProofState {
    pub values: Vec<StateValue>,
    pub parent: Option<usize>,
}

#[derive(Default)]
pub(super) struct LeanEvidence {
    pub states: Vec<ProofState>,
    pub goals: BTreeMap<String, usize>,
    pub stop_reason: &'static str,
}

impl LeanEvidence {
    pub fn is_closed(&self) -> bool {
        matches!(self.stop_reason, "exhausted" | "unreachable_goal")
    }
}

pub(super) fn collect_lean_evidence(model: &NormalizedBehaviorModel) -> LeanEvidence {
    let Ok(mut explorer) = Explorer::new(model) else {
        return LeanEvidence {
            stop_reason: "evaluation_error",
            ..LeanEvidence::default()
        };
    };
    let stop_reason = explorer
        .run(&mut Vec::new())
        .map_or("evaluation_error", |result| result.stop_reason);
    LeanEvidence {
        states: explorer
            .states
            .into_iter()
            .map(|state| ProofState {
                values: state.values,
                parent: state.parent.map(|(id, _)| id),
            })
            .collect(),
        goals: explorer.reached,
        stop_reason,
    }
}

pub(super) enum Witness<'a> {
    StateViolation,
    FailedAction(usize),
    Goal(&'a str),
}

pub(super) enum ReplayedWitness {
    Violation(Box<CounterexampleTrace>),
    Goal(ReplayTrace),
}

pub(super) fn replay_symbolic_witness(
    model: &NormalizedBehaviorModel,
    actions: &[usize],
    witness: Witness<'_>,
    diagnostics: &mut Vec<BehaviorDiagnostic>,
) -> Result<ReplayedWitness, String> {
    let attempted = match witness {
        Witness::FailedAction(action) => Some(action),
        _ => None,
    };
    if actions
        .iter()
        .copied()
        .chain(attempted)
        .any(|action| action >= model.actions.len())
    {
        return Err("SMT witness contains an unknown action".into());
    }
    let mut explorer = Explorer::new(model)?;
    for (id, &action) in actions.iter().enumerate() {
        let Transition::Next(values) = explorer.transition(id, action)? else {
            return Err("SMT witness contains an invalid transition".into());
        };
        if explorer.seen.contains_key(&values) {
            return Err("SMT witness revisits a state".into());
        }
        explorer.insert(values, Some((id, action)), id + 1)?;
    }
    let id = actions.len();
    let witness = match witness {
        Witness::StateViolation => explorer
            .state_violation(id, diagnostics)?
            .map(|trace| ReplayedWitness::Violation(Box::new(trace))),
        Witness::FailedAction(action) => match explorer.transition(id, action)? {
            Transition::Rejected(trace) => Some(ReplayedWitness::Violation(trace)),
            _ => None,
        },
        Witness::Goal(name) => explorer
            .predicate(&model.goals[name], &explorer.states[id].values)?
            .then(|| ReplayedWitness::Goal(explorer.replay(id, None))),
    };
    witness.ok_or_else(|| "SMT witness disagrees with the Behavior evaluator".into())
}

enum Transition {
    Disabled,
    Next(Vec<StateValue>),
    Rejected(Box<CounterexampleTrace>),
}

impl<'a> Explorer<'a> {
    fn new(model: &'a NormalizedBehaviorModel) -> Result<Self, String> {
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
        let mut explorer = Self {
            model,
            positions,
            states: Vec::new(),
            seen: HashMap::new(),
            explored: 0,
            max_depth: 0,
            reached: BTreeMap::new(),
        };
        explorer.insert(initial, None, 0)?;
        Ok(explorer)
    }

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

    fn value(
        &self,
        expr: &TypedExpr,
        values: &[StateValue],
        args: Option<&Row>,
    ) -> Result<StateValue, String> {
        expr.eval(&|name| {
            args.and_then(|args| args.get(name))
                .or_else(|| {
                    self.positions
                        .get(name)
                        .and_then(|index| values.get(*index))
                })
                .cloned()
        })
    }

    fn predicate(&self, expr: &TypedExpr, values: &[StateValue]) -> Result<bool, String> {
        self.value(expr, values, None)?.boolean()
    }

    fn enabled(&self, action: usize, values: &[StateValue]) -> Result<bool, String> {
        let action = &self.model.actions[action];
        action
            .guard
            .as_ref()
            .map(|guard| self.value(guard, values, Some(&action.args))?.boolean())
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
            reached: self
                .reached
                .iter()
                .map(|(name, id)| (name.clone(), self.replay(*id, None)))
                .collect(),
            unreached: self
                .model
                .goals
                .keys()
                .filter(|name| !self.reached.contains_key(*name))
                .cloned()
                .collect(),
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
                if let Some(trace) = self.state_violation(id, diagnostics)? {
                    return Ok(self.outcome("violation_found", end - id - 1, Some(trace)));
                }
                let state = &self.states[id];
                for (name, goal) in &self.model.goals {
                    if !self.reached.contains_key(name) && self.predicate(goal, &state.values)? {
                        self.reached.insert(name.clone(), id);
                    }
                }
            }

            for id in start..end {
                for action_index in 0..self.model.actions.len() {
                    let after = match self.transition(id, action_index)? {
                        Transition::Disabled => continue,
                        Transition::Next(after) => after,
                        Transition::Rejected(trace) => {
                            if depth == self.model.config.max_depth {
                                return Ok(self.outcome("max_depth_reached", 1, None));
                            }
                            return Ok(self.outcome(
                                "violation_found",
                                self.states.len() - end,
                                Some(*trace),
                            ));
                        }
                    };
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
        let reason = if self.reached.len() == self.model.goals.len() {
            "exhausted"
        } else {
            "unreachable_goal"
        };
        Ok(self.outcome(reason, 0, None))
    }

    fn state_violation(
        &self,
        id: usize,
        diagnostics: &mut Vec<BehaviorDiagnostic>,
    ) -> Result<Option<CounterexampleTrace>, String> {
        let state = &self.states[id];
        let depth = state.depth;
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
                location: format!("behavior.{}.invariant.{}", self.model.name, invariant.name),
                related_symbol: None,
            });
            let trace = self.trace(
                id,
                "invariant",
                invariant.name.clone(),
                invariant.source.clone(),
                None,
                None,
            );
            return Ok(Some(trace));
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
                    None,
                );
                return Ok(Some(trace));
            }
        }
        Ok(None)
    }

    fn transition(&self, id: usize, action_index: usize) -> Result<Transition, String> {
        let before = &self.states[id].values;
        if !self.enabled(action_index, before)? {
            return Ok(Transition::Disabled);
        }
        let action = &self.model.actions[action_index];
        let mut after = before.clone();
        for assignment in &action.updates {
            let index = self.positions[assignment.target_var.as_str()];
            let domain = &self.model.domains[index];
            let value = self.value(&assignment.value_expr, before, Some(&action.args))?;
            if !domain.contains(&value) {
                let trace = if let StateValue::Int(attempted_value) = value {
                    let min_bound = domain.min_val.unwrap();
                    let max_bound = domain.max_val.unwrap();
                    let attempt = OverflowAttempt {
                        step_index: self.states[id].depth + 1,
                        from_state: self.snapshot(id),
                        action_name: action.name.clone(),
                        surv_func_refs: action.surv_func_refs.clone(),
                        target_var: assignment.target_var.clone(),
                        attempted_value,
                        min_bound,
                        max_bound,
                    };
                    self.trace(
                        id,
                        "overflow",
                        assignment.target_var.clone(),
                        format!("{min_bound} <= {} <= {max_bound}", assignment.target_var),
                        Some(attempt),
                        Some(action),
                    )
                } else {
                    self.trace(
                        id,
                        "domain",
                        assignment.target_var.clone(),
                        format!("value {value} is outside the declared domain"),
                        None,
                        Some(action),
                    )
                };
                return Ok(Transition::Rejected(Box::new(trace)));
            }
            after[index] = value;
        }
        for (table, inserts) in &action.inserts {
            let index = self.positions[table.as_str()];
            let domain = &self.model.domains[index];
            if let Err(why) =
                self.insert_rows(domain, &mut after[index], inserts, before, &action.args)?
            {
                let trace = self.trace(id, "table_insert", table.clone(), why, None, Some(action));
                return Ok(Transition::Rejected(Box::new(trace)));
            }
        }
        Ok(Transition::Next(after))
    }

    fn insert_rows(
        &self,
        domain: &super::model::StateDomain,
        table: &mut StateValue,
        inserts: &[BTreeMap<String, TypedExpr>],
        before: &[StateValue],
        args: &Row,
    ) -> Result<Result<(), String>, String> {
        let StateValue::Table(table) = table else {
            unreachable!("insertion target was validated");
        };
        let rows = Arc::make_mut(table);
        for insert in inserts {
            let mut row = Row::new();
            for (field, expr) in insert {
                let value = self.value(expr, before, Some(args))?;
                if !domain.table_fields[field].contains(&value) {
                    return Ok(Err(format!(
                        "field '{field}' value {value} is outside its domain"
                    )));
                }
                row.insert(field.clone(), value);
            }
            let StateValue::Enum(id) = &row["id"] else {
                unreachable!("row ID was type checked");
            };
            if rows.contains_key(id) {
                return Ok(Err(format!("duplicate row ID '{id}'")));
            }
            if rows.len() == domain.max_rows.unwrap() {
                return Ok(Err(format!("inserting '{id}' would exceed max_rows")));
            }
            rows.insert(id.clone(), row);
        }
        Ok(Ok(()))
    }

    fn replay_step(&self, action: &NormalizedAction, destination: Option<usize>) -> ReplayStep {
        ReplayStep {
            action: action.name.clone(),
            operation: action.operation.clone(),
            refs: action.surv_func_refs.clone(),
            args: replay_values(&action.args),
            observations: destination.map(|id| {
                self.model
                    .domains
                    .iter()
                    .zip(&self.states[id].values)
                    .filter(|(domain, _)| {
                        self.model
                            .observe
                            .as_ref()
                            .is_some_and(|names| names.contains(&domain.var_name))
                    })
                    .map(|(domain, value)| (domain.var_name.clone(), value.replay_value()))
                    .collect()
            }),
        }
    }

    fn replay(&self, id: usize, failed_action: Option<&NormalizedAction>) -> ReplayTrace {
        let mut current = id;
        let mut steps = Vec::new();
        if let Some(action) = failed_action {
            steps.push(self.replay_step(action, None));
        }
        while let Some((parent, action)) = self.states[current].parent {
            steps.push(self.replay_step(&self.model.actions[action], Some(current)));
            current = parent;
        }
        steps.reverse();
        ReplayTrace {
            version: 1,
            initial: self
                .model
                .domains
                .iter()
                .zip(&self.states[current].values)
                .map(|(domain, value)| (domain.var_name.clone(), value.replay_value()))
                .collect(),
            steps,
        }
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
        failed_action: Option<&NormalizedAction>,
    ) -> CounterexampleTrace {
        let violated_state = failed_action.is_none().then(|| self.snapshot(id));
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
            replay: self.replay(id, failed_action),
        }
    }
}
