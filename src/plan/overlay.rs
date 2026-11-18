use super::loader::LoadedPlanContext;
use super::scc::CondensedPlanDag;
use crate::diff_impl::{diff_expected_symbols, ExpectedSymbol};
use crate::Diagnostic;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImplState {
    Done,
    Drifted,
    Ambiguous,
    Missing,
    OutOfScope,
    Unverified,
}

impl ImplState {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Drifted => "drifted",
            Self::Ambiguous => "ambiguous",
            Self::Missing => "missing",
            Self::OutOfScope => "out_of_scope",
            Self::Unverified => "unverified",
        }
    }
}

#[derive(Clone, Serialize)]
pub struct NodeImplStatus {
    pub fq_name: String,
    pub impl_state: ImplState,
    pub matched_location: Option<String>,
    pub drift_problems: Vec<String>,
    pub candidate_count: usize,
    pub is_module: bool,
}

#[derive(Serialize)]
pub struct OverlayedPlanDag {
    pub dag: Option<CondensedPlanDag>,
    pub node_statuses: Vec<NodeImplStatus>,
    pub workspace_checked: bool,
    pub has_errors: bool,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn apply_design_only_overlay(
    _context: &LoadedPlanContext,
    dag: &CondensedPlanDag,
) -> OverlayedPlanDag {
    if dag.has_errors {
        return OverlayedPlanDag {
            dag: None,
            node_statuses: Vec::new(),
            workspace_checked: false,
            has_errors: true,
            diagnostics: dag.diagnostics.clone(),
        };
    }

    let mut node_statuses = Vec::new();
    for unit in &dag.units {
        for node in &unit.member_nodes {
            node_statuses.push(NodeImplStatus {
                fq_name: node.fq_name.clone(),
                impl_state: ImplState::Unverified,
                matched_location: None,
                drift_problems: Vec::new(),
                candidate_count: 0,
                is_module: node.node_kind == "mod",
            });
        }
    }

    OverlayedPlanDag {
        dag: Some(CondensedPlanDag {
            units: dag.units.clone(),
            topological_order: dag.topological_order.clone(),
            unit_by_node: dag.unit_by_node.clone(),
            has_errors: false,
            diagnostics: Vec::new(),
        }),
        node_statuses,
        workspace_checked: false,
        has_errors: false,
        diagnostics: Vec::new(),
    }
}

pub fn apply_workspace_diff_overlay(
    context: &LoadedPlanContext,
    dag: &CondensedPlanDag,
) -> OverlayedPlanDag {
    if dag.has_errors {
        return OverlayedPlanDag {
            dag: None,
            node_statuses: Vec::new(),
            workspace_checked: true,
            has_errors: true,
            diagnostics: dag.diagnostics.clone(),
        };
    }

    let Some(workspace) = &context.request.workspace_root else {
        return apply_design_only_overlay(context, dag);
    };

    let graph = super::graph::build_plan_dependency_graph(context);

    let mut statuses: BTreeMap<String, NodeImplStatus> = dag
        .units
        .iter()
        .flat_map(|u| &u.member_nodes)
        .map(|node| {
            let impl_state = if node
                .expected
                .as_ref()
                .is_some_and(|expected| !in_language(expected, &context.request.lang))
            {
                ImplState::OutOfScope
            } else {
                ImplState::Unverified
            };
            (
                node.fq_name.clone(),
                NodeImplStatus {
                    fq_name: node.fq_name.clone(),
                    impl_state,
                    matched_location: None,
                    drift_problems: Vec::new(),
                    candidate_count: 0,
                    is_module: node.node_kind == "mod",
                },
            )
        })
        .collect();

    // Include same-name peers outside --mod so a single implementation cannot
    // satisfy the selected package merely because its peers were filtered out.
    let search_names: BTreeSet<_> = statuses
        .keys()
        .filter_map(|id| graph.nodes.get(id).and_then(|n| n.expected.as_ref()))
        .filter(|expected| in_language(expected, &context.request.lang))
        .map(ExpectedSymbol::search_name)
        .collect();

    let expected: Vec<_> = graph
        .nodes
        .values()
        .filter_map(|node| node.expected.as_ref())
        .filter(|expected| {
            search_names.contains(expected.search_name())
                && in_language(expected, &context.request.lang)
        })
        .cloned()
        .collect();

    let diff_result = diff_expected_symbols(
        &expected,
        workspace,
        &context.request.lang,
        &context.request.strategy,
        context.request.mapping_path.as_deref(),
        true,
    );

    let (diff, matched) = match diff_result {
        Ok(res) => res,
        Err(err) => {
            return OverlayedPlanDag {
                dag: None,
                node_statuses: Vec::new(),
                workspace_checked: true,
                has_errors: true,
                diagnostics: vec![super::error(
                    "PlanOverlayFailed",
                    err.to_string(),
                    workspace,
                )],
            };
        }
    };

    for (id, symbol) in matched {
        if let Some(status) = statuses.get_mut(&id) {
            status.impl_state = ImplState::Done;
            status.candidate_count = 1;
            status.matched_location = Some(format!(
                "{}:{}",
                symbol.relative_path.unwrap_or(symbol.uri),
                symbol.range.start_line + 1
            ));
        }
    }
    for expected_sym in diff.missing {
        if let Some(status) = expected_sym.stable_id.and_then(|id| statuses.get_mut(&id)) {
            status.impl_state = ImplState::Missing;
        }
    }
    for (expected_sym, candidates) in diff.ambiguous {
        if let Some(status) = expected_sym.stable_id.and_then(|id| statuses.get_mut(&id)) {
            status.impl_state = ImplState::Ambiguous;
            status.candidate_count = candidates.len();
        }
    }
    for (expected_sym, problems) in diff
        .signature_mismatches
        .into_iter()
        .map(|issue| (issue.expected, issue.problems))
        .chain(
            diff.schema_field_mismatches
                .into_iter()
                .map(|issue| (issue.expected, issue.problems)),
        )
    {
        if let Some(status) = expected_sym.stable_id.and_then(|id| statuses.get_mut(&id)) {
            status.impl_state = ImplState::Drifted;
            status.drift_problems.extend(problems);
        }
    }

    OverlayedPlanDag {
        dag: Some(CondensedPlanDag {
            units: dag.units.clone(),
            topological_order: dag.topological_order.clone(),
            unit_by_node: dag.unit_by_node.clone(),
            has_errors: false,
            diagnostics: Vec::new(),
        }),
        node_statuses: statuses.into_values().collect(),
        workspace_checked: true,
        has_errors: false,
        diagnostics: Vec::new(),
    }
}

fn in_language(expected: &ExpectedSymbol, lang: &str) -> bool {
    match lang {
        "all" => ["rust", "ts", "c", "zig"]
            .iter()
            .any(|lang| expected.matches_language(lang)),
        "both" => expected.matches_language("rust") || expected.matches_language("ts"),
        _ => expected.matches_language(lang),
    }
}
