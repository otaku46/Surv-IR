use super::expr::{infer, ExprAST, ExprType, Op, StateValue};
use super::model::{NormalizedBehaviorModel, StateDomain};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Debug, Serialize)]
pub struct TlaPlusSpec {
    pub module_name: String,
    pub tla_source: String,
    pub cfg_source: String,
}

pub fn generate_tla_spec(model: &NormalizedBehaviorModel) -> TlaPlusSpec {
    let module_name = format!("Surv_{}", safe_name(&model.name));
    let vars: BTreeMap<_, _> = model
        .domains
        .iter()
        .enumerate()
        .map(|(index, domain)| {
            (
                domain.var_name.as_str(),
                format!("v_{index}_{}", safe_name(&domain.var_name)),
            )
        })
        .collect();
    let enums: BTreeMap<_, _> = model
        .domains
        .iter()
        .flat_map(|domain| domain.variants.iter().map(String::as_str))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, literal)| (literal, index + 2))
        .collect();
    let mut tla_source = format!("---- MODULE {module_name} ----\nEXTENDS Integers\n\n");
    for (name, identifier) in &vars {
        let _ = writeln!(tla_source, "\\* {identifier} = {name}");
    }
    for (literal, value) in &enums {
        let _ = writeln!(tla_source, "\\* enum {literal} = {value}");
    }
    let mut all_vars: Vec<_> = vars.values().map(String::as_str).collect();
    all_vars.push("surv_overflow");
    let _ = writeln!(
        tla_source,
        "VARIABLES {}\nvars == <<{}>>\n",
        all_vars.join(", "),
        all_vars.join(", ")
    );
    let renderer = Renderer {
        vars: &vars,
        enums: &enums,
        domains: &model.domains,
    };
    let initial = model
        .init_assignments
        .iter()
        .map(|assignment| {
            format!(
                "{} = {}",
                vars[assignment.target_var.as_str()],
                renderer.render(&assignment.value_expr.ast)
            )
        })
        .collect::<Vec<_>>();
    tla_source.push_str("Init ==\n    /\\ surv_overflow = FALSE\n");
    for condition in initial {
        let _ = writeln!(tla_source, "    /\\ {condition}");
    }
    tla_source.push('\n');
    let terminal = model
        .terminal
        .as_ref()
        .map(|expr| renderer.render(&expr.ast))
        .unwrap_or_else(|| "FALSE".into());
    let _ = writeln!(
        tla_source,
        "Terminal == {terminal}\nNoOverflow == ~surv_overflow\n"
    );
    let mut action_names = Vec::new();
    for (index, action) in model.actions.iter().enumerate() {
        let name = format!("Action_{index}_{}", safe_name(&action.name));
        let condition = action
            .guard
            .as_ref()
            .map(|expr| renderer.render(&expr.ast))
            .unwrap_or_else(|| "TRUE".into());
        let updates: BTreeMap<_, _> = action
            .updates
            .iter()
            .map(|assignment| {
                (
                    assignment.target_var.as_str(),
                    renderer.render(&assignment.value_expr.ast),
                )
            })
            .collect();
        let unchanged: Vec<_> = vars
            .iter()
            .filter(|(name, _)| !updates.contains_key(*name))
            .map(|(_, value)| value.as_str())
            .collect();
        let bounds: Vec<_> = model
            .domains
            .iter()
            .filter_map(|domain| {
                let (lo, hi) = domain.min_val.zip(domain.max_val)?;
                let expr = updates.get(domain.var_name.as_str())?;
                Some(format!("({lo} <= {expr} /\\ {expr} <= {hi})"))
            })
            .collect();
        let _ = writeln!(
            tla_source,
            "{name} ==\n    /\\ ~surv_overflow\n    /\\ {condition}"
        );
        let mut effects = vec!["surv_overflow' = FALSE".to_string()];
        effects.extend(
            updates
                .iter()
                .map(|(name, expr)| format!("{}' = {expr}", vars[*name])),
        );
        if !unchanged.is_empty() {
            effects.push(format!("UNCHANGED <<{}>>", unchanged.join(", ")));
        }
        if bounds.is_empty() {
            for effect in effects {
                let _ = writeln!(tla_source, "    /\\ {effect}");
            }
        } else {
            let plain_vars: Vec<_> = vars.values().map(String::as_str).collect();
            let _ = writeln!(tla_source, "    /\\ IF {}\n       THEN ({})\n       ELSE (surv_overflow' = TRUE /\\ UNCHANGED <<{}>>) ",
                bounds.join(" /\\ "), effects.join(" /\\ "), plain_vars.join(", "));
        }
        tla_source.push('\n');
        action_names.push(name);
    }

    let _ = writeln!(
        tla_source,
        "Next ==\n    \\/ ({} /\\ UNCHANGED vars)",
        if model.config.check_deadlock {
            "Terminal"
        } else {
            "TRUE"
        }
    );
    for action in action_names {
        let _ = writeln!(tla_source, "    \\/ {action}");
    }
    tla_source.push_str("\nSpec == Init /\\ [][Next]_vars\n\n");
    let mut cfg_source = String::from("SPECIFICATION Spec\nINVARIANT NoOverflow\n");
    for (index, invariant) in model.invariants.iter().enumerate() {
        let name = format!("Invariant_{index}_{}", safe_name(&invariant.name));
        let _ = writeln!(
            tla_source,
            "{name} == {}",
            renderer.render(&invariant.expr.ast)
        );
        let _ = writeln!(cfg_source, "INVARIANT {name}");
    }
    if !model.config.check_deadlock {
        cfg_source.push_str("CHECK_DEADLOCK FALSE\n");
    }
    tla_source.push_str("\n====\n");
    TlaPlusSpec {
        module_name,
        tla_source,
        cfg_source,
    }
}

fn safe_name(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

struct Renderer<'a> {
    vars: &'a BTreeMap<&'a str, String>,
    enums: &'a BTreeMap<&'a str, usize>,
    domains: &'a [StateDomain],
}

impl Renderer<'_> {
    fn render(&self, expr: &ExprAST) -> String {
        match expr {
            ExprAST::Literal { value } => match value {
                StateValue::Bool(value) => (if *value { "TRUE" } else { "FALSE" }).into(),
                StateValue::Int(value) => format!("({value})"),
                StateValue::Enum(value) => self
                    .enums
                    .get(value.as_str())
                    .map_or_else(|| serde_json::to_string(value).unwrap(), usize::to_string),
            },
            ExprAST::Variable { name } => self.vars[name.as_str()].clone(),
            ExprAST::Not { arg } => format!("(~{})", self.render(arg)),
            ExprAST::Neg { arg } => format!("(-{})", self.render(arg)),
            ExprAST::If { condition, yes, no } => format!(
                "(IF {} THEN {} ELSE {})",
                self.render(condition),
                self.render(yes),
                self.render(no)
            ),
            ExprAST::InSet { value, choices } => format!(
                "({} \\in {{{}}})",
                self.render(value),
                choices
                    .iter()
                    .map(|choice| self.render(choice))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ExprAST::Binary { op, left, right } => {
                let boolean_order = matches!(op, Op::Lt | Op::Le | Op::Gt | Op::Ge)
                    && infer(left, self.domains, None).is_ok_and(|ty| ty == ExprType::Bool);
                let left = self.render(left);
                let right = self.render(right);
                let op = match op {
                    Op::Add => "+",
                    Op::Sub => "-",
                    Op::Mul => "*",
                    Op::Eq => "=",
                    Op::Ne => "#",
                    Op::Lt => "<",
                    Op::Le => "<=",
                    Op::Gt => ">",
                    Op::Ge => ">=",
                    Op::And => "/\\",
                    Op::Or => "\\/",
                    Op::Implies => "=>",
                    Op::In => "\\in",
                };
                if boolean_order {
                    format!("((IF {left} THEN 1 ELSE 0) {op} (IF {right} THEN 1 ELSE 0))")
                } else {
                    format!("({left} {op} {right})")
                }
            }
        }
    }
}
