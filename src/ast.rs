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

#[derive(Debug, Clone, Default, Serialize)]
pub struct ModSection {
    pub name: String,
    pub purpose: String,
    pub schemas: Vec<String>,
    pub funcs: Vec<String>,
    pub pipeline: Vec<String>,
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
