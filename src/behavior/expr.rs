use serde::Serialize;

use super::model::StateDomain;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum StateValue {
    Bool(bool),
    Int(i128),
    Enum(String),
}

impl Serialize for StateValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct("StateValue", 2)?;
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
        }
    }
}

impl StateValue {
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
            '+' | '-' | '*' | '(' | ')' | '{' | '}' | ',' => {}
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
        (ExprType::Enum(variants), ExprType::EnumLiterals(values))
        | (ExprType::EnumLiterals(values), ExprType::Enum(variants)) => {
            if let Some(value) = values.iter().find(|value| !variants.contains(value)) {
                return Err(format!(
                    "invalid enum variant '{value}', expected one of {}",
                    variants.join(", ")
                ));
            }
            Ok(ExprType::Enum(variants.clone()))
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
    domains: &[StateDomain],
    expected: &ExprType,
) -> Result<TypedExpr, String> {
    compatible(expected.clone(), infer(&ast, domains, Some(expected))?)?;
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
    Ok(match ast {
        ExprAST::Literal { value } => match value {
            StateValue::Bool(_) => ExprType::Bool,
            StateValue::Int(_) => ExprType::Int,
            StateValue::Enum(value) => ExprType::EnumLiterals(vec![value.clone()]),
        },
        ExprAST::Variable { name } => domains
            .iter()
            .find(|domain| domain.var_name == *name)
            .ok_or_else(|| format!("undeclared variable '{name}'"))?
            .expr_type(),
        ExprAST::Not { arg } => {
            compatible(ExprType::Bool, infer(arg, domains, None)?)?;
            ExprType::Bool
        }
        ExprAST::Neg { arg } => {
            compatible(ExprType::Int, infer(arg, domains, None)?)?;
            ExprType::Int
        }
        ExprAST::Binary { op, left, right } => {
            let left = infer(left, domains, None)?;
            let right = infer(right, domains, None)?;
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
                    if matches!(compatible(left, right)?, ExprType::EnumLiterals(_)) {
                        return Err("enum comparison requires a declared enum domain".into());
                    }
                    ExprType::Bool
                }
            }
        }
        ExprAST::InSet { value, choices } => {
            let mut ty = infer(value, domains, None)?;
            for choice in choices {
                let literal = matches!(choice, ExprAST::Literal { .. })
                    || matches!(choice, ExprAST::Neg { arg } if matches!(arg.as_ref(), ExprAST::Literal { value: StateValue::Int(_) }));
                if !literal {
                    return Err("membership expects a set of literals".into());
                }
                ty = compatible(ty, infer(choice, domains, None)?)?;
            }
            if matches!(ty, ExprType::EnumLiterals(_)) {
                return Err("enum membership requires a declared enum domain".into());
            }
            ExprType::Bool
        }
        ExprAST::If { condition, yes, no } => {
            compatible(ExprType::Bool, infer(condition, domains, None)?)?;
            let yes = infer(yes, domains, expected)?;
            let no = infer(no, domains, expected)?;
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
        lookup: &impl Fn(&str) -> Option<StateValue>,
    ) -> Result<StateValue, String> {
        self.ast.eval(lookup)
    }
}

impl ExprAST {
    fn eval(&self, lookup: &impl Fn(&str) -> Option<StateValue>) -> Result<StateValue, String> {
        Ok(match self {
            Self::Literal { value } => value.clone(),
            Self::Variable { name } => {
                lookup(name).ok_or_else(|| format!("no value for '{name}'"))?
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
