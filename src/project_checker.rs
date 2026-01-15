use crate::diagnostic::Diagnostic;
use crate::project::{NormalizedRequire, ProjectAST};
use std::collections::HashMap;

pub fn check_project(project: &ProjectAST) -> Vec<Diagnostic> {
    let normalized = project.collect_normalized_requires();
    ProjectChecker::new(project, normalized).check()
}

struct ProjectChecker<'a> {
    project: &'a ProjectAST,
    normalized: Vec<NormalizedRequire>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> ProjectChecker<'a> {
    fn new(project: &'a ProjectAST, normalized: Vec<NormalizedRequire>) -> Self {
        Self {
            project,
            normalized,
            diagnostics: Vec::new(),
        }
    }

    fn check(mut self) -> Vec<Diagnostic> {
        self.check_requires();
        self.check_cycles();
        self.diagnostics
    }

    fn check_requires(&mut self) {
        for edge in &self.normalized {
            if !self.project.mods.contains_key(&edge.to_mod) {
                let msg = format!(
                    "Module '{}' (required from '{}') does not exist",
                    edge.to_mod, edge.from_mod
                );
                self.diagnostics.push(Diagnostic {
                    severity: "error".into(),
                    kind: "UnresolvedRequire".into(),
                    message: msg,
                    location: edge.file.display().to_string(),
                });
            }
        }
    }

    fn check_cycles(&mut self) {
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        let mut edge_map: HashMap<(String, String), &NormalizedRequire> = HashMap::new();

        for edge in &self.normalized {
            graph.entry(edge.from_mod.clone()).or_default();
            graph.entry(edge.to_mod.clone()).or_default();
            graph
                .entry(edge.from_mod.clone())
                .or_default()
                .push(edge.to_mod.clone());
            edge_map.insert((edge.from_mod.clone(), edge.to_mod.clone()), edge);
        }

        #[derive(Copy, Clone, PartialEq)]
        enum Color {
            White,
            Gray,
            Black,
        }

        fn dfs(
            node: &str,
            graph: &HashMap<String, Vec<String>>,
            color: &mut HashMap<String, Color>,
            stack: &mut Vec<String>,
            cycles: &mut Vec<Vec<String>>,
        ) {
            color.insert(node.to_string(), Color::Gray);
            stack.push(node.to_string());

            if let Some(neighbors) = graph.get(node) {
                for next in neighbors {
                    match color.get(next).copied().unwrap_or(Color::White) {
                        Color::White => dfs(next, graph, color, stack, cycles),
                        Color::Gray => {
                            if let Some(pos) = stack.iter().position(|s| s == next) {
                                cycles.push(stack[pos..].to_vec());
                            }
                        }
                        Color::Black => {}
                    }
                }
            }

            color.insert(node.to_string(), Color::Black);
            stack.pop();
        }

        let mut color: HashMap<String, Color> =
            graph.keys().map(|k| (k.clone(), Color::White)).collect();
        let mut cycles = Vec::new();

        for node in graph.keys() {
            if color.get(node) == Some(&Color::White) {
                let mut stack = Vec::new();
                dfs(node, &graph, &mut color, &mut stack, &mut cycles);
            }
        }

        for cycle in cycles {
            if cycle.len() >= 2 {
                let mut path = cycle.clone();
                path.push(cycle[0].clone());
                let msg = format!("Require cycle detected: {}", path.join(" -> "));
                let file = path
                    .windows(2)
                    .filter_map(|pair| {
                        if let [from, to] = pair {
                            edge_map
                                .get(&(from.clone(), to.clone()))
                                .map(|edge| edge.file.display().to_string())
                        } else {
                            None
                        }
                    })
                    .next()
                    .unwrap_or_else(|| "require graph".into());
                self.diagnostics.push(Diagnostic {
                    severity: "error".into(),
                    kind: "RequireCycle".into(),
                    message: msg,
                    location: file,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_surv_file;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn file(path: &str, text: &str) -> (PathBuf, crate::ast::SurvFile) {
        let parsed = parse_surv_file(Cursor::new(text)).expect("parse");
        (PathBuf::from(path), parsed)
    }

    #[test]
    fn reports_unresolved_require() {
        let files = vec![file(
            "a.toml",
            r#"
require = ["mod.missing"]

[mod.alpha]
purpose = "test"
schemas = []
funcs = []
"#,
        )];

        let project = ProjectAST::from_files(files);
        let diags = check_project(&project);
        assert!(diags
            .iter()
            .any(|d| d.kind == "UnresolvedRequire" && d.location.contains("a.toml")));
    }

    #[test]
    fn detects_require_cycles() {
        let files = vec![
            file(
                "a.toml",
                r#"
require = ["mod.beta"]

[mod.alpha]
purpose = "test"
schemas = []
funcs = []
"#,
            ),
            file(
                "b.toml",
                r#"
require = ["mod.alpha"]

[mod.beta]
purpose = "test"
schemas = []
funcs = []
"#,
            ),
        ];

        let project = ProjectAST::from_files(files);
        let diags = check_project(&project);
        assert!(diags.iter().any(|d| d.kind == "RequireCycle"));
    }
}
