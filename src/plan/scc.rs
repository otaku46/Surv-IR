use super::graph::{EdgeKind, PlanDependencyGraph, PlanNode};
use super::loader::LoadedPlanContext;
use crate::Diagnostic;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Serialize)]
pub struct PlanWorkUnit {
    pub unit_id: String,
    pub unit_kind: String,
    pub member_nodes: Vec<PlanNode>,
    pub hard_dep_units: BTreeSet<String>,
    pub soft_pipeline_preds: BTreeSet<String>,
    pub is_shared_boundary: bool,
}

#[derive(Clone, Default, Serialize)]
pub struct CondensedPlanDag {
    pub units: Vec<PlanWorkUnit>,
    pub topological_order: Vec<String>,
    pub unit_by_node: HashMap<String, String>,
    pub has_errors: bool,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn condense_scc_and_rank(
    context: &LoadedPlanContext,
    graph: &PlanDependencyGraph,
) -> CondensedPlanDag {
    let scope_res = graph.scope(context.request.filter_mod.as_deref());
    let scope = match scope_res {
        Ok(s) => s,
        Err(err) => {
            return CondensedPlanDag {
                units: Vec::new(),
                topological_order: Vec::new(),
                unit_by_node: HashMap::new(),
                has_errors: true,
                diagnostics: vec![super::error(
                    "InvalidPlanScope",
                    err,
                    &context.request.ir_path,
                )],
            };
        }
    };

    match condense_scc_and_rank_inner(graph, &scope, &context.request.ir_path) {
        Ok(dag) => dag,
        Err(diag) => CondensedPlanDag {
            units: Vec::new(),
            topological_order: Vec::new(),
            unit_by_node: HashMap::new(),
            has_errors: true,
            diagnostics: vec![diag],
        },
    }
}

pub(super) fn condense_scc_and_rank_inner(
    graph: &PlanDependencyGraph,
    scope: &BTreeSet<String>,
    ir_path: &Path,
) -> Result<CondensedPlanDag, Diagnostic> {
    let mut tarjan = Tarjan::default();
    for id in scope {
        if !tarjan.indices.contains_key(id.as_str()) {
            tarjan.visit(id, graph);
        }
    }

    let mut unit_map = BTreeMap::new();
    let mut unit_by_node = HashMap::new();
    for mut component in tarjan.components {
        component.sort_unstable();
        let unit_kind = match (
            graph.nodes[component[0]].node_kind.as_str(),
            component.len(),
        ) {
            ("schema", 1) => "schema",
            ("schema", _) => "schema_scc",
            ("func", 1) => "func",
            ("mod", 1)
                if !graph
                    .dependencies(component[0])
                    .any(|(to, kind)| kind != EdgeKind::PipelineOrder && to == component[0]) =>
            {
                "mod_integration"
            }
            _ => {
                return Err(super::error(
                    "CyclicPlanDependencies",
                    format!("Cyclic module dependencies: {}", component.join(", ")),
                    ir_path,
                ));
            }
        };
        let id = if unit_kind == "schema_scc" {
            format!("scc:{}", component[0])
        } else {
            component[0].to_string()
        };
        let member_nodes: Vec<_> = component
            .into_iter()
            .map(|member| {
                unit_by_node.insert(member.to_string(), id.clone());
                graph.nodes[member].clone()
            })
            .collect();
        let is_shared_boundary = unit_kind != "func"
            && unit_kind != "mod_integration"
            && member_nodes
                .iter()
                .flat_map(|node| &node.referenced_by_mods)
                .collect::<BTreeSet<_>>()
                .len()
                > 1;
        unit_map.insert(
            id.clone(),
            PlanWorkUnit {
                unit_id: id,
                unit_kind: unit_kind.to_string(),
                member_nodes,
                hard_dep_units: BTreeSet::new(),
                soft_pipeline_preds: BTreeSet::new(),
                is_shared_boundary,
            },
        );
    }

    for unit in unit_map.values_mut() {
        for node in &unit.member_nodes {
            for (to, kind) in graph.dependencies(&node.fq_name) {
                if let Some(other) = unit_by_node.get(to).filter(|other| **other != unit.unit_id) {
                    if kind == EdgeKind::PipelineOrder {
                        unit.soft_pipeline_preds.insert(other.clone());
                    } else {
                        unit.hard_dep_units.insert(other.clone());
                    }
                }
            }
        }
    }

    let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut counts = HashMap::new();
    let mut ready = BTreeSet::new();
    for (id, unit) in &unit_map {
        counts.insert(id.as_str(), unit.hard_dep_units.len());
        if unit.hard_dep_units.is_empty() {
            ready.insert(id.as_str());
        }
        for dep in &unit.hard_dep_units {
            reverse.entry(dep).or_default().push(id);
        }
    }
    let mut topological_order = Vec::with_capacity(unit_map.len());
    while let Some(next) = ready.pop_first() {
        topological_order.push(next.to_string());
        for dependent in reverse.get(next).into_iter().flatten() {
            let count = counts.get_mut(dependent).unwrap();
            *count -= 1;
            if *count == 0 {
                ready.insert(dependent);
            }
        }
    }
    if topological_order.len() != unit_map.len() {
        return Err(super::error(
            "CyclicPlanDependencies",
            "Could not rank the condensed dependency graph".into(),
            ir_path,
        ));
    }

    let units = unit_map.into_values().collect();
    Ok(CondensedPlanDag {
        units,
        topological_order,
        unit_by_node,
        has_errors: false,
        diagnostics: Vec::new(),
    })
}

#[derive(Default)]
struct Tarjan<'a> {
    indices: HashMap<&'a str, usize>,
    low_links: HashMap<&'a str, usize>,
    stack: Vec<&'a str>,
    active: HashSet<&'a str>,
    components: Vec<Vec<&'a str>>,
}

impl<'a> Tarjan<'a> {
    fn visit(&mut self, id: &'a str, graph: &'a PlanDependencyGraph) {
        let index = self.indices.len();
        self.indices.insert(id, index);
        self.low_links.insert(id, index);
        self.stack.push(id);
        self.active.insert(id);

        for (target, kind) in graph.dependencies(id) {
            if kind == EdgeKind::PipelineOrder {
                continue;
            }
            let low = if !self.indices.contains_key(target) {
                self.visit(target, graph);
                self.low_links[target]
            } else if self.active.contains(target) {
                self.indices[target]
            } else {
                continue;
            };
            self.low_links
                .entry(id)
                .and_modify(|value| *value = (*value).min(low));
        }

        if self.low_links[id] == index {
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.active.remove(member);
                component.push(member);
                if member == id {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}
