use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: String,
    pub kind: String,
    pub message: String,
    pub location: String,
}
