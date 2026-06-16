use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize)]
pub struct SurvFile {
    pub package: Option<String>,
    pub namespace: Option<String>,
    pub imports: Vec<ImportDecl>,
    pub requires: Vec<RequireDecl>,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportDecl {
    pub target: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequireDecl {
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum Section {
    Meta(MetaSection),
    Schema(SchemaSection),
    Func(FuncSection),
    Mod(ModSection),
    Status(StatusSection),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MetaSection {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SchemaSection {
    pub name: String,
    pub kind: String,
    pub role: String,
    pub r#type: String,
    pub from: String,
    pub to: String,
    pub base: String,
    pub label: String,
    pub fields: BTreeMap<String, String>,
    pub over: Vec<String>,

    // Implementation metadata for diff-impl
    pub impl_bind: Option<String>,
    pub impl_lang: Option<String>,
    pub impl_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FuncSection {
    pub name: String,
    pub intent: String,
    pub input: Vec<String>,
    pub output: Vec<String>,
    pub design_notes: String,

    // Implementation metadata for diff-impl
    pub impl_bind: Option<String>,
    pub impl_lang: Option<String>,
    pub impl_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum PipelineStep {
    Call(String),
    Sequential(Vec<PipelineStep>),
    Parallel(Vec<PipelineStep>),
    Branch {
        condition: String,
        on_true: Box<PipelineStep>,
        on_false: Box<PipelineStep>,
    },
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ModSection {
    pub name: String,
    pub purpose: String,
    pub schemas: Vec<String>,
    pub funcs: Vec<String>,
    pub pipeline: Vec<PipelineStep>,
    pub submods: Vec<String>,
}

impl ModSection {
    pub fn get_pipeline_calls(&self) -> Vec<String> {
        let mut calls = Vec::new();
        for step in &self.pipeline {
            step.collect_calls(&mut calls);
        }
        calls
    }
}

impl PipelineStep {
    pub fn collect_calls(&self, out: &mut Vec<String>) {
        match self {
            PipelineStep::Call(func) => out.push(func.clone()),
            PipelineStep::Sequential(inner) | PipelineStep::Parallel(inner) => {
                for step in inner {
                    step.collect_calls(out);
                }
            }
            PipelineStep::Branch {
                on_true, on_false, ..
            } => {
                on_true.collect_calls(out);
                on_false.collect_calls(out);
            }
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StatusSection {
    pub name: String,
    pub updated_at: String,
    pub modules: BTreeMap<String, ModuleStatus>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ModuleStatus {
    pub state: String,
    pub coverage: f64,
    pub notes: String,
}
