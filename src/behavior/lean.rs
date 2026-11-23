mod proof;

pub use proof::{LeanProof, LeanProofMode};

use super::model::NormalizedBehaviorModel;
use super::symbolic::{lower, Checked, Node, Operator, Sort, SymbolicBehavior, Term};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt::Write;

#[derive(Debug, Serialize)]
pub struct LeanSpec {
    pub namespace: String,
    pub lean_source: String,
    pub proof: LeanProof,
}

pub fn generate_lean_spec(model: &NormalizedBehaviorModel) -> Result<LeanSpec, String> {
    generate_lean_spec_with_proof(model, LeanProofMode::Auto)
}

pub fn generate_lean_spec_with_proof(
    model: &NormalizedBehaviorModel,
    mode: LeanProofMode,
) -> Result<LeanSpec, String> {
    let symbolic = lower(model)?;
    let namespace = format!("Surv_{}", identifier(&model.name));
    let mut source = format!("import Std\n\nnamespace {namespace}\n\n");
    source.push_str("structure State where\n");
    for (index, column) in symbolic.columns.iter().enumerate() {
        let sort = match column.sort {
            Sort::Bool => "Bool",
            Sort::Int => "Int",
        };
        let _ = writeln!(source, "  c{index} : {sort} -- {:?}", column.label);
    }
    source.push_str("  deriving DecidableEq, Repr\n\n");
    let initial = symbolic
        .columns
        .iter()
        .map(|column| column.initial)
        .collect::<Vec<_>>();
    let _ = writeln!(
        source,
        "def initial : State := {}\n",
        record(&symbolic, &initial)
    );

    source.push_str("namespace Symbol\n");
    for (name, number) in &symbolic.symbols {
        let _ = writeln!(
            source,
            "def v{number}_{} : Int := {number}",
            identifier(name)
        );
    }
    source.push_str("end Symbol\n\n");

    predicate(&mut source, &symbolic, "terminal", symbolic.terminal);
    for (index, invariant) in model.invariants.iter().enumerate() {
        predicate(
            &mut source,
            &symbolic,
            &format!("invariant_{index}_{}", identifier(&invariant.name)),
            symbolic.invariants[index],
        );
    }
    for (index, (name, goal)) in symbolic.goals.iter().enumerate() {
        predicate(
            &mut source,
            &symbolic,
            &format!("goal_{index}_{}", identifier(name)),
            *goal,
        );
    }

    source.push_str(
        "inductive Transition where\n\
         \x20 | disabled\n\
         \x20 | evaluationError\n\
         \x20 | rejected\n\
         \x20 | next (state : State)\n\
         \x20 deriving DecidableEq, Repr\n\n",
    );
    let mut actions = Vec::new();
    for (index, action) in symbolic.actions.iter().enumerate() {
        let name = format!("action_{index}_{}", identifier(&model.actions[index].name));
        let mut roots = vec![action.error, action.rejected, action.success];
        roots.extend(&action.next);
        definition(&mut source, &symbolic, &name, "Transition", &roots);
        let success = reference(&symbolic, action.success);
        let rejected = reference(&symbolic, action.rejected);
        let error = reference(&symbolic, action.error);
        let _ = writeln!(
            source, "  if {success} then .next ({})\n  else if {error} then .evaluationError\n  else if {rejected} then .rejected\n  else .disabled\n",
            record(&symbolic, &action.next)
        );
        actions.push(name);
    }

    let _ = writeln!(
        source,
        "def actions : List (State → Transition) := [{}]\n",
        actions.join(", ")
    );
    source.push_str(
        r#"def successors (s : State) : List State :=
  actions.filterMap fun action =>
    match action s with
    | .next next => some next
    | _ => none

inductive Reachable : State → Prop where
  | initial : Reachable initial
  | step {before after : State} :
      Reachable before → after ∈ successors before → Reachable after

theorem reachable_induction {p : State → Prop}
    (base : p initial)
    (preserve : ∀ before after, after ∈ successors before → p before → p after)
    {s : State} (reachable : Reachable s) : p s := by
  induction reachable with
  | initial => exact base
  | step _ step ih => exact preserve _ _ step ih

"#,
    );
    let proof = proof::emit(&mut source, model, &symbolic, mode);
    let _ = writeln!(source, "end {namespace}");
    Ok(LeanSpec {
        namespace,
        lean_source: source,
        proof,
    })
}

fn identifier(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn predicate(source: &mut String, model: &SymbolicBehavior, name: &str, expr: Checked) {
    definition(
        source,
        model,
        name,
        "Option Bool",
        &[expr.defined, expr.value],
    );
    let _ = writeln!(
        source,
        "  if {} then some {} else none\n",
        reference(model, expr.defined),
        reference(model, expr.value)
    );
}

fn definition(
    source: &mut String,
    model: &SymbolicBehavior,
    name: &str,
    sort: &str,
    roots: &[Term],
) {
    let mut applications = BTreeSet::new();
    let mut pending = roots.to_vec();
    let mut uses_state = false;
    while let Some(term) = pending.pop() {
        match &model.terms.nodes[term].0 {
            Node::Column(_) => uses_state = true,
            Node::Apply(_, args) if applications.insert(term) => pending.extend(args),
            _ => {}
        }
    }
    let param = if uses_state { "s" } else { "_s" };
    let _ = writeln!(source, "def {name} ({param} : State) : {sort} :=");
    for term in applications {
        let Node::Apply(operator, args) = &model.terms.nodes[term].0 else {
            unreachable!()
        };
        let args: Vec<_> = args.iter().map(|term| reference(model, *term)).collect();
        let expr = operation(*operator, &args);
        let _ = writeln!(source, "  let t{term} := {expr}");
    }
}

fn record(model: &SymbolicBehavior, values: &[Term]) -> String {
    render_state(values.iter().map(|term| reference(model, *term)))
}

fn render_state(values: impl IntoIterator<Item = String>) -> String {
    let fields: Vec<_> = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| format!("c{index} := {value}"))
        .collect();
    if fields.is_empty() {
        "State.mk".into()
    } else {
        format!("{{ {} }}", fields.join(", "))
    }
}

fn reference(model: &SymbolicBehavior, term: Term) -> String {
    match &model.terms.nodes[term].0 {
        Node::Bool(value) => value.to_string(),
        Node::Int(value) => format!("({value} : Int)"),
        Node::Column(column) => format!("s.c{column}"),
        Node::Apply(..) => format!("t{term}"),
    }
}

fn operation(operator: Operator, args: &[String]) -> String {
    match operator {
        Operator::Not => return format!("!{}", args[0]),
        Operator::Ite => return format!("if {} then {} else {}", args[0], args[1], args[2]),
        Operator::And => return args.join(" && "),
        Operator::Or => return args.join(" || "),
        _ => {}
    }
    let operator = match operator {
        Operator::Eq => "==",
        Operator::Lt => "<",
        Operator::Le => "≤",
        Operator::Add => "+",
        Operator::Sub => "-",
        Operator::Mul => "*",
        _ => unreachable!(),
    };
    let expr = format!("{} {operator} {}", args[0], args[1]);
    if matches!(operator, "<" | "≤") {
        format!("decide ({expr})")
    } else {
        expr
    }
}
