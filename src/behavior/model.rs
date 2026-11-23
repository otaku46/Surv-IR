use super::expr::{
    identifier, parse_expression, type_expression, ExprAST, ExprType, Row, Rows, StateValue,
    TypedExpr,
};
use super::loader::{BehaviorSource, ProjectContext};
use crate::symbol::{check_package_dependency, reference_candidates};
use crate::{FileImportContext, SymbolKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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
    #[serde(default, deserialize_with = "deserialize_dotted_values")]
    pub vars: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "deserialize_dotted_values")]
    pub init: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub action: BTreeMap<String, RawAction>,
    #[serde(default)]
    pub tables: BTreeMap<String, RawTable>,
    #[serde(default)]
    pub reach: BTreeMap<String, RawGoal>,
    pub observe: Option<Vec<String>>,
    #[serde(default)]
    pub invariant: BTreeMap<String, RawInvariant>,
    #[serde(default)]
    pub check: CheckSettings,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawAction {
    pub guard: Option<String>,
    pub operation: Option<String>,
    #[serde(default)]
    pub args: BTreeMap<String, RawArgument>,
    #[serde(default)]
    pub insert: BTreeMap<String, Vec<BTreeMap<String, String>>>,
    #[serde(default, deserialize_with = "deserialize_dotted_values")]
    pub update: BTreeMap<String, String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawTable {
    pub schema: String,
    pub max_rows: usize,
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub initial: Vec<BTreeMap<String, toml::Value>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawArgument {
    #[serde(rename = "type")]
    pub domain: String,
    pub value: toml::Value,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawGoal {
    pub expr: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_schema: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub table_fields: BTreeMap<String, StateDomain>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rows: Option<usize>,
}

impl StateDomain {
    pub(super) fn expr_type(&self) -> ExprType {
        match self.domain_kind {
            "bool" => ExprType::Bool,
            "enum" => ExprType::Enum(self.variants.clone()),
            "id" => ExprType::Id(self.id_schema.clone().unwrap(), self.variants.clone()),
            "table" => ExprType::Table(
                self.table_fields
                    .iter()
                    .map(|(name, field)| (name.clone(), field.expr_type()))
                    .collect(),
            ),
            _ => ExprType::Int,
        }
    }

    pub(super) fn contains(&self, value: &StateValue) -> bool {
        match value {
            StateValue::Int(value) => self
                .min_val
                .zip(self.max_val)
                .is_none_or(|(lo, hi)| (lo..=hi).contains(value)),
            StateValue::Enum(value) => self.variants.contains(value),
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
    pub operation: String,
    pub args: Row,
    pub inserts: BTreeMap<String, Vec<BTreeMap<String, TypedExpr>>>,
    pub updates: Vec<StateAssignment>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NormalizedInvariant {
    pub name: String,
    pub expr: TypedExpr,
    pub severity: String,
    pub(super) source: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationBackend {
    #[default]
    Internal,
    Smt,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExplorerConfig {
    pub backend: VerificationBackend,
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
    pub goals: BTreeMap<String, TypedExpr>,
    pub observe: Option<Vec<String>>,
    pub terminal: Option<TypedExpr>,
    pub config: ExplorerConfig,
}

impl NormalizedBehaviorModel {
    pub(super) fn expressions(&self) -> impl Iterator<Item = &TypedExpr> {
        self.init_assignments
            .iter()
            .map(|assignment| &assignment.value_expr)
            .chain(self.terminal.iter())
            .chain(self.invariants.iter().map(|invariant| &invariant.expr))
            .chain(self.goals.values())
            .chain(self.actions.iter().flat_map(|action| {
                action
                    .guard
                    .iter()
                    .chain(
                        action
                            .updates
                            .iter()
                            .map(|assignment| &assignment.value_expr),
                    )
                    .chain(
                        action
                            .inserts
                            .values()
                            .flatten()
                            .flat_map(|row| row.values()),
                    )
            }))
    }
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
                if raw.vars.get(name).is_some_and(|ty| {
                    ty.trim().starts_with("enum(") || ty.trim().starts_with("id(")
                }) && !value.starts_with('\'') =>
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
    for (name, goal) in &raw.reach {
        add(&["reach", name, "expr"], parse_expression(&goal.expr));
    }
    for (name, action) in &raw.action {
        for (table, rows) in &action.insert {
            for (index, row) in rows.iter().enumerate() {
                for (field, expr) in row {
                    add(
                        &["action", name, "insert", table, &index.to_string(), field],
                        parse_expression(expr),
                    );
                }
            }
        }
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
        id_schema: None,
        table_fields: BTreeMap::new(),
        max_rows: None,
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

type Resolve<'a> =
    dyn FnMut(&str, SymbolKind, &str, &mut Vec<BehaviorDiagnostic>) -> Option<String> + 'a;

fn resolved_domain(
    name: &str,
    text: &str,
    bound: Option<&Bound>,
    location: &str,
    resolve: &mut Resolve<'_>,
    diagnostics: &mut Vec<BehaviorDiagnostic>,
) -> Result<StateDomain, String> {
    if let Some(id) = text
        .trim()
        .strip_prefix("id(")
        .and_then(|ty| ty.strip_suffix(')'))
    {
        let (schema, variants) = id
            .split_once(',')
            .ok_or("expected id(schema.Name, value, ...)")?;
        let mut domain = state_domain(name, &format!("enum({variants})"), bound)?;
        domain.id_schema = Some(
            resolve(schema.trim(), SymbolKind::Schema, location, diagnostics)
                .ok_or("could not resolve ID schema")?,
        );
        domain.domain_kind = "id";
        Ok(domain)
    } else {
        state_domain(name, text, bound)
    }
}

fn finite_value(domain: &StateDomain, value: &toml::Value) -> Result<StateValue, String> {
    let value = match (domain.domain_kind, value) {
        ("bool", toml::Value::Boolean(value)) => StateValue::Bool(*value),
        ("bounded_int", toml::Value::Integer(value)) => StateValue::Int((*value).into()),
        ("enum" | "id", toml::Value::String(value)) => StateValue::Enum(value.clone()),
        _ => {
            return Err(format!(
                "'{}' requires a literal of finite type",
                domain.var_name
            ))
        }
    };
    if !domain.contains(&value) {
        return Err(format!(
            "value {value} is outside the domain of '{}'",
            domain.var_name
        ));
    }
    Ok(value)
}

fn table_domain(
    name: &str,
    table: &RawTable,
    schema: &str,
    project: &ProjectContext,
    location: &str,
    resolve: &mut Resolve<'_>,
    diagnostics: &mut Vec<BehaviorDiagnostic>,
) -> Result<(StateDomain, Rows), String> {
    if name.contains('.') || table.max_rows == 0 {
        return Err("table requires a simple name and positive max_rows".into());
    }
    let mut domain = state_domain(name, "bool", None)?;
    domain.domain_kind = "table";
    domain.max_rows = Some(table.max_rows);
    for (field, ty) in &table.fields {
        if field != "id" {
            project.check_field_path(schema, field)?;
        }
        let field_domain = resolved_domain(field, ty, None, location, resolve, diagnostics)?;
        if field_domain.domain_kind == "int" {
            return Err(format!("field '{field}' requires a finite domain"));
        }
        domain.table_fields.insert(field.clone(), field_domain);
    }
    if domain
        .table_fields
        .get("id")
        .and_then(|id| id.id_schema.as_deref())
        != Some(schema)
    {
        return Err("table id must have the table's schema as its ID type".into());
    }
    if table.initial.len() > table.max_rows {
        return Err("initial rows exceed max_rows".into());
    }
    let mut initial = Rows::new();
    for row in &table.initial {
        if !row.keys().eq(domain.table_fields.keys()) {
            return Err("initial row must define exactly the declared fields".into());
        }
        let values: Row = row
            .iter()
            .map(|(field, value)| {
                finite_value(&domain.table_fields[field], value).map(|value| (field.clone(), value))
            })
            .collect::<Result<_, _>>()?;
        let StateValue::Enum(id) = &values["id"] else {
            unreachable!("validated ID")
        };
        if initial.insert(id.clone(), values).is_some() {
            return Err("duplicate initial row ID".into());
        }
    }
    Ok((domain, initial))
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
    let mut table_initial = BTreeMap::new();
    let mut resolver = |reference: &str, kind, loc: &str, diagnostics: &mut Vec<_>| {
        resolve_symbol(reference, kind, project, context, loc, diagnostics)
    };
    for (var, text) in &raw.vars {
        match resolved_domain(
            var,
            text,
            raw.check.bounds.get(var),
            &format!("{location}.vars.{var}"),
            &mut resolver,
            diagnostics,
        ) {
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
    for (name, table) in &raw.tables {
        let loc = format!("{location}.tables.{name}");
        if raw.vars.contains_key(name) {
            diagnostics.push(BehaviorDiagnostic::error(
                "E_TABLE",
                &loc,
                "table name duplicates a state variable",
            ));
            continue;
        }
        let Some(schema) = resolver(&table.schema, SymbolKind::Schema, &loc, diagnostics) else {
            continue;
        };
        let result = table_domain(
            name,
            table,
            &schema,
            project,
            &loc,
            &mut resolver,
            diagnostics,
        );
        match result {
            Ok((domain, initial)) => {
                domains.push(domain);
                table_initial.insert(name.clone(), initial);
            }
            Err(err) => diagnostics.push(BehaviorDiagnostic::error("E_TABLE", loc, err)),
        }
    }
    domains.sort_by(|a, b| a.var_name.cmp(&b.var_name));
    if let Some(observe) = &raw.observe {
        let mut seen = BTreeSet::new();
        for name in observe {
            if !seen.insert(name) || !raw.vars.contains_key(name) {
                diagnostics.push(BehaviorDiagnostic::error(
                    "E_OBSERVE",
                    format!("{location}.observe"),
                    format!("observation must name a distinct scalar state variable: '{name}'"),
                ));
            }
        }
    }

    let config = ExplorerConfig {
        backend: if project.verification.default_backend == "smt" {
            VerificationBackend::Smt
        } else {
            VerificationBackend::Internal
        },
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
                     arguments: &BTreeMap<String, StateDomain>,
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
        let available = |name: &str| {
            if key[0] == "init" {
                return None;
            }
            arguments
                .get(name)
                .or_else(|| domains.iter().find(|domain| domain.var_name == name))
                .map(StateDomain::expr_type)
        };
        match type_expression(ast, &available, &ty) {
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

    let no_arguments = BTreeMap::new();
    let mut init_assignments: Vec<_> = raw
        .init
        .keys()
        .filter_map(|var| {
            check(
                &["init", var],
                domains
                    .iter()
                    .find(|domain| domain.var_name == *var && domain.max_rows.is_none()),
                true,
                &no_arguments,
                diagnostics,
            )
            .map(|value_expr| StateAssignment {
                target_var: var.clone(),
                value_expr,
            })
        })
        .collect();
    init_assignments.extend(table_initial.into_iter().map(|(target_var, rows)| {
        StateAssignment {
            value_expr: TypedExpr {
                ast: ExprAST::Literal {
                    value: StateValue::Table(Arc::new(rows)),
                },
                result_type: domains
                    .iter()
                    .find(|domain| domain.var_name == target_var)
                    .unwrap()
                    .expr_type(),
            },
            target_var,
        }
    }));
    init_assignments.sort_by(|a, b| a.target_var.cmp(&b.target_var));
    let terminal = check(&["terminal"], None, false, &no_arguments, diagnostics);
    let goals = raw
        .reach
        .keys()
        .filter_map(|name| {
            check(
                &["reach", name, "expr"],
                None,
                false,
                &no_arguments,
                diagnostics,
            )
            .map(|expr| (name.clone(), expr))
        })
        .collect();
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
        let operation = action.operation.clone().unwrap_or_else(|| name.clone());
        if action.operation.is_some() && (!identifier(&operation) || operation.contains('.')) {
            diagnostics.push(BehaviorDiagnostic::error(
                "E_OPERATION",
                format!("{location}.action.{name}.operation"),
                "expected an operation identifier",
            ));
        }
        let mut argument_domains = BTreeMap::new();
        let mut args = Row::new();
        for (argument, binding) in &action.args {
            let loc = format!("{location}.action.{name}.args.{argument}");
            let result = resolved_domain(
                argument,
                &binding.domain,
                None,
                &loc,
                &mut resolver,
                diagnostics,
            )
            .and_then(|domain| {
                if argument.contains('.') || domains.iter().any(|var| var.var_name == *argument) {
                    return Err("argument name must be simple and distinct from state names".into());
                }
                let value = finite_value(&domain, &binding.value)?;
                Ok((domain, value))
            });
            match result {
                Ok((domain, value)) => {
                    argument_domains.insert(argument.clone(), domain);
                    args.insert(argument.clone(), value);
                }
                Err(err) => diagnostics.push(BehaviorDiagnostic::error("E_ARGUMENT", loc, err)),
            }
        }
        let guard = check(
            &["action", name, "guard"],
            None,
            false,
            &argument_domains,
            diagnostics,
        );
        let updates = action
            .update
            .keys()
            .filter_map(|var| {
                check(
                    &["action", name, "update", var],
                    domains
                        .iter()
                        .find(|domain| domain.var_name == *var && domain.max_rows.is_none()),
                    true,
                    &argument_domains,
                    diagnostics,
                )
                .map(|value_expr| StateAssignment {
                    target_var: var.clone(),
                    value_expr,
                })
            })
            .collect();
        let mut inserts = BTreeMap::new();
        for (table, rows) in &action.insert {
            let loc = format!("{location}.action.{name}.insert.{table}");
            let Some(domain) = domains
                .iter()
                .find(|domain| domain.var_name == *table && domain.max_rows.is_some())
            else {
                diagnostics.push(BehaviorDiagnostic::error(
                    "E_TABLE_INSERT",
                    loc,
                    "undeclared table",
                ));
                continue;
            };
            let mut checked_rows = Vec::new();
            for (index, row) in rows.iter().enumerate() {
                if !row.keys().eq(domain.table_fields.keys()) {
                    diagnostics.push(BehaviorDiagnostic::error(
                        "E_TABLE_INSERT",
                        &loc,
                        format!("row {index} must define exactly the declared fields"),
                    ));
                    continue;
                }
                checked_rows.push(
                    domain
                        .table_fields
                        .iter()
                        .filter_map(|(field, domain)| {
                            check(
                                &["action", name, "insert", table, &index.to_string(), field],
                                Some(domain),
                                true,
                                &argument_domains,
                                diagnostics,
                            )
                            .map(|expr| (field.clone(), expr))
                        })
                        .collect(),
                );
            }
            inserts.insert(table.clone(), checked_rows);
        }
        actions.push(NormalizedAction {
            name: name.clone(),
            surv_func_refs,
            resolved_func_fqns,
            operation,
            args,
            inserts,
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
        if let Some(expr) = check(
            &["invariant", name, "expr"],
            None,
            false,
            &no_arguments,
            diagnostics,
        ) {
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
        goals,
        observe: raw.observe.clone(),
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
    let expected = match kind {
        SymbolKind::Mod => "mod",
        SymbolKind::Func => "func",
        SymbolKind::Schema => "schema",
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
