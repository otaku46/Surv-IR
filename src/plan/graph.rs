use super::loader::LoadedPlanContext;
use crate::ast::{PipelineStep, Section};
use crate::diff_impl::{ExpectedSymbol, SymbolKind as ImplKind};
use crate::symbol::{check_package_dependency, reference_candidates};
use crate::{Diagnostic, FileImportContext, Manifest, ProjectAST, SymbolKind, SymbolTable};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

#[derive(Clone, Serialize)]
pub struct PlanNode {
    pub fq_name: String,
    pub display_ref: String,
    pub node_kind: String,
    pub package: String,
    pub namespace: Option<String>,
    pub source_file: PathBuf,
    pub impl_bind: Option<String>,
    pub impl_lang: Option<String>,
    pub impl_path: Option<String>,
    pub referenced_by_mods: BTreeSet<String>,
    pub expected: Option<ExpectedSymbol>,
    pub manual_state: Option<String>,
    pub local_name: String,
}

#[derive(Clone, Serialize)]
pub struct PlanDependencyEdge {
    pub from_fqn: String,
    pub to_fqn: String,
    pub edge_kind: String,
    pub is_hard_blocker: bool,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub enum EdgeKind {
    SchemaRef,
    FuncIoSchema,
    ModMember,
    ModHierarchy,
    ModRequire,
    PipelineOrder,
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SchemaRef => "schema_ref",
            Self::FuncIoSchema => "func_io_schema",
            Self::ModMember => "mod_member",
            Self::ModHierarchy => "mod_hierarchy",
            Self::ModRequire => "mod_require",
            Self::PipelineOrder => "pipeline_order",
        }
    }
}

#[derive(Default, Serialize)]
pub struct PlanDependencyGraph {
    pub nodes: BTreeMap<String, PlanNode>,
    pub edges: Vec<PlanDependencyEdge>,
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip)]
    pub(super) edge_map: BTreeMap<String, BTreeSet<(String, EdgeKind)>>,
}

impl PlanDependencyGraph {
    fn edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        if self
            .edge_map
            .entry(from.into())
            .or_default()
            .insert((to.into(), kind))
        {
            self.edges.push(PlanDependencyEdge {
                from_fqn: from.to_string(),
                to_fqn: to.to_string(),
                edge_kind: kind.as_str().to_string(),
                is_hard_blocker: kind != EdgeKind::PipelineOrder,
            });
        }
    }

    pub fn dependencies(&self, id: &str) -> impl Iterator<Item = (&str, EdgeKind)> {
        self.edge_map
            .get(id)
            .into_iter()
            .flatten()
            .map(|(to, kind)| (to.as_str(), *kind))
    }

    pub fn scope(&self, module: Option<&str>) -> Result<BTreeSet<String>, String> {
        let Some(module) = module else {
            return Ok(self.nodes.keys().cloned().collect());
        };
        let matches: Vec<_> = self
            .nodes
            .values()
            .filter(|node| {
                node.node_kind == "mod"
                    && (node.fq_name == module
                        || node.display_ref == module
                        || node.local_name == module
                        || format!("{}.{}", node.package, node.display_ref) == module
                        || node
                            .namespace
                            .as_ref()
                            .is_some_and(|ns| format!("{ns}.{}", node.display_ref) == module))
            })
            .collect();
        let [root] = matches.as_slice() else {
            return Err(if matches.is_empty() {
                format!("Unknown module '{module}'")
            } else {
                format!(
                    "Ambiguous module '{module}'. Use one of: {}",
                    matches
                        .iter()
                        .map(|node| node.fq_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
        };
        let mut selected = BTreeSet::new();
        let mut pending = vec![root.fq_name.as_str()];
        while let Some(id) = pending.pop() {
            if selected.insert(id.to_string()) {
                pending.extend(
                    self.dependencies(id)
                        .filter_map(|(to, kind)| (kind != EdgeKind::PipelineOrder).then_some(to)),
                );
            }
        }
        Ok(selected)
    }
}

pub fn build_plan_dependency_graph(context: &LoadedPlanContext) -> PlanDependencyGraph {
    let mut diagnostics = Vec::new();
    let Some(project) = &context.project else {
        return PlanDependencyGraph::default();
    };
    let Some(symbols) = &context.symbol_table else {
        return PlanDependencyGraph::default();
    };

    let graph = build_plan_dependency_graph_inner(
        project,
        context.manifest.as_ref(),
        symbols,
        &context.import_contexts,
        &mut diagnostics,
    );
    graph
}

pub(super) fn build_plan_dependency_graph_inner(
    project: &ProjectAST,
    manifest: Option<&Manifest>,
    symbols: &SymbolTable,
    contexts: &[FileImportContext],
    diagnostics: &mut Vec<Diagnostic>,
) -> PlanDependencyGraph {
    let sections: HashMap<_, _> = project
        .files
        .iter()
        .flat_map(|(path, file)| {
            file.sections.iter().filter_map(move |section| {
                let (kind, name) = match section {
                    Section::Schema(value) => (SymbolKind::Schema, &value.name),
                    Section::Func(value) => (SymbolKind::Func, &value.name),
                    Section::Mod(value) => (SymbolKind::Mod, &value.name),
                    _ => return None,
                };
                Some(((path, kind, name), (file, section)))
            })
        })
        .collect();
    let mut graph = PlanDependencyGraph::default();
    for entry in &symbols.entries {
        let (file, section) = sections[&(&entry.file, entry.kind, &entry.local_name)];
        let (node_kind, expected, manual_state) = match section {
            Section::Schema(schema) => (
                "schema".to_string(),
                Some(ExpectedSymbol {
                    stable_id: Some(entry.fq_name.clone()),
                    surv_name: schema.name.clone(),
                    surv_path: Some(format!("schema.{}", schema.name)),
                    impl_bind: schema.impl_bind.clone(),
                    impl_lang: schema.impl_lang.clone(),
                    impl_path: schema.impl_path.clone(),
                    module_refs: Vec::new(),
                    kind: ImplKind::Schema,
                    input: Vec::new(),
                    output: Vec::new(),
                    fields: schema.fields.clone(),
                }),
                None,
            ),
            Section::Func(func) => (
                "func".to_string(),
                Some(ExpectedSymbol {
                    stable_id: Some(entry.fq_name.clone()),
                    surv_name: func.name.clone(),
                    surv_path: Some(format!("func.{}", func.name)),
                    impl_bind: func.impl_bind.clone(),
                    impl_lang: func.impl_lang.clone(),
                    impl_path: func.impl_path.clone(),
                    module_refs: Vec::new(),
                    kind: ImplKind::Func,
                    input: func.input.clone(),
                    output: func.output.clone(),
                    fields: BTreeMap::new(),
                }),
                None,
            ),
            Section::Mod(module) => (
                "mod".to_string(),
                None,
                file.sections.iter().find_map(|section| {
                    if let Section::Status(status) = section {
                        status
                            .modules
                            .get(&module.name)
                            .map(|status| status.state.clone())
                    } else {
                        None
                    }
                }),
            ),
            _ => unreachable!("only symbol table definitions are indexed"),
        };
        let previous = graph.nodes.insert(
            entry.fq_name.clone(),
            PlanNode {
                fq_name: entry.fq_name.clone(),
                display_ref: format!("{node_kind}.{}", entry.local_name),
                node_kind,
                package: entry.package.clone(),
                namespace: entry.namespace.clone(),
                source_file: entry.file.clone(),
                impl_bind: expected.as_ref().and_then(|item| item.impl_bind.clone()),
                impl_lang: expected.as_ref().and_then(|item| item.impl_lang.clone()),
                impl_path: expected.as_ref().and_then(|item| item.impl_path.clone()),
                expected,
                manual_state,
                local_name: entry.local_name.clone(),
                referenced_by_mods: BTreeSet::new(),
            },
        );
        if previous.is_some() {
            diagnostics.push(super::error(
                "DuplicatePlanSymbol",
                format!("Duplicate symbol {}", entry.fq_name),
                &entry.file,
            ));
        }
    }
    let context_by_file: HashMap<_, _> = contexts.iter().map(|ctx| (&ctx.file_path, ctx)).collect();

    for entry in &symbols.entries {
        let (file, section) = sections[&(&entry.file, entry.kind, &entry.local_name)];
        let context = context_by_file[&entry.file];
        let from = &entry.fq_name;
        let mut resolve = |kind, reference: &str, required: bool| {
            if reference.is_empty() {
                return None;
            }
            let candidates = reference_candidates(symbols, kind, reference, context);
            match candidates.as_slice() {
                [target] => {
                    diagnostics.extend(check_package_dependency(
                        manifest,
                        context,
                        &target.package,
                    ));
                    Some(target.fq_name.as_str())
                }
                [] if !required => None,
                [] => {
                    diagnostics.push(super::error(
                        "UndefinedPlanReference",
                        format!("{from}: undefined reference '{reference}'"),
                        &entry.file,
                    ));
                    None
                }
                _ => {
                    diagnostics.push(super::error(
                        "AmbiguousPlanReference",
                        format!("{from}: ambiguous reference '{reference}'"),
                        &entry.file,
                    ));
                    None
                }
            }
        };
        match section {
            Section::Schema(schema) => {
                for reference in [&schema.from, &schema.to, &schema.base]
                    .into_iter()
                    .chain(&schema.over)
                {
                    if let Some(to) = resolve(
                        SymbolKind::Schema,
                        reference,
                        explicit_schema_ref(reference),
                    ) {
                        if to != from {
                            graph.edge(from, to, EdgeKind::SchemaRef);
                        }
                    }
                }
                for ty in schema.fields.values() {
                    for reference in ty
                        .split(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '.' | '_' | '-')))
                    {
                        if primitive_type(reference) {
                            continue;
                        }
                        if let Some(to) = resolve(
                            SymbolKind::Schema,
                            reference,
                            explicit_schema_ref(reference),
                        ) {
                            if to != from {
                                graph.edge(from, to, EdgeKind::SchemaRef);
                            }
                        }
                    }
                }
            }
            Section::Func(func) => {
                for reference in func.input.iter().chain(&func.output) {
                    if let Some(to) = resolve(SymbolKind::Schema, reference, true) {
                        graph.edge(from, to, EdgeKind::FuncIoSchema);
                    }
                }
            }
            Section::Mod(module) => {
                for (kind, values) in [
                    (SymbolKind::Schema, &module.schemas),
                    (SymbolKind::Func, &module.funcs),
                ] {
                    for reference in values {
                        if let Some(to) = resolve(kind, reference, true) {
                            graph.edge(from, to, EdgeKind::ModMember);
                        }
                    }
                }
                for child in &module.submods {
                    if let Some(to) = resolve(
                        SymbolKind::Mod,
                        &format!("mod.{}.{child}", module.name),
                        true,
                    ) {
                        graph.edge(from, to, EdgeKind::ModHierarchy);
                    }
                }
                for requirement in &file.requires {
                    if let Some(to) = resolve(SymbolKind::Mod, &requirement.target, true) {
                        graph.edge(from, to, EdgeKind::ModRequire);
                    }
                }
                for (after, before) in pipeline_edges(&module.pipeline) {
                    if let (Some(after), Some(before)) = (
                        resolve(SymbolKind::Func, after, true),
                        resolve(SymbolKind::Func, before, true),
                    ) {
                        if after != before {
                            graph.edge(after, before, EdgeKind::PipelineOrder);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut owners = Vec::new();
    for module in graph.nodes.values().filter(|node| node.node_kind == "mod") {
        let mut pending: Vec<_> = graph
            .dependencies(&module.fq_name)
            .filter_map(|(to, kind)| (kind == EdgeKind::ModMember).then_some(to))
            .collect();
        let mut seen = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if seen.insert(id) {
                owners.push((id.to_string(), module.fq_name.clone()));
                pending.extend(graph.dependencies(id).filter_map(|(to, kind)| {
                    matches!(kind, EdgeKind::SchemaRef | EdgeKind::FuncIoSchema).then_some(to)
                }));
            }
        }
    }
    for (id, module) in owners {
        graph
            .nodes
            .get_mut(&id)
            .unwrap()
            .referenced_by_mods
            .insert(module);
    }
    graph.diagnostics = std::mem::take(diagnostics);
    graph
}

fn explicit_schema_ref(name: &str) -> bool {
    name.starts_with("schema.") || name.contains(".schema.")
}

fn primitive_type(name: &str) -> bool {
    matches!(
        name,
        "" | "String"
            | "str"
            | "string"
            | "int"
            | "float"
            | "bool"
            | "uuid"
            | "timestamp"
            | "bytes"
            | "any"
            | "PathBuf"
            | "Path"
            | "Vec"
            | "Option"
            | "Box"
            | "HashMap"
            | "BTreeMap"
            | "HashSet"
            | "BTreeSet"
            | "Result"
            | "usize"
            | "isize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "f32"
            | "f64"
    )
}

fn pipeline_edges(steps: &[PipelineStep]) -> BTreeSet<(&str, &str)> {
    fn walk<'a>(
        steps: &'a [PipelineStep],
        mut previous: BTreeSet<&'a str>,
        edges: &mut BTreeSet<(&'a str, &'a str)>,
    ) -> BTreeSet<&'a str> {
        for step in steps {
            previous = match step {
                PipelineStep::Call(after) => {
                    edges.extend(previous.iter().map(|before| (after.as_str(), *before)));
                    BTreeSet::from([after.as_str()])
                }
                PipelineStep::Sequential(inner) => walk(inner, previous, edges),
                PipelineStep::Parallel(inner) if !inner.is_empty() => inner
                    .iter()
                    .flat_map(|step| walk(std::slice::from_ref(step), previous.clone(), edges))
                    .collect(),
                PipelineStep::Branch {
                    on_true, on_false, ..
                } => [on_true.as_ref(), on_false.as_ref()]
                    .into_iter()
                    .flat_map(|step| walk(std::slice::from_ref(step), previous.clone(), edges))
                    .collect(),
                PipelineStep::Parallel(_) => previous,
            };
        }
        previous
    }
    let mut edges = BTreeSet::new();
    walk(steps, BTreeSet::new(), &mut edges);
    edges
}
