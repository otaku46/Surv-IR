use serde::Serialize;
use std::collections::BTreeMap;

use super::model::StateDomain;

pub type Row = BTreeMap<String, StateValue>;
pub type Rows = BTreeMap<String, Row>;
pub type ReplayValues = BTreeMap<String, ReplayValue>;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ReplayValue {
    Boolean(bool),
    Integer(i128),
    Symbol(String),
    Object(ReplayValues),
    Null,
}

pub(super) fn replay_values(row: &Row) -> ReplayValues {
    row.iter()
        .map(|(name, value)| (name.clone(), value.replay_value()))
        .collect()
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum StateValue {
    Bool(bool),
    Int(i128),
    Enum(String),
    None,
    Row(Row),
    Table(std::sync::Arc<Rows>),
}

impl Serialize for StateValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer
            .serialize_struct("StateValue", if matches!(self, Self::None) { 1 } else { 2 })?;
        match self {
            Self::Bool(value) => {
                record.serialize_field("value_kind", "bool")?;
                record.serialize_field("bool_val", value)?;
            }
            Self::Int(value) => {
                record.serialize_field("value_kind", "int")?;
                record.serialize_field("int_val", value)?;
            }
            Self::Enum(value) => {
                record.serialize_field("value_kind", "enum")?;
                record.serialize_field("enum_val", value)?;
            }
            Self::None => record.serialize_field("value_kind", "none")?,
            Self::Row(value) => {
                record.serialize_field("value_kind", "row")?;
                record.serialize_field("fields", value)?;
            }
            Self::Table(value) => {
                record.serialize_field("value_kind", "table")?;
                record.serialize_field("rows", value.as_ref())?;
            }
        }
        record.end()
    }
}

impl std::fmt::Display for StateValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Enum(value) => write!(f, "'{value}'"),
            Self::None => write!(f, "None"),
            Self::Row(fields) => write!(f, "{fields:?}"),
            Self::Table(rows) => write!(f, "{rows:?}"),
        }
    }
}

impl StateValue {
    pub(super) fn replay_value(&self) -> ReplayValue {
        match self {
            Self::Bool(value) => ReplayValue::Boolean(*value),
            Self::Int(value) => ReplayValue::Integer(*value),
            Self::Enum(value) => ReplayValue::Symbol(value.clone()),
            Self::None => ReplayValue::Null,
            Self::Row(fields) => ReplayValue::Object(replay_values(fields)),
            Self::Table(rows) => ReplayValue::Object(
                rows.iter()
                    .map(|(id, row)| (id.clone(), ReplayValue::Object(replay_values(row))))
                    .collect(),
            ),
        }
    }

    pub(super) fn boolean(self) -> Result<bool, String> {
        match self {
            Self::Bool(value) => Ok(value),
            _ => Err("expected a boolean value".into()),
        }
    }

    fn integer(self) -> Result<i128, String> {
        match self {
            Self::Int(value) => Ok(value),
            _ => Err("expected an integer value".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Add,
    Sub,
    Mul,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Implies,
    In,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "node_kind", rename_all = "snake_case")]
pub enum ExprAST {
    Literal {
        value: StateValue,
    },
    Variable {
        name: String,
    },
    Lookup {
        table: String,
        key: Box<Self>,
    },
    Field {
        record: Box<Self>,
        name: String,
    },
    Quantifier {
        universal: bool,
        binder: String,
        table: String,
        body: Box<Self>,
    },
    Not {
        arg: Box<Self>,
    },
    Neg {
        arg: Box<Self>,
    },
    Binary {
        op: Op,
        left: Box<Self>,
        right: Box<Self>,
    },
    InSet {
        value: Box<Self>,
        choices: Vec<Self>,
    },
    If {
        condition: Box<Self>,
        yes: Box<Self>,
        no: Box<Self>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExprType {
    Bool,
    Int,
    Enum(Vec<String>),
    EnumLiterals(Vec<String>),
    Id(String, Vec<String>),
    None,
    Row(BTreeMap<String, ExprType>),
    Table(BTreeMap<String, ExprType>),
}

#[derive(Clone, Debug, Serialize)]
pub struct TypedExpr {
    pub ast: ExprAST,
    pub result_type: ExprType,
}

pub(super) fn parse_expression(source: &str) -> Result<ExprAST, String> {
    let tokens = lex(source)?;
    let mut parser = Parser { tokens, cursor: 0 };
    let expr = parser.expression(0, 0)?;
    if let Some(token) = parser.peek() {
        return Err(format!("unexpected token {token:?}"));
    }
    Ok(expr)
}

fn lex(source: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        let mut token = String::from(ch);
        match ch {
            '\'' => loop {
                let ch = chars.next().ok_or("unterminated enum literal")?;
                token.push(ch);
                if ch == '\'' {
                    break;
                }
            },
            '=' | '!' | '<' | '>' => {
                if chars
                    .peek()
                    .is_some_and(|next| *next == '=' || (ch == '=' && *next == '>'))
                {
                    token.push(chars.next().unwrap());
                }
            }
            '+' | '-' | '*' | '(' | ')' | '{' | '}' | ',' | '[' | ']' | ':' | '.' => {}
            c if c.is_ascii_digit() => {
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    token.push(chars.next().unwrap());
                }
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                while chars
                    .peek()
                    .is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, '_' | '.'))
                {
                    token.push(chars.next().unwrap());
                }
            }
            _ => return Err(format!("unexpected character {ch:?}")),
        }
        tokens.push(token);
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<String>,
    cursor: usize,
}

impl Parser {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.cursor).map(String::as_str)
    }

    fn take(&mut self) -> Option<String> {
        let token = self.tokens.get(self.cursor)?.clone();
        self.cursor += 1;
        Some(token)
    }

    fn expect(&mut self, expected: &str) -> Result<(), String> {
        if self.peek() != Some(expected) {
            return Err(format!("expected {expected:?}, found {:?}", self.peek()));
        }
        self.cursor += 1;
        Ok(())
    }

    fn expression(&mut self, min: u8, depth: usize) -> Result<ExprAST, String> {
        // The typed tree is subsequently traversed recursively, including left-associated chains.
        if depth > 128 {
            return Err("expression nesting exceeds 128 levels".into());
        }
        let token = self.take().ok_or("expected an expression")?;
        let mut left = match token.as_str() {
            "(" => {
                let inner = self.expression(0, depth + 1)?;
                self.expect(")")?;
                inner
            }
            "forall" | "exists" if self.tokens.get(self.cursor + 1).is_some_and(|t| t == "in") => {
                let binder = self
                    .take()
                    .filter(|name| identifier(name) && !name.contains('.'))
                    .ok_or("expected a quantifier binder")?;
                self.expect("in")?;
                let table = self
                    .take()
                    .and_then(|name| name.strip_suffix(".values").map(str::to_owned))
                    .filter(|name| identifier(name))
                    .ok_or("expected <table>.values")?;
                self.expect(":")?;
                ExprAST::Quantifier {
                    universal: token == "forall",
                    binder,
                    table,
                    body: Box::new(self.expression(0, depth + 1)?),
                }
            }
            "not" => ExprAST::Not {
                arg: Box::new(self.expression(4, depth + 1)?),
            },
            "-" => ExprAST::Neg {
                arg: Box::new(self.expression(7, depth + 1)?),
            },
            "if" => {
                let condition = self.expression(0, depth + 1)?;
                self.expect("then")?;
                let yes = self.expression(0, depth + 1)?;
                self.expect("else")?;
                let no = self.expression(0, depth + 1)?;
                ExprAST::If {
                    condition: Box::new(condition),
                    yes: Box::new(yes),
                    no: Box::new(no),
                }
            }
            "true" | "false" => ExprAST::Literal {
                value: StateValue::Bool(token == "true"),
            },
            t if t.starts_with('\'') => ExprAST::Literal {
                value: StateValue::Enum(t[1..t.len() - 1].into()),
            },
            t if t.as_bytes()[0].is_ascii_digit() => ExprAST::Literal {
                value: StateValue::Int(
                    t.parse()
                        .map_err(|_| format!("integer literal out of range: {t}"))?,
                ),
            },
            t if identifier(t)
                && !matches!(t, "then" | "else" | "in" | "and" | "or" | "implies") =>
            {
                ExprAST::Variable { name: token }
            }
            _ => return Err(format!("expected an expression, found {token:?}")),
        };
        let mut chain_length = 0;
        while let Some(token) = self.peek() {
            if !matches!(token, "[" | ".") {
                break;
            }
            chain_length += 1;
            if depth + chain_length > 128 {
                return Err("expression access chain exceeds 128 levels".into());
            }
            if token == "[" {
                let ExprAST::Variable { name: table } = left else {
                    return Err("lookup requires a declared table name".into());
                };
                self.take();
                let key = self.expression(0, depth + 1)?;
                self.expect("]")?;
                left = ExprAST::Lookup {
                    table,
                    key: Box::new(key),
                };
            } else {
                self.take();
                let name = self
                    .take()
                    .filter(|name| identifier(name))
                    .ok_or("expected a field path")?;
                left = ExprAST::Field {
                    record: Box::new(left),
                    name,
                };
            }
        }
        while let Some((op, prec)) = self.peek().and_then(operator) {
            if prec < min {
                break;
            }
            chain_length += 1;
            if depth + chain_length > 128 {
                return Err("expression operator chain exceeds 128 levels".into());
            }
            self.cursor += 1;
            if matches!(op, Op::In) {
                self.expect("{")?;
                let mut choices = Vec::new();
                if self.peek() != Some("}") {
                    loop {
                        choices.push(self.expression(0, depth + 1)?);
                        if self.peek() != Some(",") {
                            break;
                        }
                        self.cursor += 1;
                    }
                }
                self.expect("}")?;
                left = ExprAST::InSet {
                    value: Box::new(left),
                    choices,
                };
            } else {
                let right =
                    self.expression(prec + u8::from(!matches!(op, Op::Implies)), depth + 1)?;
                left = ExprAST::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
        }
        Ok(left)
    }
}

pub(super) fn identifier(name: &str) -> bool {
    name.split('.').all(|part| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

fn operator(token: &str) -> Option<(Op, u8)> {
    Some(match token {
        "=>" | "implies" => (Op::Implies, 1),
        "or" => (Op::Or, 2),
        "and" => (Op::And, 3),
        "==" => (Op::Eq, 4),
        "!=" => (Op::Ne, 4),
        "<" => (Op::Lt, 4),
        "<=" => (Op::Le, 4),
        ">" => (Op::Gt, 4),
        ">=" => (Op::Ge, 4),
        "in" => (Op::In, 4),
        "+" => (Op::Add, 5),
        "-" => (Op::Sub, 5),
        "*" => (Op::Mul, 6),
        _ => return None,
    })
}

fn compatible(a: ExprType, b: ExprType) -> Result<ExprType, String> {
    match (&a, &b) {
        _ if a == b => Ok(a),
        (ExprType::Id(left, left_values), ExprType::Id(right, right_values)) if left == right => {
            let mut values = left_values.clone();
            values.extend(right_values.iter().cloned());
            values.sort();
            values.dedup();
            Ok(ExprType::Id(left.clone(), values))
        }
        (ExprType::Enum(variants) | ExprType::Id(_, variants), ExprType::EnumLiterals(values))
        | (ExprType::EnumLiterals(values), ExprType::Enum(variants) | ExprType::Id(_, variants)) => {
            if let Some(value) = values.iter().find(|value| !variants.contains(value)) {
                return Err(format!(
                    "invalid enum variant '{value}', expected one of {}",
                    variants.join(", ")
                ));
            }
            if matches!(a, ExprType::EnumLiterals(_)) {
                Ok(b)
            } else {
                Ok(a)
            }
        }
        (ExprType::EnumLiterals(left), ExprType::EnumLiterals(right)) => {
            let mut values = left.clone();
            values.extend(right.iter().cloned());
            values.sort();
            values.dedup();
            Ok(ExprType::EnumLiterals(values))
        }
        _ => Err(format!("incompatible types {a:?} and {b:?}")),
    }
}

pub(super) fn type_expression(
    ast: ExprAST,
    lookup: &dyn Fn(&str) -> Option<ExprType>,
    expected: &ExprType,
) -> Result<TypedExpr, String> {
    compatible(expected.clone(), infer_type(&ast, lookup, Some(expected))?)?;
    Ok(TypedExpr {
        ast,
        result_type: expected.clone(),
    })
}

pub(super) fn infer(
    ast: &ExprAST,
    domains: &[StateDomain],
    expected: Option<&ExprType>,
) -> Result<ExprType, String> {
    infer_type(
        ast,
        &|name| {
            domains
                .iter()
                .find(|domain| domain.var_name == name)
                .map(StateDomain::expr_type)
        },
        expected,
    )
}

fn field_type(record: ExprType, name: &str) -> Result<ExprType, String> {
    if let ExprType::Row(fields) = record {
        return fields
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown record field '{name}'"));
    }
    Err(format!("field access '{name}' requires a record"))
}

fn infer_type(
    ast: &ExprAST,
    lookup: &dyn Fn(&str) -> Option<ExprType>,
    expected: Option<&ExprType>,
) -> Result<ExprType, String> {
    Ok(match ast {
        ExprAST::Literal { value } => match value {
            StateValue::Bool(_) => ExprType::Bool,
            StateValue::Int(_) => ExprType::Int,
            StateValue::Enum(value) => ExprType::EnumLiterals(vec![value.clone()]),
            StateValue::None => ExprType::None,
            _ => return Err("record and table literals require a table declaration".into()),
        },
        ExprAST::Variable { name } => {
            if let Some((binder, field)) = name.split_once('.') {
                if let Some(record @ ExprType::Row(_)) = lookup(binder) {
                    return field_type(record, field);
                }
            }
            match lookup(name) {
                Some(ty) => ty,
                None if name == "None" => ExprType::None,
                None => return Err(format!("undeclared variable '{name}'")),
            }
        }
        ExprAST::Lookup { table, key } => {
            let Some(ExprType::Table(fields)) = lookup(table) else {
                return Err(format!("undeclared table '{table}'"));
            };
            compatible(fields["id"].clone(), infer_type(key, lookup, None)?)?;
            ExprType::Row(fields)
        }
        ExprAST::Field { record, name } => field_type(infer_type(record, lookup, None)?, name)?,
        ExprAST::Quantifier {
            binder,
            table,
            universal: _,
            body,
        } => {
            let Some(ExprType::Table(fields)) = lookup(table) else {
                return Err(format!("undeclared table '{table}'"));
            };
            if lookup(binder).is_some()
                || matches!(
                    binder.as_str(),
                    "if" | "then"
                        | "else"
                        | "or"
                        | "and"
                        | "not"
                        | "implies"
                        | "in"
                        | "true"
                        | "false"
                        | "None"
                )
            {
                return Err(format!(
                    "invalid or already bound quantifier variable '{binder}'"
                ));
            }
            let row = ExprType::Row(fields);
            let scope = |name: &str| {
                if name == binder {
                    Some(row.clone())
                } else {
                    lookup(name)
                }
            };
            compatible(ExprType::Bool, infer_type(body, &scope, None)?)?;
            ExprType::Bool
        }
        ExprAST::Not { arg } => {
            compatible(ExprType::Bool, infer_type(arg, lookup, None)?)?;
            ExprType::Bool
        }
        ExprAST::Neg { arg } => {
            compatible(ExprType::Int, infer_type(arg, lookup, None)?)?;
            ExprType::Int
        }
        ExprAST::Binary { op, left, right } => {
            let left = infer_type(left, lookup, None)?;
            let right = infer_type(right, lookup, None)?;
            match op {
                Op::Add | Op::Sub | Op::Mul => {
                    compatible(ExprType::Int, left)?;
                    compatible(ExprType::Int, right)?;
                    ExprType::Int
                }
                Op::And | Op::Or | Op::Implies => {
                    compatible(ExprType::Bool, left)?;
                    compatible(ExprType::Bool, right)?;
                    ExprType::Bool
                }
                _ => {
                    let equality = matches!(op, Op::Eq | Op::Ne);
                    if equality
                        && matches!(
                            (&left, &right),
                            (ExprType::Row(_), ExprType::None) | (ExprType::None, ExprType::Row(_))
                        )
                    {
                        return Ok(ExprType::Bool);
                    }
                    let common = compatible(left, right)?;
                    if matches!(
                        common,
                        ExprType::Table(_) | ExprType::Row(_) | ExprType::None
                    ) || (!equality && matches!(common, ExprType::Id(..)))
                    {
                        return Err("unsupported comparison for this operand type".into());
                    }
                    if matches!(common, ExprType::EnumLiterals(_)) {
                        return Err("enum comparison requires a declared enum domain".into());
                    }
                    ExprType::Bool
                }
            }
        }
        ExprAST::InSet { value, choices } => {
            let mut ty = infer_type(value, lookup, None)?;
            for choice in choices {
                let literal = matches!(choice, ExprAST::Literal { .. })
                    || matches!(choice, ExprAST::Neg { arg } if matches!(arg.as_ref(), ExprAST::Literal { value: StateValue::Int(_) }));
                if !literal {
                    return Err("membership expects a set of literals".into());
                }
                ty = compatible(ty, infer_type(choice, lookup, None)?)?;
            }
            match ty {
                ExprType::Bool | ExprType::Int | ExprType::Enum(_) | ExprType::Id(..) => {
                    ExprType::Bool
                }
                ExprType::EnumLiterals(_) => {
                    return Err("enum membership requires a declared enum domain".into())
                }
                _ => return Err("membership requires a scalar value".into()),
            }
        }
        ExprAST::If { condition, yes, no } => {
            compatible(ExprType::Bool, infer_type(condition, lookup, None)?)?;
            let yes = infer_type(yes, lookup, expected)?;
            let no = infer_type(no, lookup, expected)?;
            if let Some(expected) = expected {
                compatible(expected.clone(), yes)?;
                compatible(expected.clone(), no)?
            } else {
                compatible(yes, no)?
            }
        }
    })
}

impl TypedExpr {
    pub(super) fn eval(
        &self,
        lookup: &dyn Fn(&str) -> Option<StateValue>,
    ) -> Result<StateValue, String> {
        self.ast.eval(lookup)
    }
}

fn field_value(record: StateValue, name: &str) -> Result<StateValue, String> {
    match record {
        StateValue::Row(mut fields) => fields
            .remove(name)
            .ok_or_else(|| format!("unknown record field '{name}'")),
        StateValue::None => Err(format!(
            "field '{name}' accessed on a missing row; check lookup against None first"
        )),
        _ => Err(format!("field access '{name}' requires a record")),
    }
}

impl ExprAST {
    fn eval(&self, lookup: &dyn Fn(&str) -> Option<StateValue>) -> Result<StateValue, String> {
        Ok(match self {
            Self::Literal { value } => value.clone(),
            Self::Variable { name } => {
                if let Some((binder, field)) = name.split_once('.') {
                    if let Some(record @ StateValue::Row(_)) = lookup(binder) {
                        return field_value(record, field);
                    }
                }
                match lookup(name) {
                    Some(value) => value,
                    None if name == "None" => StateValue::None,
                    None => return Err(format!("no value for '{name}'")),
                }
            }
            Self::Lookup { table, key } => {
                let Some(StateValue::Table(rows)) = lookup(table) else {
                    return Err(format!("no table '{table}'"));
                };
                let StateValue::Enum(id) = key.eval(lookup)? else {
                    return Err("expected a typed ID".into());
                };
                rows.get(&id)
                    .cloned()
                    .map_or(StateValue::None, StateValue::Row)
            }
            Self::Field { record, name } => field_value(record.eval(lookup)?, name)?,
            Self::Quantifier {
                universal,
                binder,
                table,
                body,
            } => {
                let Some(StateValue::Table(rows)) = lookup(table) else {
                    return Err(format!("no table '{table}'"));
                };
                for row in rows.values() {
                    let value = StateValue::Row(row.clone());
                    let scope = |name: &str| {
                        if name == binder {
                            Some(value.clone())
                        } else {
                            lookup(name)
                        }
                    };
                    if body.eval(&scope)?.boolean()? != *universal {
                        return Ok(StateValue::Bool(!universal));
                    }
                }
                StateValue::Bool(*universal)
            }
            Self::Not { arg } => StateValue::Bool(!arg.eval(lookup)?.boolean()?),
            Self::Neg { arg } => StateValue::Int(
                arg.eval(lookup)?
                    .integer()?
                    .checked_neg()
                    .ok_or("integer arithmetic exceeds engine precision")?,
            ),
            Self::If { condition, yes, no } => {
                if condition.eval(lookup)?.boolean()? {
                    yes.eval(lookup)?
                } else {
                    no.eval(lookup)?
                }
            }
            Self::InSet { value, choices } => {
                let value = value.eval(lookup)?;
                for choice in choices {
                    if value == choice.eval(lookup)? {
                        return Ok(StateValue::Bool(true));
                    }
                }
                StateValue::Bool(false)
            }
            Self::Binary { op, left, right } => {
                let left = left.eval(lookup)?;
                match op {
                    Op::And => StateValue::Bool(left.boolean()? && right.eval(lookup)?.boolean()?),
                    Op::Or => StateValue::Bool(left.boolean()? || right.eval(lookup)?.boolean()?),
                    Op::Implies => {
                        StateValue::Bool(!left.boolean()? || right.eval(lookup)?.boolean()?)
                    }
                    Op::Add | Op::Sub | Op::Mul => {
                        let a = left.integer()?;
                        let b = right.eval(lookup)?.integer()?;
                        let value = match op {
                            Op::Add => a.checked_add(b),
                            Op::Sub => a.checked_sub(b),
                            _ => a.checked_mul(b),
                        }
                        .ok_or("integer arithmetic exceeds engine precision")?;
                        StateValue::Int(value)
                    }
                    Op::Eq => StateValue::Bool(left == right.eval(lookup)?),
                    Op::Ne => StateValue::Bool(left != right.eval(lookup)?),
                    Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                        let order = match (left, right.eval(lookup)?) {
                            (StateValue::Int(a), StateValue::Int(b)) => a.cmp(&b),
                            (StateValue::Bool(a), StateValue::Bool(b)) => a.cmp(&b),
                            (StateValue::Enum(a), StateValue::Enum(b)) => a.cmp(&b),
                            _ => return Err("comparison has incompatible operand types".into()),
                        };
                        StateValue::Bool(match op {
                            Op::Lt => order.is_lt(),
                            Op::Le => order.is_le(),
                            Op::Gt => order.is_gt(),
                            _ => order.is_ge(),
                        })
                    }
                    Op::In => return Err("membership must use a set".into()),
                }
            }
        })
    }
}
