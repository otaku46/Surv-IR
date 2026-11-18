use super::expr::{
    identifier, parse_expression, type_expression, ExprAST, ExprType, StateValue, TypedExpr,
};
use super::loader::{BehaviorSource, ProjectContext};
use crate::symbol::{check_package_dependency, reference_candidates};
use crate::{FileImportContext, SymbolKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub struct BehaviorDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub location: String,
    pub related_symbol: Option<String>,
}

impl BehaviorDiagnostic {
    pub(super) fn error(
        code: &str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: "error".into(),
            message: message.into(),
            location: location.into(),
            related_symbol: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BehaviorFileAST {
    pub package: Option<String>,
    pub namespace: Option<String>,
    #[serde(default, rename = "import")]
    pub imports: Vec<String>,
    #[serde(rename = "behavior")]
    pub raw_behaviors: BTreeMap<String, RawBehavior>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawBehavior {
    pub description: Option<String>,
    pub owner: Option<String>,
    pub terminal: Option<String>,
    #[serde(deserialize_with = "deserialize_dotted_values")]
    pub vars: BTreeMap<String, String>,
    #[serde(deserialize_with = "deserialize_dotted_values")]
    pub init: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub action: BTreeMap<String, RawAction>,
    #[serde(default)]
    pub invariant: BTreeMap<String, RawInvariant>,
    #[serde(default)]
    pub check: CheckSettings,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawAction {
    pub guard: Option<String>,
    #[serde(default, deserialize_with = "deserialize_dotted_values")]
    pub update: BTreeMap<String, String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    #[serde(default)]
    pub refs: Vec<String>,
}

fn deserialize_dotted_values<'de, D, T>(deserializer: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    fn flatten<T: serde::de::DeserializeOwned>(
        prefix: &str,
        table: toml::Table,
        result: &mut BTreeMap<String, T>,
    ) -> Result<(), String> {
        for (key, value) in table {
            let name = if prefix.is_empty() {
                key
            } else {
                format!("{prefix}.{key}")
            };
            match value {
                toml::Value::Table(table) => flatten(&name, table, result)?,
                value => {
                    let typed = value
                        .try_into::<T>()
                        .map_err(|err| format!("{name}: {err}"))?;
                    if result.insert(name.clone(), typed).is_some() {
                        return Err(format!("duplicate variable '{name}'"));
                    }
                }
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    flatten("", toml::Table::deserialize(deserializer)?, &mut result)
        .map_err(serde::de::Error::custom)?;
    Ok(result)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawInvariant {
    pub expr: String,
    pub severity: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSettings {
    pub max_states: Option<usize>,
    pub max_depth: Option<usize>,
    pub check_deadlock: Option<bool>,
    #[serde(default)]
    pub bounds: BTreeMap<String, Bound>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Bound {
    Pair([i64; 2]),
    Range(String),
    Named { min: i64, max: i64 },
}

#[derive(Clone, Debug, Serialize)]
pub struct StateDomain {
    pub var_name: String,
    pub domain_kind: &'static str,
    pub variants: Vec<String>,
    pub min_val: Option<i128>,
    pub max_val: Option<i128>,
}

impl StateDomain {
    pub(super) fn expr_type(&self) -> ExprType {
        match self.domain_kind {
            "bool" => ExprType::Bool,
            "enum" => ExprType::Enum(self.variants.clone()),
            _ => ExprType::Int,
        }
    }

    pub(super) fn contains(&self, value: &StateValue) -> bool {
        match value {
            StateValue::Int(value) => self
                .min_val
                .zip(self.max_val)
                .is_none_or(|(lo, hi)| (lo..=hi).contains(value)),
            _ => true,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StateAssignment {
    pub target_var: String,
    pub value_expr: TypedExpr,
}

#[derive(Clone, Debug, Serialize)]
pub struct NormalizedAction {
    pub name: String,
    pub surv_func_refs: Vec<String>,
    pub resolved_func_fqns: Vec<String>,
    pub guard: Option<TypedExpr>,
    pub updates: Vec<StateAssignment>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NormalizedInvariant {
    pub name: String,
    pub expr: TypedExpr,
    pub severity: String,
    pub(super) source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExplorerConfig {
    pub max_states: usize,
    pub max_depth: usize,
    pub check_deadlock: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct NormalizedBehaviorModel {
    pub name: String,
    pub description: Option<String>,
    pub owner_mod: Option<String>,
    pub resolved_owner_fqn: Option<String>,
    pub domains: Vec<StateDomain>,
    pub init_assignments: Vec<StateAssignment>,
    pub actions: Vec<NormalizedAction>,
    pub invariants: Vec<NormalizedInvariant>,
    pub terminal: Option<TypedExpr>,
    pub config: ExplorerConfig,
}

#[derive(Debug, Serialize)]
pub struct FrontendOutcome {
    pub target_name: String,
    pub has_errors: bool,
    pub model: Option<NormalizedBehaviorModel>,
    pub diagnostics: Vec<BehaviorDiagnostic>,
}

impl FrontendOutcome {
    pub(super) fn error(target_name: String, diagnostics: Vec<BehaviorDiagnostic>) -> Self {
        Self {
            target_name,
            has_errors: true,
            model: None,
            diagnostics,
        }
    }
}

type Trees = BTreeMap<Vec<String>, ExprAST>;

pub struct ParsedExpressions {
    pub models: BTreeMap<String, BTreeMap<Vec<String>, ExprAST>>,
}

pub struct SemanticDraft {
    pub model: NormalizedBehaviorModel,
    pub diagnostics: Vec<BehaviorDiagnostic>,
}

pub(super) fn parse_expressions(
    source: &BehaviorSource,
    ast: &BehaviorFileAST,
    mut diagnostics: Vec<BehaviorDiagnostic>,
) -> Result<(ParsedExpressions, Vec<BehaviorDiagnostic>), Vec<BehaviorDiagnostic>> {
    let models = ast
        .raw_behaviors
        .iter()
        .map(|(name, raw)| {
            let location = format!("{}: behavior.{name}", source.path.display());
            (
                name.clone(),
                parse_model_expressions(raw, &location, &mut diagnostics),
            )
        })
        .collect();
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "error")
    {
        Err(diagnostics)
    } else {
        Ok((ParsedExpressions { models }, diagnostics))
    }
}

fn parse_model_expressions(
    raw: &RawBehavior,
    location: &str,
    diagnostics: &mut Vec<BehaviorDiagnostic>,
) -> Trees {
    let mut trees = Trees::new();
    let mut add = |key: &[&str], result: Result<ExprAST, String>| match result {
        Ok(ast) => {
            trees.insert(key.iter().map(|part| (*part).into()).collect(), ast);
        }
        Err(err) => diagnostics.push(BehaviorDiagnostic::error(
            "E_EXPR_SYNTAX",
            format!("{location}.{}", key.join(".")),
            err,
        )),
    };
    for (name, value) in &raw.init {
        let ast = match value {
            toml::Value::Boolean(value) => Ok(ExprAST::Literal {
                value: StateValue::Bool(*value),
            }),
            toml::Value::Integer(value) => Ok(ExprAST::Literal {
                value: StateValue::Int((*value).into()),
            }),
            toml::Value::String(value)
                if raw
                    .vars
                    .get(name)
                    .is_some_and(|ty| ty.trim().starts_with("enum("))
                    && !value.starts_with('\'') =>
            {
                Ok(ExprAST::Literal {
                    value: StateValue::Enum(value.clone()),
                })
            }
            toml::Value::String(value) => parse_expression(value),
            _ => Err("expected a boolean, integer or enum initial value".into()),
        };
        add(&["init", name], ast);
    }
    if let Some(expr) = &raw.terminal {
        add(&["terminal"], parse_expression(expr));
    }
    for (name, action) in &raw.action {
        if let Some(expr) = &action.guard {
            add(&["action", name, "guard"], parse_expression(expr));
        }
        for (var, expr) in &action.update {
            add(&["action", name, "update", var], parse_expression(expr));
        }
    }
    for (name, invariant) in &raw.invariant {
        add(
            &["invariant", name, "expr"],
            parse_expression(&invariant.expr),
        );
    }
    trees
}

fn integer_range(source: &str) -> Result<(i128, i128), String> {
    let source = source.trim();
    let source = source
        .strip_prefix("int(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(source);
    let (lo, hi) = source
        .split_once("..")
        .ok_or("expected an integer range min..max")?;
    let lo = lo.trim().parse().map_err(|_| "invalid minimum integer")?;
    let hi = hi.trim().parse().map_err(|_| "invalid maximum integer")?;
    Ok((lo, hi))
}

fn state_domain(name: &str, text: &str, bound: Option<&Bound>) -> Result<StateDomain, String> {
    if !identifier(name)
        || matches!(
            name,
            "if" | "then" | "else" | "or" | "and" | "not" | "implies" | "in" | "true" | "false"
        )
    {
        return Err(format!("invalid state variable name '{name}'"));
    }
    let mut domain = StateDomain {
        var_name: name.into(),
        domain_kind: "int",
        variants: Vec::new(),
        min_val: None,
        max_val: None,
    };
    let text = text.trim();
    match text {
        "bool" => domain.domain_kind = "bool",
        "int" => {}
        ty if ty.starts_with("int(") => {
            domain.domain_kind = "bounded_int";
            let (lo, hi) = integer_range(ty)?;
            domain.min_val = Some(lo);
            domain.max_val = Some(hi);
        }
        ty if ty.starts_with("enum(") && ty.ends_with(')') => {
            domain.domain_kind = "enum";
            domain.variants = ty[5..ty.len() - 1]
                .split(',')
                .map(|part| part.trim().into())
                .collect();
            if domain.variants.iter().any(|part| !identifier(part))
                || domain.variants.iter().collect::<BTreeSet<_>>().len() != domain.variants.len()
            {
                return Err("enum variants must be distinct, nonempty identifiers".into());
            }
            domain.variants.sort();
        }
        _ => return Err(format!("unsupported state domain '{text}'")),
    }
    if let Some(bound) = bound {
        if domain.domain_kind != "int" {
            return Err("check.bounds applies to unbounded int variables".into());
        }
        let (lo, hi) = match bound {
            Bound::Pair([lo, hi]) | Bound::Named { min: lo, max: hi } => {
                ((*lo).into(), (*hi).into())
            }
            Bound::Range(range) => integer_range(range)?,
        };
        domain.domain_kind = "bounded_int";
        domain.min_val = Some(lo);
        domain.max_val = Some(hi);
    }
    if domain
        .min_val
        .zip(domain.max_val)
        .is_some_and(|(lo, hi)| lo > hi)
    {
        return Err("minimum must not exceed maximum".into());
    }
    Ok(domain)
}

pub(super) fn check_behavior_semantics(
    ast: &BehaviorFileAST,
    mut expressions: ParsedExpressions,
    project: &ProjectContext,
    context: &FileImportContext,
    diagnostics: &[BehaviorDiagnostic],
) -> Vec<SemanticDraft> {
    ast.raw_behaviors
        .iter()
        .map(|(name, raw)| {
            let location = format!("{}: behavior.{name}", context.file_path.display());
            let mut diagnostics = diagnostics.to_vec();
            let trees = expressions
                .models
                .remove(name)
                .expect("expressions were parsed for every Behavior");
            let model = check_model_semantics(
                name,
                raw,
                trees,
                project,
                context,
                &location,
                &mut diagnostics,
            );
            SemanticDraft { model, diagnostics }
        })
        .collect()
}

fn check_model_semantics(
    name: &str,
    raw: &RawBehavior,
    mut trees: Trees,
    project: &ProjectContext,
    context: &FileImportContext,
    location: &str,
    diagnostics: &mut Vec<BehaviorDiagnostic>,
) -> NormalizedBehaviorModel {
    let mut domains = Vec::new();
    for (var, text) in &raw.vars {
        match state_domain(var, text, raw.check.bounds.get(var)) {
            Ok(domain) => domains.push(domain),
            Err(err) => diagnostics.push(BehaviorDiagnostic::error(
                "E_STATE_DOMAIN",
                format!("{location}.vars.{var}"),
                err,
            )),
        }
        if !raw.init.contains_key(var) {
            diagnostics.push(BehaviorDiagnostic::error(
                "E_UNINITIALIZED",
                format!("{location}.init.{var}"),
                "missing initial value",
            ));
        }
    }
    for var in raw
        .check
        .bounds
        .keys()
        .filter(|var| !raw.vars.contains_key(*var))
    {
        diagnostics.push(BehaviorDiagnostic::error(
            "E_STATE_DOMAIN",
            format!("{location}.check.bounds.{var}"),
            "bounds refer to an undeclared variable",
        ));
    }
    let config = ExplorerConfig {
        max_states: raw
            .check
            .max_states
            .unwrap_or(project.verification.default_max_states),
        max_depth: raw
            .check
            .max_depth
            .unwrap_or(project.verification.default_max_depth),
        check_deadlock: raw.check.check_deadlock.unwrap_or(true),
    };
    if config.max_states == 0 {
        diagnostics.push(BehaviorDiagnostic::error(
            "E_EXPLORER_CONFIG",
            format!("{location}.check.max_states"),
            "max_states must be positive",
        ));
    }

    let resolved_owner_fqn = raw.owner.as_ref().and_then(|owner| {
        resolve_symbol(
            owner,
            SymbolKind::Mod,
            project,
            context,
            &format!("{location}.owner"),
            diagnostics,
        )
    });
    let mut check = |key: &[&str],
                     domain: Option<&StateDomain>,
                     is_assignment: bool,
                     diagnostics: &mut Vec<BehaviorDiagnostic>| {
        let ast = trees.remove(
            &key.iter()
                .map(|part| (*part).to_string())
                .collect::<Vec<_>>(),
        )?;
        let loc = format!("{location}.{}", key.join("."));
        if is_assignment && domain.is_none() {
            diagnostics.push(BehaviorDiagnostic::error(
                "E_ASSIGNMENT_TARGET",
                loc,
                "assignment target is not a declared variable",
            ));
            return None;
        }
        let ty = domain.map_or(ExprType::Bool, StateDomain::expr_type);
        let available = if key[0] == "init" { &[][..] } else { &domains };
        match type_expression(ast, available, &ty) {
            Ok(expr) => {
                if let Some(domain) = domain.filter(|_| key[0] == "init") {
                    match expr.eval(&|_| None) {
                        Ok(value) if domain.contains(&value) => {}
                        Ok(value) => diagnostics.push(BehaviorDiagnostic::error(
                            "E_INITIAL_DOMAIN",
                            loc,
                            format!("initial value {value} is out of bounds"),
                        )),
                        Err(err) => {
                            diagnostics.push(BehaviorDiagnostic::error("E_INITIAL_VALUE", loc, err))
                        }
                    }
                }
                Some(expr)
            }
            Err(err) => {
                diagnostics.push(BehaviorDiagnostic::error("E_EXPR_TYPE", loc, err));
                None
            }
        }
    };

    let init_assignments = raw
        .init
        .keys()
        .filter_map(|var| {
            check(
                &["init", var],
                domains.iter().find(|domain| domain.var_name == *var),
                true,
                diagnostics,
            )
            .map(|value_expr| StateAssignment {
                target_var: var.clone(),
                value_expr,
            })
        })
        .collect();
    let terminal = check(&["terminal"], None, false, diagnostics);
    let mut actions = Vec::new();
    for (name, action) in &raw.action {
        let surv_func_refs: Vec<_> = action
            .reference
            .iter()
            .chain(&action.refs)
            .cloned()
            .collect();
        let resolved_func_fqns = surv_func_refs
            .iter()
            .filter_map(|reference| {
                resolve_symbol(
                    reference,
                    SymbolKind::Func,
                    project,
                    context,
                    &format!("{location}.action.{name}"),
                    diagnostics,
                )
            })
            .collect();
        let guard = check(&["action", name, "guard"], None, false, diagnostics);
        let updates = action
            .update
            .keys()
            .filter_map(|var| {
                check(
                    &["action", name, "update", var],
                    domains.iter().find(|domain| domain.var_name == *var),
                    true,
                    diagnostics,
                )
                .map(|value_expr| StateAssignment {
                    target_var: var.clone(),
                    value_expr,
                })
            })
            .collect();
        actions.push(NormalizedAction {
            name: name.clone(),
            surv_func_refs,
            resolved_func_fqns,
            guard,
            updates,
        });
    }
    let mut invariants = Vec::new();
    for (name, invariant) in &raw.invariant {
        let severity = invariant.severity.as_deref().unwrap_or("error");
        if !matches!(severity, "warning" | "error") {
            diagnostics.push(BehaviorDiagnostic::error(
                "E_INVARIANT_SEVERITY",
                format!("{location}.invariant.{name}"),
                "severity must be error or warning",
            ));
        }
        if let Some(expr) = check(&["invariant", name, "expr"], None, false, diagnostics) {
            invariants.push(NormalizedInvariant {
                name: name.clone(),
                expr,
                severity: severity.into(),
                source: invariant.expr.clone(),
            });
        }
    }
    NormalizedBehaviorModel {
        name: name.into(),
        description: raw.description.clone(),
        owner_mod: raw.owner.clone(),
        resolved_owner_fqn,
        domains,
        init_assignments,
        actions,
        invariants,
        terminal,
        config,
    }
}

fn resolve_symbol(
    reference: &str,
    kind: SymbolKind,
    project: &ProjectContext,
    context: &FileImportContext,
    location: &str,
    diagnostics: &mut Vec<BehaviorDiagnostic>,
) -> Option<String> {
    let expected = if kind == SymbolKind::Mod {
        "mod"
    } else {
        "func"
    };
    if !reference.starts_with(&format!("{expected}."))
        && !reference.contains(&format!(".{expected}."))
    {
        diagnostics.push(BehaviorDiagnostic::error(
            "E_BEHAVIOR_REFERENCE",
            location,
            format!("expected a {expected}.* reference, found '{reference}'"),
        ));
        return None;
    }
    let matches = reference_candidates(&project.symbol_table, kind, reference, context);
    match matches.as_slice() {
        [entry] => {
            if let Some(err) =
                check_package_dependency(project.manifest.as_ref(), context, &entry.package)
            {
                diagnostics.push(BehaviorDiagnostic::error(&err.kind, location, err.message));
                return None;
            }
            Some(entry.fq_name.clone())
        }
        _ => {
            let (code, message) = if matches.is_empty() {
                (
                    "E_BEHAVIOR_UNRESOLVED",
                    format!("unresolved reference '{reference}'"),
                )
            } else {
                (
                    "E_BEHAVIOR_AMBIGUOUS",
                    format!(
                        "ambiguous reference '{reference}': {}",
                        matches
                            .iter()
                            .map(|entry| entry.fq_name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            };
            let mut diagnostic = BehaviorDiagnostic::error(code, location, message);
            diagnostic.related_symbol = Some(reference.into());
            diagnostics.push(diagnostic);
            None
        }
    }
}

pub(super) fn build_frontend_outcome(draft: SemanticDraft) -> FrontendOutcome {
    let SemanticDraft { model, diagnostics } = draft;
    let name = model.name.clone();
    if diagnostics.iter().any(|diag| diag.severity == "error") {
        FrontendOutcome::error(name, diagnostics)
    } else {
        FrontendOutcome {
            target_name: name,
            has_errors: false,
            model: Some(model),
            diagnostics,
        }
    }
}
