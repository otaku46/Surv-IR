use super::graph::{EdgeKind, PlanDependencyGraph};
use super::loader::LoadedPlanContext;
use super::overlay::{ImplState, NodeImplStatus, OverlayedPlanDag};
use super::scc::{CondensedPlanDag, PlanWorkUnit};
use crate::Diagnostic;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Done,
    Ready,
    Blocked,
    OutOfScope,
    Unverified,
}

impl Readiness {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::OutOfScope => "out_of_scope",
            Self::Unverified => "unverified",
        }
    }

    fn complete(self) -> bool {
        matches!(self, Self::Done | Self::OutOfScope)
    }
}

#[derive(Clone, Serialize)]
pub struct ScheduledUnit {
    pub unit: PlanWorkUnit,
    pub wave_index: usize,
    pub readiness: Readiness,
    pub blocked_by_units: Vec<String>,
    pub drift_problems: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct PlanWave {
    pub wave_index: usize,
    pub label: String,
    pub units: Vec<ScheduledUnit>,
}

#[derive(Clone, Serialize)]
pub struct ParallelTrack {
    pub track_id: String,
    pub owner_mod: String,
    pub target_impl_paths: BTreeSet<String>,
    pub prerequisite_boundary_units: BTreeSet<String>,
    pub ordered_units: Vec<ScheduledUnit>,
}

#[derive(Clone, Serialize)]
pub struct ModulePlanSummary {
    pub mod_name: String,
    pub source_file: std::path::PathBuf,
    pub total_symbols: usize,
    pub done_symbols: usize,
    pub drifted_symbols: usize,
    pub unverified_symbols: usize,
    pub derived_state: String,
    pub derived_coverage: f64,
    pub out_of_scope: bool,
}

#[derive(Serialize)]
pub struct ScheduledPlan {
    pub project_name: String,
    pub workspace_checked: bool,
    pub waves: Vec<PlanWave>,
    pub parallel_tracks: Vec<ParallelTrack>,
    pub module_summaries: Vec<ModulePlanSummary>,
    pub diagnostics: Vec<Diagnostic>,
    pub node_statuses: Vec<NodeImplStatus>,
}

pub fn schedule_waves_and_tracks(
    context: &LoadedPlanContext,
    overlay: &OverlayedPlanDag,
) -> ScheduledPlan {
    let Some(dag) = &overlay.dag else {
        return ScheduledPlan {
            project_name: context.project_name.clone(),
            workspace_checked: overlay.workspace_checked,
            waves: Vec::new(),
            parallel_tracks: Vec::new(),
            module_summaries: Vec::new(),
            diagnostics: overlay.diagnostics.clone(),
            node_statuses: overlay.node_statuses.clone(),
        };
    };

    let graph = super::graph::build_plan_dependency_graph(context);
    let mut statuses: BTreeMap<String, NodeImplStatus> = overlay
        .node_statuses
        .iter()
        .map(|s| (s.fq_name.clone(), s.clone()))
        .collect();

    let (waves, parallel_tracks, module_summaries) = schedule_waves_and_tracks_inner(
        &graph,
        dag.clone(),
        &mut statuses,
        overlay.workspace_checked,
    );

    ScheduledPlan {
        project_name: context.project_name.clone(),
        workspace_checked: overlay.workspace_checked,
        waves,
        parallel_tracks,
        module_summaries,
        diagnostics: overlay.diagnostics.clone(),
        node_statuses: statuses.into_values().collect(),
    }
}

pub(super) fn schedule_waves_and_tracks_inner(
    graph: &PlanDependencyGraph,
    dag: CondensedPlanDag,
    statuses: &mut BTreeMap<String, NodeImplStatus>,
    workspace_checked: bool,
) -> (Vec<PlanWave>, Vec<ParallelTrack>, Vec<ModulePlanSummary>) {
    let mut scheduled: BTreeMap<String, ScheduledUnit> = BTreeMap::new();
    let mut summaries: BTreeMap<String, ModulePlanSummary> = BTreeMap::new();
    let mut units: BTreeMap<String, PlanWorkUnit> = dag
        .units
        .into_iter()
        .map(|u| (u.unit_id.clone(), u))
        .collect();

    for id in dag.topological_order {
        let unit = units.remove(&id).unwrap();
        let node = &unit.member_nodes[0];
        let blocked_by_units: Vec<_> = unit
            .hard_dep_units
            .iter()
            .filter(|dep| !scheduled[*dep].readiness.complete())
            .cloned()
            .collect();
        let drift_problems = unit
            .member_nodes
            .iter()
            .flat_map(|member| {
                statuses[&member.fq_name]
                    .drift_problems
                    .iter()
                    .map(|problem| format!("{}: {problem}", member.display_ref))
            })
            .collect();

        let readiness = if unit.unit_kind == "mod_integration" {
            let members: BTreeSet<_> = graph
                .dependencies(&node.fq_name)
                .filter_map(|(to, kind)| (kind == EdgeKind::ModMember).then_some(to))
                .collect();
            let children: BTreeSet<_> = graph
                .dependencies(&node.fq_name)
                .filter_map(|(to, kind)| (kind == EdgeKind::ModHierarchy).then_some(to))
                .collect();
            let summary = module_summary(graph, statuses, &summaries, node, &members, &children);
            let in_scope_children = children.iter().any(|child| {
                scheduled[&dag.unit_by_node[*child]].readiness != Readiness::OutOfScope
            });
            let readiness = if summary.derived_state == "blocked" || !blocked_by_units.is_empty() {
                Readiness::Blocked
            } else if workspace_checked
                && (summary.total_symbols > 0
                    || in_scope_children
                    || (members.is_empty()
                        && children.is_empty()
                        && node.manual_state.as_deref() == Some("done")))
            {
                Readiness::Done
            } else if workspace_checked && (!members.is_empty() || !children.is_empty()) {
                Readiness::OutOfScope
            } else {
                Readiness::Ready
            };
            statuses.get_mut(&node.fq_name).unwrap().impl_state = match readiness {
                Readiness::Done => ImplState::Done,
                Readiness::OutOfScope => ImplState::OutOfScope,
                _ => ImplState::Unverified,
            };
            summaries.insert(id.clone(), summary);
            readiness
        } else if unit
            .member_nodes
            .iter()
            .all(|member| statuses[&member.fq_name].impl_state == ImplState::OutOfScope)
        {
            Readiness::OutOfScope
        } else if !blocked_by_units.is_empty() {
            Readiness::Blocked
        } else if workspace_checked
            && unit
                .member_nodes
                .iter()
                .any(|member| statuses[&member.fq_name].impl_state == ImplState::Unverified)
        {
            Readiness::Unverified
        } else if unit.member_nodes.iter().all(|member| {
            matches!(
                statuses[&member.fq_name].impl_state,
                ImplState::Done | ImplState::OutOfScope
            )
        }) {
            Readiness::Done
        } else {
            Readiness::Ready
        };

        let wave_index = if readiness.complete() {
            0
        } else {
            1 + blocked_by_units
                .iter()
                .map(|id| scheduled[id].wave_index)
                .max()
                .unwrap_or(0)
        };
        scheduled.insert(
            id,
            ScheduledUnit {
                unit,
                wave_index,
                readiness,
                blocked_by_units,
                drift_problems,
            },
        );
    }

    let parallel_tracks = parallel_tracks(&scheduled);
    let mut waves = BTreeMap::<usize, Vec<_>>::new();
    for unit in scheduled.into_values() {
        waves.entry(unit.wave_index).or_default().push(unit);
    }
    let waves = waves
        .into_iter()
        .map(|(wave_index, units)| PlanWave {
            wave_index,
            label: if wave_index == 0 {
                "Completed Foundation".into()
            } else {
                format!("Wave {wave_index}")
            },
            units,
        })
        .collect();
    (waves, parallel_tracks, summaries.into_values().collect())
}

fn module_summary(
    graph: &PlanDependencyGraph,
    statuses: &BTreeMap<String, NodeImplStatus>,
    summaries: &BTreeMap<String, ModulePlanSummary>,
    module: &super::graph::PlanNode,
    members: &BTreeSet<&str>,
    children: &BTreeSet<&str>,
) -> ModulePlanSummary {
    let mut schemas = 0;
    let mut funcs = 0;
    let mut done_schemas = 0;
    let mut done_funcs = 0;
    let mut drifted = 0;
    let mut ambiguous = 0;
    let mut unverified = 0;
    for member in members {
        let state = statuses[*member].impl_state;
        if state == ImplState::OutOfScope {
            continue;
        }
        if graph.nodes[*member].node_kind == "schema" {
            schemas += 1;
            done_schemas += usize::from(state == ImplState::Done);
        } else {
            funcs += 1;
            done_funcs += usize::from(state == ImplState::Done);
        }
        drifted += usize::from(state == ImplState::Drifted);
        ambiguous += usize::from(state == ImplState::Ambiguous);
        unverified += usize::from(state == ImplState::Unverified);
    }
    let child_summaries: Vec<&ModulePlanSummary> = children
        .iter()
        .filter_map(|child| summaries.get(*child))
        .collect();
    let all_members_out_of_scope = members
        .iter()
        .all(|member| statuses[*member].impl_state == ImplState::OutOfScope);
    let all_children_out_of_scope = child_summaries.iter().all(|summary| summary.out_of_scope);
    let out_of_scope = all_members_out_of_scope
        && (!members.is_empty() || !children.is_empty())
        && (children.is_empty() || all_children_out_of_scope);
    let total = schemas + funcs;
    let done = done_schemas + done_funcs;
    let derived_state = if module.manual_state.as_deref() == Some("blocked") {
        "blocked"
    } else if out_of_scope {
        "out_of_scope"
    } else if let Some("done") = module.manual_state.as_deref() {
        let children_done = total == 0
            && !child_summaries.is_empty()
            && child_summaries
                .iter()
                .all(|summary| summary.derived_state == "done" || summary.out_of_scope);
        if (members.is_empty() && children.is_empty())
            || (total > 0 && done == total)
            || children_done
        {
            "done"
        } else {
            "partial"
        }
    } else if total > 0 && done == total {
        "done"
    } else if drifted > 0 || ambiguous > 0 || unverified > 0 {
        "partial"
    } else if total == 0 && !child_summaries.is_empty() {
        if child_summaries
            .iter()
            .all(|summary| summary.derived_state == "done" || summary.out_of_scope)
        {
            "done"
        } else if child_summaries
            .iter()
            .any(|summary| summary.derived_state == "partial")
        {
            "partial"
        } else {
            "todo"
        }
    } else if done == 0 {
        "todo"
    } else if schemas > 0 && done_schemas == schemas && funcs > 0 && done_funcs == 0 {
        "skeleton"
    } else {
        "partial"
    };
    let in_scope_children: Vec<&ModulePlanSummary> = child_summaries
        .iter()
        .copied()
        .filter(|summary| !summary.out_of_scope)
        .collect();
    let derived_coverage = if total > 0 {
        done as f64 / total as f64
    } else if !in_scope_children.is_empty() {
        in_scope_children
            .iter()
            .map(|summary| summary.derived_coverage)
            .sum::<f64>()
            / in_scope_children.len() as f64
    } else {
        0.0
    };
    ModulePlanSummary {
        mod_name: module.local_name.clone(),
        source_file: module.source_file.clone(),
        total_symbols: total,
        done_symbols: done,
        drifted_symbols: drifted,
        unverified_symbols: unverified,
        derived_state: derived_state.to_string(),
        derived_coverage,
        out_of_scope,
    }
}

fn parallel_tracks(scheduled: &BTreeMap<String, ScheduledUnit>) -> Vec<ParallelTrack> {
    let mut by_owner: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (id, task) in scheduled {
        if task.readiness.complete()
            || task.unit.is_shared_boundary
            || task.unit.unit_kind == "mod_integration"
        {
            continue;
        }
        if let Some(owner) = task
            .unit
            .member_nodes
            .iter()
            .find_map(|node| node.referenced_by_mods.iter().max_by_key(|m| m.len()))
        {
            by_owner.entry(owner).or_default().insert(id);
        }
    }

    by_owner
        .into_iter()
        .map(|(owner, mut pending)| {
            let mut target_impl_paths = BTreeSet::new();
            let mut prerequisite_boundary_units = BTreeSet::new();
            let mut ordered_units = Vec::new();
            let mut visited = BTreeSet::new();
            while !pending.is_empty() {
                let next = pending
                    .iter()
                    .copied()
                    .filter(|id| {
                        scheduled[*id]
                            .unit
                            .hard_dep_units
                            .iter()
                            .all(|dep| !pending.contains(dep.as_str()))
                    })
                    .min_by_key(|id| {
                        let task = &scheduled[*id];
                        let hint_waiting = task
                            .unit
                            .soft_pipeline_preds
                            .iter()
                            .any(|dep| pending.contains(dep.as_str()));
                        (hint_waiting, task.wave_index, *id)
                    })
                    .expect("the condensed hard dependency graph is acyclic");
                pending.remove(next);
                let task = &scheduled[next];
                let mut dependencies: Vec<_> = task.unit.hard_dep_units.iter().collect();
                while let Some(dep) = dependencies.pop() {
                    if !visited.insert(dep) {
                        continue;
                    }
                    let dependency = &scheduled[dep].unit;
                    if dependency.is_shared_boundary {
                        prerequisite_boundary_units.insert(dep.clone());
                    }
                    dependencies.extend(&dependency.hard_dep_units);
                }
                target_impl_paths.extend(
                    task.unit
                        .member_nodes
                        .iter()
                        .filter_map(|node| node.impl_path.clone()),
                );
                ordered_units.push(task.clone());
            }
            ParallelTrack {
                track_id: format!("track:{owner}"),
                owner_mod: owner.into(),
                target_impl_paths,
                prerequisite_boundary_units,
                ordered_units,
            }
        })
        .collect()
}
