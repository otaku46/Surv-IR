use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Symbol kind in Surv IR
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Func,
    Schema,
}

/// Expected symbol from Surv IR
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedSymbol {
    /// Stable symbol id used by future mapping files.
    pub stable_id: Option<String>,

    /// Original name in Surv IR (e.g., "createUser")
    pub surv_name: String,

    /// Full Surv reference path (e.g., "func.runtime.run").
    pub surv_path: Option<String>,

    /// Optional implementation binding name (from impl.bind)
    /// If present, this is the name to search for in code
    pub impl_bind: Option<String>,

    /// Optional language constraint (from impl.lang)
    /// Values: "ts", "rust", "either", or None (both)
    pub impl_lang: Option<String>,

    /// Optional namespace/container path (from impl.path)
    /// e.g., "commands::file" or "utils.fileOps"
    pub impl_path: Option<String>,

    /// Modules that reference this symbol.
    pub module_refs: Vec<String>,

    /// Symbol kind (Func or Schema)
    pub kind: SymbolKind,

    /// Expected input schema refs for functions.
    pub input: Vec<String>,

    /// Expected output schema refs for functions.
    pub output: Vec<String>,

    /// Expected fields for schema symbols.
    pub fields: BTreeMap<String, String>,
}

impl ExpectedSymbol {
    /// Get the name to search for in code
    pub fn search_name(&self) -> &str {
        self.impl_bind.as_deref().unwrap_or(&self.surv_name)
    }

    /// Check if this symbol can be implemented in the given language
    pub fn matches_language(&self, lang: &str) -> bool {
        match &self.impl_lang {
            None => true, // No constraint, both languages OK
            Some(constraint) => constraint == lang || constraint == "either",
        }
    }
}

/// Symbol found in codebase via LSP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundSymbol {
    /// Implementation language ("rust", "ts", etc.).
    pub language: Option<String>,

    /// Symbol name
    pub name: String,

    /// LSP symbol kind string (e.g., "Function", "Interface", "Struct")
    pub kind: String,

    /// File URI where symbol is defined
    pub uri: String,

    /// Workspace-relative source path when known.
    pub relative_path: Option<String>,

    /// Implementation module path inferred from source layout.
    pub module_path: Option<String>,

    /// Fully qualified implementation path when known.
    pub impl_path: Option<String>,

    /// Line and column range
    pub range: SymbolRange,

    /// Container name (e.g., module, class)
    pub container_name: Option<String>,

    /// Visibility such as "pub" when statically known.
    pub visibility: Option<String>,

    /// Whether this symbol appears to be test-only.
    pub is_test: bool,

    /// Whether this symbol is a method rather than a free function.
    pub is_method: bool,

    /// Additional details from LSP
    pub detail: Option<String>,

    /// Function signature extracted by static analysis when available.
    pub signature: Option<FunctionSignature>,

    /// Struct/interface fields extracted by static analysis when available.
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRange {
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionSignature {
    pub parameters: Vec<ParameterSignature>,
    pub return_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterSignature {
    pub name: Option<String>,
    pub type_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignatureMismatch {
    pub expected: ExpectedSymbol,
    pub found: FoundSymbol,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFieldMismatch {
    pub expected: ExpectedSymbol,
    pub found: FoundSymbol,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingFile {
    pub version: String,
    pub entries: Vec<MappingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingEntry {
    pub stable_id: String,
    pub surv_ref: String,
    pub impl_lang: String,
    pub impl_path: String,
    pub source_file: String,
    pub symbol_name: String,
    pub container: Option<String>,
    pub kind: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupMode {
    Name,
    Path,
    None,
}

impl DedupMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "name" => Some(Self::Name),
            "path" => Some(Self::Path),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DesignSkeletonOptions {
    pub exclude_tests: bool,
    pub dedup: DedupMode,
    pub emit_schemas: bool,
    pub emit_funcs: bool,
    pub emit_mods: bool,
    pub emit_mapping: bool,
}

impl Default for DesignSkeletonOptions {
    fn default() -> Self {
        Self {
            exclude_tests: false,
            dedup: DedupMode::Path,
            emit_schemas: true,
            emit_funcs: true,
            emit_mods: true,
            emit_mapping: false,
        }
    }
}

/// Type of drift detected
#[derive(Debug, Serialize)]
pub enum DriftKind {
    /// Symbol expected in IR but not found in code
    Missing { expected: ExpectedSymbol },

    /// Symbol found in code but not in IR
    Extra { found: FoundSymbol },

    /// Multiple candidates found for expected symbol
    Ambiguous {
        expected: ExpectedSymbol,
        candidates: Vec<FoundSymbol>,
    },
}

/// Result of diff-impl analysis
#[derive(Debug, Default, Serialize)]
pub struct DiffResult {
    /// Symbols defined in IR but missing from code
    pub missing: Vec<ExpectedSymbol>,

    /// Symbols in code but not in IR
    pub extra: Vec<FoundSymbol>,

    /// Symbols with ambiguous matches (multiple candidates)
    pub ambiguous: Vec<(ExpectedSymbol, Vec<FoundSymbol>)>,

    /// Symbols that exist but whose implementation signature differs from IR.
    pub signature_mismatches: Vec<SignatureMismatch>,

    /// Schemas that exist but whose fields differ from implementation.
    pub schema_field_mismatches: Vec<SchemaFieldMismatch>,

    /// Symbols successfully matched (for statistics)
    pub matched: usize,
}

impl DiffResult {
    pub fn has_issues(&self) -> bool {
        !self.missing.is_empty()
            || !self.extra.is_empty()
            || !self.ambiguous.is_empty()
            || !self.signature_mismatches.is_empty()
            || !self.schema_field_mismatches.is_empty()
    }

    pub fn issue_count(&self) -> usize {
        self.missing.len()
            + self.extra.len()
            + self.ambiguous.len()
            + self.signature_mismatches.len()
            + self.schema_field_mismatches.len()
    }

    pub fn drift_rate(&self) -> f64 {
        let total = self.total_expected().max(self.total_found()).max(1);
        self.issue_count() as f64 / total as f64
    }

    pub fn total_expected(&self) -> usize {
        self.missing.len() + self.ambiguous.len() + self.matched
    }

    pub fn total_found(&self) -> usize {
        self.extra.len() + self.matched
    }
}
