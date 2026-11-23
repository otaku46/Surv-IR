use super::explorer::{replay_symbolic_witness, ExplorationOutcome, ReplayedWitness, Witness};
use super::model::{BehaviorDiagnostic, NormalizedBehaviorModel};
use super::symbolic::{lower, Node, Operator, Sort, SymbolicBehavior, Term, Terms};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub(super) fn verify(
    model: &NormalizedBehaviorModel,
    diagnostics: &mut Vec<BehaviorDiagnostic>,
) -> Result<ExplorationOutcome, Error> {
    let mut symbolic = lower(model)?;
    let terms = &mut symbolic.terms;
    let terminal_error = terms.not(symbolic.terminal.defined);
    let mut failures = vec![terminal_error];
    for invariant in &symbolic.invariants {
        let success = invariant.succeeds(terms);
        failures.push(terms.not(success));
    }
    if model.config.check_deadlock {
        let enabled = terms.or(symbolic.actions.iter().map(|action| action.enabled));
        let no_action = terms.not(enabled);
        let nonterminal = terms.not(symbolic.terminal.value);
        failures.push(terms.and([nonterminal, no_action]));
    }
    let violation = terms.or(failures);
    let goals: BTreeMap<_, _> = symbolic
        .goals
        .iter()
        .map(|(name, goal)| {
            let undefined = terms.not(goal.defined);
            (name, terms.or([undefined, goal.value]))
        })
        .collect();

    let mut solver = Solver::start()?;
    frame(&symbolic, &mut solver.input, 0).map_err(io_error)?;
    for (column, data) in symbolic.columns.iter().enumerate() {
        writeln!(
            solver.input,
            "(assert (= s_0_{column} {}))",
            reference(&symbolic.terms, 0, data.initial),
        )
        .map_err(io_error)?;
    }

    let mut report = ExplorationOutcome {
        behavior_name: model.name.clone(),
        stop_reason: "solver_unknown",
        explored_states: 0,
        pending_states: 0,
        max_depth_reached: 0,
        counterexample: None,
        reached: BTreeMap::new(),
        unreached: model.goals.keys().cloned().collect(),
    };

    for depth in 0..=model.config.max_depth {
        report.max_depth_reached = depth;
        match solver.query(&reference(&symbolic.terms, depth, violation), Some(depth))? {
            Query::Sat(actions) => {
                let ReplayedWitness::Violation(trace) =
                    replay_symbolic_witness(model, &actions, Witness::StateViolation, diagnostics)?
                else {
                    unreachable!()
                };
                report.counterexample = Some(*trace);
                report.stop_reason = "violation_found";
                break;
            }
            Query::Unknown => break,
            Query::Unsat => {}
        }

        let mut unknown = false;
        for (name, condition) in &goals {
            if report.reached.contains_key(*name) {
                continue;
            }
            match solver.query(&reference(&symbolic.terms, depth, *condition), Some(depth))? {
                Query::Sat(actions) => {
                    let ReplayedWitness::Goal(trace) =
                        replay_symbolic_witness(model, &actions, Witness::Goal(name), diagnostics)?
                    else {
                        unreachable!()
                    };
                    report.reached.insert((*name).clone(), trace);
                }
                Query::Unknown => unknown = true,
                Query::Unsat => {}
            }
        }

        let capped = depth == model.config.max_depth;
        let attempted = selected(&symbolic, depth, |action| {
            let error = reference(&symbolic.terms, depth, action.error);
            if capped {
                error
            } else {
                let rejected = reference(&symbolic.terms, depth, action.rejected);
                format!("(or {error} {rejected})")
            }
        });
        match solver.query(&attempted, Some(depth + 1))? {
            Query::Sat(mut actions) => {
                let attempted = actions.pop().expect("query included the attempted action");
                let ReplayedWitness::Violation(trace) = replay_symbolic_witness(
                    model,
                    &actions,
                    Witness::FailedAction(attempted),
                    diagnostics,
                )?
                else {
                    unreachable!()
                };
                report.counterexample = Some(*trace);
                report.stop_reason = "violation_found";
                break;
            }
            Query::Unknown => break,
            Query::Unsat => {}
        }
        if capped {
            let rejected = selected(&symbolic, depth, |action| {
                reference(&symbolic.terms, depth, action.rejected)
            });
            match solver.query(&rejected, None)? {
                Query::Sat(_) => {
                    report.stop_reason = "max_depth_reached";
                    break;
                }
                Query::Unknown => break,
                Query::Unsat => {}
            }
        }

        extend(&symbolic, &mut solver.input, depth).map_err(io_error)?;
        match solver.query("true", None)? {
            Query::Sat(_) if capped => {
                report.stop_reason = "max_depth_reached";
                break;
            }
            Query::Sat(_) => {}
            Query::Unknown => break,
            Query::Unsat => {
                report.stop_reason = if report.reached.len() == goals.len() {
                    "exhausted"
                } else if unknown {
                    "solver_unknown"
                } else {
                    "unreachable_goal"
                };
                break;
            }
        }
        if unknown {
            // A later witness can show reachability, but we could no longer
            // guarantee that it is shortest.
            break;
        }
    }
    report
        .unreached
        .retain(|name| !report.reached.contains_key(name));
    Ok(report)
}

fn frame(model: &SymbolicBehavior, out: &mut impl Write, depth: usize) -> std::io::Result<()> {
    for (column, data) in model.columns.iter().enumerate() {
        writeln!(
            out,
            "(declare-const s_{depth}_{column} {})",
            sort(data.sort)
        )?;
    }
    writeln!(out, "(declare-const a_{depth} Int)")?;
    for (term, (node, kind)) in model.terms.nodes.iter().enumerate() {
        let Node::Apply(op, args) = node else {
            continue;
        };
        let children = args
            .iter()
            .map(|arg| reference(&model.terms, depth, *arg))
            .collect::<Vec<_>>();
        writeln!(
            out,
            "(define-fun t_{depth}_{term} () {} ({} {}))",
            sort(*kind),
            operator(*op),
            children.join(" ")
        )?;
    }
    Ok(())
}

fn extend(model: &SymbolicBehavior, out: &mut impl Write, depth: usize) -> std::io::Result<()> {
    let future = depth + 1;
    frame(model, out, future)?;
    let transition = selected(model, depth, |action| {
        let mut clauses = vec![reference(&model.terms, depth, action.success)];
        clauses.extend(action.next.iter().enumerate().map(|(column, term)| {
            format!(
                "(= s_{future}_{column} {})",
                reference(&model.terms, depth, *term)
            )
        }));
        connective("and", "true", clauses)
    });
    writeln!(out, "(assert {transition})")?;
    // Every shortest path is simple. Unsat here proves that all reachable
    // states have shorter paths, even when actions can cycle forever.
    for earlier in 0..future {
        let difference = connective(
            "or",
            "false",
            (0..model.columns.len())
                .map(|column| format!("(distinct s_{earlier}_{column} s_{future}_{column})")),
        );
        writeln!(out, "(assert {difference})")?;
    }
    Ok(())
}

fn selected(
    model: &SymbolicBehavior,
    depth: usize,
    condition: impl Fn(&super::symbolic::Action) -> String,
) -> String {
    connective(
        "or",
        "false",
        model
            .actions
            .iter()
            .enumerate()
            .map(|(index, action)| format!("(and (= a_{depth} {index}) {})", condition(action))),
    )
}

fn connective(name: &str, identity: &str, terms: impl IntoIterator<Item = String>) -> String {
    let terms: Vec<_> = terms.into_iter().collect();
    match terms.as_slice() {
        [] => identity.into(),
        [term] => term.clone(),
        _ => format!("({name} {})", terms.join(" ")),
    }
}

fn sort(sort: Sort) -> &'static str {
    match sort {
        Sort::Bool => "Bool",
        Sort::Int => "Int",
    }
}

fn operator(op: Operator) -> &'static str {
    match op {
        Operator::Not => "not",
        Operator::And => "and",
        Operator::Or => "or",
        Operator::Eq => "=",
        Operator::Lt => "<",
        Operator::Le => "<=",
        Operator::Add => "+",
        Operator::Sub => "-",
        Operator::Mul => "*",
        Operator::Ite => "ite",
    }
}

fn reference(terms: &Terms, depth: usize, term: Term) -> String {
    match &terms.nodes[term].0 {
        Node::Bool(value) => value.to_string(),
        Node::Int(value) if *value < 0 => format!("(- {})", value.unsigned_abs()),
        Node::Int(value) => value.to_string(),
        Node::Column(column) => format!("s_{depth}_{column}"),
        Node::Apply(..) => format!("t_{depth}_{term}"),
    }
}

enum Query {
    Sat(Vec<usize>),
    Unsat,
    Unknown,
}

struct Solver {
    child: Child,
    input: std::io::BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

pub(super) enum Error {
    Solver(String),
    Evaluation(String),
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Evaluation(message)
    }
}

fn io_error(error: std::io::Error) -> Error {
    Error::Solver(format!("could not communicate with Z3: {error}"))
}

impl Solver {
    fn start() -> Result<Self, Error> {
        let mut child = Command::new("z3")
            .args(["-in", "-smt2"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| Error::Solver(format!("could not start z3 from PATH: {error}")))?;
        let input = std::io::BufWriter::new(child.stdin.take().unwrap());
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut solver = Self {
            child,
            input,
            output,
        };
        writeln!(
            solver.input,
            "(set-option :produce-models true)\n(set-option :timeout 10000)"
        )
        .map_err(io_error)?;
        Ok(solver)
    }

    fn query(&mut self, formula: &str, witness_length: Option<usize>) -> Result<Query, Error> {
        if formula == "false" {
            return Ok(Query::Unsat);
        }
        writeln!(self.input, "(push)\n(assert {formula})\n(check-sat)").map_err(io_error)?;
        self.input.flush().map_err(io_error)?;
        let result = match self.line()?.trim() {
            "sat" => Query::Sat(self.actions(witness_length.unwrap_or(0))?),
            "unsat" => Query::Unsat,
            "unknown" => Query::Unknown,
            response => return Err(Error::Solver(format!("unexpected Z3 response: {response}"))),
        };
        writeln!(self.input, "(pop)").map_err(io_error)?;
        Ok(result)
    }

    fn actions(&mut self, length: usize) -> Result<Vec<usize>, Error> {
        if length == 0 {
            return Ok(Vec::new());
        }
        let keys: Vec<_> = (0..length).map(|depth| format!("a_{depth}")).collect();
        writeln!(self.input, "(get-value ({}))", keys.join(" ")).map_err(io_error)?;
        self.input.flush().map_err(io_error)?;
        let mut reply = String::new();
        let mut nesting = 0isize;
        loop {
            let line = self.line()?;
            if line.trim_start().starts_with("(error ") {
                return Err(Error::Solver(format!(
                    "Z3 could not return action choices: {}",
                    line.trim()
                )));
            }
            for byte in line.bytes() {
                nesting += isize::from(byte == b'(') - isize::from(byte == b')');
            }
            reply.push_str(&line);
            if nesting <= 0 {
                break;
            }
        }
        let mut values = reply
            .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')'))
            .filter(|value| !value.is_empty());
        let mut result = Vec::with_capacity(length);
        for key in keys {
            if values.next() != Some(key.as_str()) {
                return Err(Error::Solver(format!(
                    "Z3 did not return {key}: {}",
                    reply.trim()
                )));
            }
            result.push(
                values
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or_else(|| {
                        Error::Solver(format!("Z3 returned an invalid action choice for {key}"))
                    })?,
            );
        }
        if values.next().is_some() {
            return Err(Error::Solver(format!(
                "unexpected extra values in Z3 reply: {}",
                reply.trim()
            )));
        }
        Ok(result)
    }

    fn line(&mut self) -> Result<String, Error> {
        loop {
            let mut line = String::new();
            if self.output.read_line(&mut line).map_err(io_error)? == 0 {
                return Err(Error::Solver(
                    "Z3 closed its output before returning a result".into(),
                ));
            }
            if !line.trim().is_empty() {
                return Ok(line);
            }
        }
    }
}

impl Drop for Solver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
