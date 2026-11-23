use super::{identifier, reference, render_state};
use crate::behavior::explorer::{collect_lean_evidence, LeanEvidence};
use crate::behavior::expr::StateValue;
use crate::behavior::model::NormalizedBehaviorModel;
use crate::behavior::symbolic::SymbolicBehavior;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt::Write;

#[derive(Clone, Copy, Debug, Default)]
pub enum LeanProofMode {
    #[default]
    Auto,
    Obligations,
    None,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LeanProof {
    Certificate {
        states: usize,
    },
    Obligations {
        reason: &'static str,
    },
    Mixed {
        reason: &'static str,
        witnessed_goals: usize,
    },
    Witnesses {
        reason: &'static str,
        witnessed_goals: usize,
    },
    None,
}

pub(super) fn emit(
    out: &mut String,
    model: &NormalizedBehaviorModel,
    symbolic: &SymbolicBehavior,
    mode: LeanProofMode,
) -> LeanProof {
    if matches!(mode, LeanProofMode::None)
        || (model.invariants.is_empty() && model.goals.is_empty())
    {
        return LeanProof::None;
    }
    let evidence = match mode {
        LeanProofMode::Auto => collect_lean_evidence(model),
        LeanProofMode::Obligations => LeanEvidence {
            stop_reason: "requested",
            ..LeanEvidence::default()
        },
        LeanProofMode::None => unreachable!(),
    };
    let closed = evidence.is_closed();
    let closure_needed =
        closed && (!model.invariants.is_empty() || evidence.goals.len() != model.goals.len());
    let _ = writeln!(out, "-- Proof generation: {}\n", evidence.stop_reason);
    let mut witness_states = BTreeSet::new();
    for &goal in evidence.goals.values() {
        let mut current = Some(goal);
        while let Some(id) = current {
            if !witness_states.insert(id) {
                break;
            }
            current = evidence.states[id].parent;
        }
    }
    let certificate = closure_needed || !witness_states.is_empty();
    if certificate {
        let size = if closure_needed {
            evidence.states.len()
        } else {
            witness_states.len()
        };
        let depth = size.saturating_mul(32).max(2048);
        let heartbeats = size.saturating_mul(10_000).max(200_000);
        let _ = writeln!(
            out,
            "section
set_option maxRecDepth {depth}
set_option maxHeartbeats {heartbeats}
"
        );
    }
    for (id, state) in evidence.states.iter().enumerate() {
        if closure_needed || witness_states.contains(&id) {
            let value = if id == 0 {
                "initial".into()
            } else {
                concrete_state(symbolic, &state.values)
            };
            let _ = writeln!(out, "private def certState{id} : State := {value}");
        }
    }
    if closure_needed {
        emit_closure(out, evidence.states.len());
    }
    if !closed && !model.invariants.is_empty() {
        out.push_str(INDUCTIVE_OBLIGATIONS);
    }

    for (index, invariant) in model.invariants.iter().enumerate() {
        let name = format!("invariant_{index}_{}", identifier(&invariant.name));
        if closed {
            emit_invariant(out, &name);
        } else {
            emit_invariant_obligation(out, &name);
        }
    }

    for id in witness_states {
        let proof = match evidence.states[id].parent {
            None => "Reachable.initial".into(),
            Some(parent) => format!("Reachable.step certReach{parent} (by decide)"),
        };
        let _ = writeln!(
            out,
            "\nprivate theorem certReach{id} : Reachable certState{id} :=\n  {proof}"
        );
    }
    for (index, name) in model.goals.keys().enumerate() {
        let goal = format!("goal_{index}_{}", identifier(name));
        let _ = writeln!(
            out,
            "\ndef claim_{goal} : Prop := ∃ s, Reachable s ∧ {goal} s = some true"
        );
        if let Some(id) = evidence.goals.get(name) {
            let _ = writeln!(out, "\ntheorem certified_{goal} : claim_{goal} :=\n  ⟨certState{id}, certReach{id}, by decide⟩");
        } else if closed {
            emit_unreachable(out, &goal);
        } else {
            let _ = writeln!(
                out,
                "\n-- example : claim_{goal} := by\n--   refine ⟨?witness, ?reachability, ?satisfies⟩\n"
            );
        }
    }

    if certificate {
        out.push_str(
            "
end

",
        );
    }
    if closed {
        LeanProof::Certificate {
            states: evidence.states.len(),
        }
    } else if evidence.goals.is_empty() {
        LeanProof::Obligations {
            reason: evidence.stop_reason,
        }
    } else if model.invariants.is_empty() && evidence.goals.len() == model.goals.len() {
        LeanProof::Witnesses {
            reason: evidence.stop_reason,
            witnessed_goals: evidence.goals.len(),
        }
    } else {
        LeanProof::Mixed {
            reason: evidence.stop_reason,
            witnessed_goals: evidence.goals.len(),
        }
    }
}

fn concrete_state(model: &SymbolicBehavior, state: &[StateValue]) -> String {
    render_state(
        model
            .columns
            .iter()
            .map(|column| match column.origin.project(state) {
                Some(StateValue::Bool(value)) => value.to_string(),
                Some(StateValue::Int(value)) => format!("({value} : Int)"),
                Some(StateValue::Enum(value)) => format!("({} : Int)", model.symbols[&value]),
                Some(_) => unreachable!("state columns contain scalar values"),
                // Tables are append-only; absent cells keep their canonical initial value.
                None => reference(model, column.initial),
            }),
    )
}

fn emit_closure(out: &mut String, count: usize) {
    let _ = writeln!(out, "\nprivate def certStates : List State := [");
    for index in 0..count {
        let comma = if index + 1 == count { "" } else { "," };
        let _ = writeln!(out, "  certState{index}{comma}");
    }
    out.push_str(
        r#"]

private theorem certClosed :
    (certStates.all fun s => (successors s).all certStates.contains) = true := by
  decide

private theorem certContains {s : State} (reachable : Reachable s) : s ∈ certStates := by
  apply reachable_induction (p := fun x => x ∈ certStates) _ _ reachable
  · decide
  · intro before after step member
    have checked := List.all_eq_true.mp certClosed before member
    exact List.contains_iff_mem.mp (List.all_eq_true.mp checked after step)

"#,
    );
}

fn emit_invariant(out: &mut String, name: &str) {
    let _ = writeln!(
        out,
        r#"private theorem certAll_{name} :
    (certStates.all fun s => {name} s == some true) = true := by
  decide

theorem certified_{name} {{s : State}} (reachable : Reachable s) :
    {name} s = some true :=
  LawfulBEq.eq_of_beq (List.all_eq_true.mp certAll_{name} s (certContains reachable))
"#
    );
}

fn emit_unreachable(out: &mut String, name: &str) {
    let _ = writeln!(
        out,
        r#"
private theorem certAbsent_{name} :
    (certStates.all fun s => !({name} s == some true)) = true := by
  decide

theorem unreachable_{name} : ¬ claim_{name} := by
  rintro ⟨s, reachable, found⟩
  have checked := List.all_eq_true.mp certAbsent_{name} s (certContains reachable)
  simp [found] at checked
"#
    );
}

pub(super) const INDUCTIVE_OBLIGATIONS: &str = r#"structure InvariantObligations (aux claim : State → Prop) : Prop where
  initial : aux initial
  preservation : ∀ before after, after ∈ successors before → aux before → aux after
  implication : ∀ s, aux s → claim s

"#;

fn emit_invariant_obligation(out: &mut String, name: &str) {
    let _ = writeln!(
        out,
        r#"theorem prove_{name} {{aux : State → Prop}}
    (proof : InvariantObligations aux (fun s => {name} s = some true))
    {{s : State}} (reachable : Reachable s) : {name} s = some true :=
  proof.implication s (reachable_induction proof.initial proof.preservation reachable)

-- Replace True with an auxiliary invariant, then uncomment these lines.
-- private def aux_{name} (_s : State) : Prop := True
-- example : InvariantObligations aux_{name} (fun s => {name} s = some true) := by
--   refine {{ initial := ?initial, preservation := ?preservation, implication := ?implication }}
"#
    );
}
