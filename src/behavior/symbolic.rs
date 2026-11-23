use super::expr::{ExprAST, Op, StateValue};
use super::model::{NormalizedAction, NormalizedBehaviorModel, StateDomain};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(super) type Term = usize;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Sort {
    Bool,
    Int,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Operator {
    Not,
    And,
    Or,
    Eq,
    Lt,
    Le,
    Add,
    Sub,
    Mul,
    Ite,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum Node {
    Bool(bool),
    Int(i128),
    Column(usize),
    Apply(Operator, Vec<Term>),
}

pub(super) struct Terms {
    pub nodes: Vec<(Node, Sort)>,
    ids: HashMap<Node, Term>,
}

impl Terms {
    fn new() -> Self {
        let mut terms = Self {
            nodes: Vec::new(),
            ids: HashMap::new(),
        };
        terms.intern(Node::Bool(false), Sort::Bool);
        terms.intern(Node::Bool(true), Sort::Bool);
        terms
    }

    fn intern(&mut self, node: Node, sort: Sort) -> Term {
        *self.ids.entry(node.clone()).or_insert_with(|| {
            let id = self.nodes.len();
            self.nodes.push((node, sort));
            id
        })
    }

    pub const FALSE: Term = 0;
    pub const TRUE: Term = 1;

    fn int(&mut self, value: i128) -> Term {
        self.intern(Node::Int(value), Sort::Int)
    }

    fn default(&mut self, term: Term) -> Term {
        match self.nodes[term].1 {
            Sort::Bool => Self::FALSE,
            Sort::Int => self.int(0),
        }
    }

    fn op(&mut self, op: Operator, args: Vec<Term>, sort: Sort) -> Term {
        self.intern(Node::Apply(op, args), sort)
    }

    pub fn not(&mut self, term: Term) -> Term {
        match &self.nodes[term].0 {
            Node::Bool(value) => usize::from(!value),
            Node::Apply(Operator::Not, args) => args[0],
            _ => self.op(Operator::Not, vec![term], Sort::Bool),
        }
    }

    pub fn and(&mut self, args: impl IntoIterator<Item = Term>) -> Term {
        self.connective(Operator::And, args, Self::TRUE, Self::FALSE)
    }

    pub fn or(&mut self, args: impl IntoIterator<Item = Term>) -> Term {
        self.connective(Operator::Or, args, Self::FALSE, Self::TRUE)
    }

    fn connective(
        &mut self,
        op: Operator,
        args: impl IntoIterator<Item = Term>,
        identity: Term,
        absorbing: Term,
    ) -> Term {
        let mut operands = Vec::new();
        for term in args {
            if term == absorbing {
                return absorbing;
            }
            if term != identity {
                operands.push(term);
            }
        }
        match operands.as_slice() {
            [] => identity,
            [term] => *term,
            _ => self.op(op, operands, Sort::Bool),
        }
    }

    fn eq(&mut self, a: Term, b: Term) -> Term {
        if a == b {
            return Self::TRUE;
        }
        match (&self.nodes[a].0, &self.nodes[b].0) {
            (Node::Int(_), Node::Int(_)) | (Node::Bool(_), Node::Bool(_)) => Self::FALSE,
            _ => self.op(Operator::Eq, vec![a, b], Sort::Bool),
        }
    }

    fn compare(&mut self, op: Operator, a: Term, b: Term) -> Term {
        if let (Node::Int(a), Node::Int(b)) = (&self.nodes[a].0, &self.nodes[b].0) {
            return usize::from(if op == Operator::Lt { a < b } else { a <= b });
        }
        if self.nodes[a].1 == Sort::Bool {
            let not_a = self.not(a);
            return if op == Operator::Lt {
                self.and([not_a, b])
            } else {
                self.or([not_a, b])
            };
        }
        self.op(op, vec![a, b], Sort::Bool)
    }

    fn ite(&mut self, condition: Term, yes: Term, no: Term) -> Term {
        if condition == Self::TRUE || yes == no {
            yes
        } else if condition == Self::FALSE {
            no
        } else if yes == Self::TRUE && no == Self::FALSE {
            condition
        } else {
            self.op(Operator::Ite, vec![condition, yes, no], self.nodes[yes].1)
        }
    }

    fn bounds(&mut self, value: Term, min: i128, max: i128) -> Term {
        let min = self.int(min);
        let max = self.int(max);
        let lower = self.compare(Operator::Le, min, value);
        let upper = self.compare(Operator::Le, value, max);
        self.and([lower, upper])
    }
}

#[derive(Clone, Copy)]
pub(super) struct Checked {
    pub value: Term,
    pub defined: Term,
}

impl Checked {
    const TRUE: Self = Self::total(Terms::TRUE);
    const FALSE: Self = Self::total(Terms::FALSE);

    const fn total(value: Term) -> Self {
        Self {
            value,
            defined: Terms::TRUE,
        }
    }

    pub fn succeeds(self, terms: &mut Terms) -> Term {
        terms.and([self.defined, self.value])
    }
}

#[derive(Clone)]
struct Record {
    present: Term,
    fields: BTreeMap<String, Term>,
}

#[derive(Clone)]
struct Slot {
    id: Term,
    record: Record,
    presence_column: usize,
    columns: BTreeMap<String, usize>,
}

#[derive(Clone)]
enum Value {
    Scalar(Term),
    Row(Record),
    None,
}

struct Evaluated {
    value: Value,
    defined: Term,
}

impl Evaluated {
    fn scalar(self) -> Checked {
        let Value::Scalar(value) = self.value else {
            unreachable!("expected a checked scalar expression");
        };
        Checked {
            value,
            defined: self.defined,
        }
    }

    fn from_scalar(scalar: Checked) -> Self {
        Self {
            value: Value::Scalar(scalar.value),
            defined: scalar.defined,
        }
    }
}

pub(super) enum ColumnOrigin {
    Scalar(usize),
    Presence(usize, String),
    Field(usize, String, String),
}

impl ColumnOrigin {
    pub fn project(&self, state: &[StateValue]) -> Option<StateValue> {
        fn row<'a>(
            state: &'a [StateValue],
            domain: usize,
            key: &str,
        ) -> Option<&'a super::expr::Row> {
            let StateValue::Table(rows) = &state[domain] else {
                unreachable!("table column refers to a table domain");
            };
            rows.get(key)
        }
        match self {
            Self::Scalar(domain) => Some(state[*domain].clone()),
            Self::Presence(domain, key) => {
                Some(StateValue::Bool(row(state, *domain, key).is_some()))
            }
            Self::Field(domain, key, field) => {
                row(state, *domain, key).map(|row| row[field].clone())
            }
        }
    }
}

pub(super) struct Column {
    pub origin: ColumnOrigin,
    pub label: String,
    pub sort: Sort,
    pub initial: Term,
}

pub(super) struct Action {
    pub enabled: Term,
    pub error: Term,
    pub rejected: Term,
    pub success: Term,
    pub next: Vec<Term>,
}

pub(super) struct SymbolicBehavior {
    pub symbols: BTreeMap<String, i128>,
    pub terms: Terms,
    pub columns: Vec<Column>,
    pub terminal: Checked,
    pub invariants: Vec<Checked>,
    pub goals: BTreeMap<String, Checked>,
    pub actions: Vec<Action>,
}

struct Lowering {
    terms: Terms,
    symbols: BTreeMap<String, i128>,
    columns: Vec<Column>,
    scalars: BTreeMap<String, usize>,
    tables: BTreeMap<String, Vec<Slot>>,
}

pub(super) fn lower(model: &NormalizedBehaviorModel) -> Result<SymbolicBehavior, String> {
    let mut symbols = BTreeSet::new();
    for domain in &model.domains {
        symbols.extend(domain.variants.iter().cloned());
        for field in domain.table_fields.values() {
            symbols.extend(field.variants.iter().cloned());
        }
    }
    // Argument-only enum domains are erased by normalization. The fixed
    // values suffice because argument expressions compare against typed literals.
    for action in &model.actions {
        for value in action.args.values() {
            collect_value_symbols(value, &mut symbols);
        }
    }
    for expr in model.expressions() {
        collect_expr_symbols(&expr.ast, &mut symbols);
    }
    let mut lower = Lowering {
        terms: Terms::new(),
        symbols: symbols
            .into_iter()
            .enumerate()
            .map(|(index, symbol)| (symbol, index as i128))
            .collect(),
        columns: Vec::new(),
        scalars: BTreeMap::new(),
        tables: BTreeMap::new(),
    };
    for (index, (domain, assignment)) in model
        .domains
        .iter()
        .zip(&model.init_assignments)
        .enumerate()
    {
        let initial = assignment.value_expr.eval(&|_| None)?;
        match initial {
            StateValue::Table(rows) => {
                let mut slots = Vec::new();
                for key in &domain.table_fields["id"].variants {
                    let row = rows.get(key);
                    let (presence_column, present) = lower.column(
                        format!("{}[{key}].present", domain.var_name),
                        usize::from(row.is_some()),
                        ColumnOrigin::Presence(index, key.clone()),
                    );
                    let id = lower.symbol(key);
                    let mut fields = BTreeMap::from([("id".into(), id)]);
                    let mut columns = BTreeMap::new();
                    for (field, field_domain) in &domain.table_fields {
                        if field == "id" {
                            continue;
                        }
                        let value = row
                            .map_or_else(|| field_default(field_domain), |row| row[field].clone());
                        let initial = lower.literal(&value);
                        let (column, value) = lower.column(
                            format!("{}[{key}].{field}", domain.var_name),
                            initial,
                            ColumnOrigin::Field(index, key.clone(), field.clone()),
                        );
                        fields.insert(field.clone(), value);
                        columns.insert(field.clone(), column);
                    }
                    slots.push(Slot {
                        id,
                        record: Record { present, fields },
                        presence_column,
                        columns,
                    });
                }
                lower.tables.insert(domain.var_name.clone(), slots);
            }
            initial => {
                let initial = lower.literal(&initial);
                let (column, _) = lower.column(
                    domain.var_name.clone(),
                    initial,
                    ColumnOrigin::Scalar(index),
                );
                lower.scalars.insert(domain.var_name.clone(), column);
            }
        }
    }
    let terminal = model
        .terminal
        .as_ref()
        .map_or(Checked::FALSE, |expr| lower.predicate(&expr.ast));
    let invariants = model
        .invariants
        .iter()
        .map(|item| lower.predicate(&item.expr.ast))
        .collect();
    let goals = model
        .goals
        .iter()
        .map(|(name, expr)| (name.clone(), lower.predicate(&expr.ast)))
        .collect();
    let actions = model
        .actions
        .iter()
        .map(|action| lower.action(model, action))
        .collect();
    Ok(SymbolicBehavior {
        symbols: lower.symbols,
        terms: lower.terms,
        columns: lower.columns,
        terminal,
        invariants,
        goals,
        actions,
    })
}

fn field_default(domain: &StateDomain) -> StateValue {
    match domain.domain_kind {
        "bool" => StateValue::Bool(false),
        "enum" | "id" => StateValue::Enum(domain.variants[0].clone()),
        _ => StateValue::Int(domain.min_val.unwrap_or(0)),
    }
}

fn collect_value_symbols(value: &StateValue, symbols: &mut BTreeSet<String>) {
    match value {
        StateValue::Enum(value) => {
            symbols.insert(value.clone());
        }
        StateValue::Table(rows) => {
            for value in rows.values().flat_map(|row| row.values()) {
                collect_value_symbols(value, symbols);
            }
        }
        _ => {}
    }
}

fn collect_expr_symbols(expr: &ExprAST, symbols: &mut BTreeSet<String>) {
    match expr {
        ExprAST::Literal { value } => collect_value_symbols(value, symbols),
        ExprAST::Variable { .. } => {}
        ExprAST::Lookup { key, .. }
        | ExprAST::Field { record: key, .. }
        | ExprAST::Quantifier { body: key, .. }
        | ExprAST::Not { arg: key }
        | ExprAST::Neg { arg: key } => collect_expr_symbols(key, symbols),
        ExprAST::Binary { left, right, .. } => {
            collect_expr_symbols(left, symbols);
            collect_expr_symbols(right, symbols);
        }
        ExprAST::InSet { value, choices } => {
            collect_expr_symbols(value, symbols);
            for expr in choices {
                collect_expr_symbols(expr, symbols);
            }
        }
        ExprAST::If { condition, yes, no } => {
            for expr in [condition, yes, no] {
                collect_expr_symbols(expr, symbols);
            }
        }
    }
}

impl Lowering {
    fn literal(&mut self, value: &StateValue) -> Term {
        match value {
            StateValue::Bool(value) => usize::from(*value),
            StateValue::Int(value) => self.terms.int(*value),
            StateValue::Enum(value) => self.symbol(value),
            _ => unreachable!("expected a scalar literal"),
        }
    }

    fn symbol(&mut self, value: &str) -> Term {
        self.terms.int(self.symbols[value])
    }

    fn column(&mut self, label: String, initial: Term, origin: ColumnOrigin) -> (usize, Term) {
        let column = self.columns.len();
        let sort = self.terms.nodes[initial].1;
        self.columns.push(Column {
            origin,
            label,
            sort,
            initial,
        });
        (column, self.terms.intern(Node::Column(column), sort))
    }

    fn column_value(&mut self, column: usize) -> Term {
        self.terms
            .intern(Node::Column(column), self.columns[column].sort)
    }

    fn predicate(&mut self, ast: &ExprAST) -> Checked {
        self.expression(ast, &BTreeMap::new()).scalar()
    }

    fn expression(&mut self, ast: &ExprAST, scope: &BTreeMap<String, Value>) -> Evaluated {
        let scalar = match ast {
            ExprAST::Literal { value } => Checked::total(self.literal(value)),
            ExprAST::Variable { name } => {
                if let Some((binder, field)) = name.split_once('.') {
                    if let Some(Value::Row(record)) = scope.get(binder) {
                        return Evaluated::from_scalar(Checked::total(record.fields[field]));
                    }
                }
                if let Some(value) = scope.get(name) {
                    return Evaluated {
                        value: value.clone(),
                        defined: Terms::TRUE,
                    };
                }
                if let Some(column) = self.scalars.get(name) {
                    Checked::total(self.column_value(*column))
                } else {
                    return Evaluated {
                        value: Value::None,
                        defined: Terms::TRUE,
                    };
                }
            }
            ExprAST::Lookup { table, key } => {
                let key = self.expression(key, scope).scalar();
                let slots = &self.tables[table];
                let mut fields: BTreeMap<_, _> = slots[0]
                    .record
                    .fields
                    .iter()
                    .map(|(name, term)| (name.clone(), self.terms.default(*term)))
                    .collect();
                let mut presence = Vec::new();
                for slot in slots {
                    let matches = self.terms.eq(key.value, slot.id);
                    let present = self.terms.and([matches, slot.record.present]);
                    presence.push(present);
                    for (name, term) in &mut fields {
                        *term = self.terms.ite(present, slot.record.fields[name], *term);
                    }
                }
                return Evaluated {
                    value: Value::Row(Record {
                        present: self.terms.or(presence),
                        fields,
                    }),
                    defined: key.defined,
                };
            }
            ExprAST::Field { record, name } => {
                let record = self.expression(record, scope);
                let Value::Row(row) = record.value else {
                    unreachable!("checked record access")
                };
                Checked {
                    value: row.fields[name],
                    defined: self.terms.and([record.defined, row.present]),
                }
            }
            ExprAST::Quantifier {
                universal,
                binder,
                table,
                body,
            } => {
                let mut result = if *universal {
                    Checked::TRUE
                } else {
                    Checked::FALSE
                };
                let records: Vec<_> = self.tables[table]
                    .iter()
                    .map(|slot| slot.record.clone())
                    .collect();
                let mut scope = scope.clone();
                for record in records {
                    let present = record.present;
                    scope.insert(binder.clone(), Value::Row(record));
                    let body = self.expression(body, &scope).scalar();
                    let skipped = if *universal {
                        Checked::TRUE
                    } else {
                        Checked::FALSE
                    };
                    let body = self.select(present, body, skipped);
                    result = self.short_circuit(result, body, *universal);
                }
                result
            }
            ExprAST::Not { arg } => {
                let arg = self.expression(arg, scope).scalar();
                Checked {
                    value: self.terms.not(arg.value),
                    defined: arg.defined,
                }
            }
            ExprAST::Neg { arg } => {
                let arg = self.expression(arg, scope).scalar();
                let zero = Checked::total(self.terms.int(0));
                self.arithmetic(Operator::Sub, zero, arg)
            }
            ExprAST::Binary { op, left, right } => {
                let left = self.expression(left, scope);
                let right = self.expression(right, scope);
                self.binary(*op, left, right)
            }
            ExprAST::InSet { value, choices } => {
                let value = self.expression(value, scope).scalar();
                let mut result = Checked {
                    value: Terms::FALSE,
                    defined: value.defined,
                };
                for choice in choices {
                    let choice = self.expression(choice, scope).scalar();
                    let same = self.terms.eq(value.value, choice.value);
                    result = self.short_circuit(
                        result,
                        Checked {
                            value: same,
                            defined: choice.defined,
                        },
                        false,
                    );
                }
                result
            }
            ExprAST::If { condition, yes, no } => {
                let condition = self.expression(condition, scope).scalar();
                let yes = self.expression(yes, scope);
                let no = self.expression(no, scope);
                let defined = self.terms.ite(condition.value, yes.defined, no.defined);
                let value = match (yes.value, no.value) {
                    (Value::Scalar(yes), Value::Scalar(no)) => {
                        Value::Scalar(self.terms.ite(condition.value, yes, no))
                    }
                    (Value::Row(yes), Value::Row(no)) => Value::Row(Record {
                        present: self.terms.ite(condition.value, yes.present, no.present),
                        fields: yes
                            .fields
                            .into_iter()
                            .map(|(name, term)| {
                                let value = self.terms.ite(condition.value, term, no.fields[&name]);
                                (name, value)
                            })
                            .collect(),
                    }),
                    (Value::None, Value::None) => Value::None,
                    _ => unreachable!("checked branch types"),
                };
                return Evaluated {
                    value,
                    defined: self.terms.and([condition.defined, defined]),
                };
            }
        };
        Evaluated::from_scalar(scalar)
    }

    fn select(&mut self, condition: Term, yes: Checked, no: Checked) -> Checked {
        Checked {
            value: self.terms.ite(condition, yes.value, no.value),
            defined: self.terms.ite(condition, yes.defined, no.defined),
        }
    }

    fn short_circuit(&mut self, left: Checked, right: Checked, conjunction: bool) -> Checked {
        let branch = if conjunction {
            self.select(left.value, right, Checked::FALSE)
        } else {
            self.select(left.value, Checked::TRUE, right)
        };
        Checked {
            value: branch.value,
            defined: self.terms.and([left.defined, branch.defined]),
        }
    }

    fn arithmetic(&mut self, op: Operator, left: Checked, right: Checked) -> Checked {
        let value = self.terms.op(op, vec![left.value, right.value], Sort::Int);
        let in_range = self.terms.bounds(value, i128::MIN, i128::MAX);
        Checked {
            value,
            defined: self.terms.and([left.defined, right.defined, in_range]),
        }
    }

    fn binary(&mut self, op: Op, left: Evaluated, right: Evaluated) -> Checked {
        use Operator as O;
        if matches!(op, Op::Eq | Op::Ne) {
            let equal = match (left.value, right.value) {
                (Value::Scalar(a), Value::Scalar(b)) => self.terms.eq(a, b),
                (Value::Row(row), Value::None) | (Value::None, Value::Row(row)) => {
                    self.terms.not(row.present)
                }
                _ => unreachable!("checked equality types"),
            };
            return Checked {
                value: if matches!(op, Op::Eq) {
                    equal
                } else {
                    self.terms.not(equal)
                },
                defined: self.terms.and([left.defined, right.defined]),
            };
        }
        let left = left.scalar();
        let right = right.scalar();
        match op {
            Op::And => self.short_circuit(left, right, true),
            Op::Or => self.short_circuit(left, right, false),
            Op::Implies => {
                let left = Checked {
                    value: self.terms.not(left.value),
                    ..left
                };
                self.short_circuit(left, right, false)
            }
            Op::Add => self.arithmetic(O::Add, left, right),
            Op::Sub => self.arithmetic(O::Sub, left, right),
            Op::Mul => self.arithmetic(O::Mul, left, right),
            Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                let (a, b) = if matches!(op, Op::Gt | Op::Ge) {
                    (right, left)
                } else {
                    (left, right)
                };
                let comparison = if matches!(op, Op::Lt | Op::Gt) {
                    O::Lt
                } else {
                    O::Le
                };
                Checked {
                    value: self.terms.compare(comparison, a.value, b.value),
                    defined: self.terms.and([a.defined, b.defined]),
                }
            }
            _ => unreachable!("checked binary operator"),
        }
    }

    fn domain_constraint(&mut self, domain: &StateDomain, value: Term) -> Term {
        match domain.domain_kind {
            "bounded_int" => {
                self.terms
                    .bounds(value, domain.min_val.unwrap(), domain.max_val.unwrap())
            }
            "enum" | "id" => {
                let matches: Vec<_> = domain
                    .variants
                    .iter()
                    .map(|name| {
                        let variant = self.symbol(name);
                        self.terms.eq(value, variant)
                    })
                    .collect();
                self.terms.or(matches)
            }
            _ => Terms::TRUE,
        }
    }

    fn action(&mut self, model: &NormalizedBehaviorModel, action: &NormalizedAction) -> Action {
        let scope = action
            .args
            .iter()
            .map(|(name, value)| (name.clone(), Value::Scalar(self.literal(value))))
            .collect();
        let guard = action.guard.as_ref().map_or(Checked::TRUE, |guard| {
            self.expression(&guard.ast, &scope).scalar()
        });
        let enabled = guard.succeeds(&mut self.terms);
        let mut ok = enabled;
        let mut error = self.terms.not(guard.defined);
        let mut next: Vec<_> = (0..self.columns.len())
            .map(|column| self.column_value(column))
            .collect();

        for assignment in &action.updates {
            let column = self.scalars[&assignment.target_var];
            let domain = model
                .domains
                .iter()
                .find(|domain| domain.var_name == assignment.target_var)
                .unwrap();
            let value = self.expression(&assignment.value_expr.ast, &scope).scalar();
            self.check_update(&mut ok, &mut error, domain, value);
            next[column] = value.value;
        }
        for (table, rows) in &action.inserts {
            let domain = model
                .domains
                .iter()
                .find(|domain| &domain.var_name == table)
                .unwrap();
            let slots = self.tables[table].clone();
            for row in rows {
                let mut fields = BTreeMap::new();
                for (field, expr) in row {
                    let value = self.expression(&expr.ast, &scope).scalar();
                    self.check_update(&mut ok, &mut error, &domain.table_fields[field], value);
                    fields.insert(field.as_str(), value.value);
                }
                let mut count = self.terms.int(0);
                let one = self.terms.int(1);
                let zero = self.terms.int(0);
                for slot in &slots {
                    let present = next[slot.presence_column];
                    let adding_here = self.terms.eq(fields["id"], slot.id);
                    let duplicate = self.terms.and([present, adding_here]);
                    let fresh = self.terms.not(duplicate);
                    ok = self.terms.and([ok, fresh]);
                    let increment = self.terms.ite(present, one, zero);
                    count = self
                        .terms
                        .op(Operator::Add, vec![count, increment], Sort::Int);
                    next[slot.presence_column] = self.terms.or([present, adding_here]);
                    for (field, column) in &slot.columns {
                        next[*column] =
                            self.terms
                                .ite(adding_here, fields[field.as_str()], next[*column]);
                    }
                }
                let capacity = self.terms.int(domain.max_rows.unwrap() as i128);
                let has_space = self.terms.compare(Operator::Lt, count, capacity);
                ok = self.terms.and([ok, has_space]);
            }
        }
        let unsuccessful = self.terms.not(ok);
        let no_error = self.terms.not(error);
        let rejected = self.terms.and([enabled, unsuccessful, no_error]);
        Action {
            enabled,
            error,
            rejected,
            success: ok,
            next,
        }
    }

    fn check_update(
        &mut self,
        ok: &mut Term,
        error: &mut Term,
        domain: &StateDomain,
        value: Checked,
    ) {
        let undefined = self.terms.not(value.defined);
        let first_error = self.terms.and([*ok, undefined]);
        *error = self.terms.or([*error, first_error]);
        let in_domain = self.domain_constraint(domain, value.value);
        *ok = self.terms.and([*ok, value.defined, in_domain]);
    }
}
