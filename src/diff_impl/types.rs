use serde::{Deserialize, Serialize};

/// Symbol kind in Surv IR
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Func,
    Schema,
}

/// Expected symbol from Surv IR
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedSymbol {
    /// Original name in Surv IR (e.g., "createUser")
    pub surv_name: String,

    /// Optional implementation binding name (from impl.bind)
    /// If present, this is the name to search for in code
    pub impl_bind: Option<String>,

    /// Optional language constraint (from impl.lang)
    /// Values: "ts", "rust", "either", or None (both)
    pub impl_lang: Option<String>,

    /// Optional namespace/container path (from impl.path)
    /// e.g., "commands::file" or "utils.fileOps"
    pub impl_path: Option<String>,

    /// Symbol kind (Func or Schema)
    pub kind: SymbolKind,
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
            Some(constraint) => {
                constraint == lang || constraint == "either"
            }
        }
    }
}

/// Symbol found in codebase via LSP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundSymbol {
    /// Symbol name
    pub name: String,

    /// LSP symbol kind string (e.g., "Function", "Interface", "Struct")
    pub kind: String,

    /// File URI where symbol is defined
    pub uri: String,

    /// Line and column range
    pub range: SymbolRange,

    /// Container name (e.g., module, class)
    pub container_name: Option<String>,

    /// Additional details from LSP
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRange {
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
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

    /// Symbols successfully matched (for statistics)
    pub matched: usize,
}

impl DiffResult {
    pub fn has_issues(&self) -> bool {
        !self.missing.is_empty() || !self.extra.is_empty() || !self.ambiguous.is_empty()
    }

    pub fn total_expected(&self) -> usize {
        self.missing.len() + self.ambiguous.len() + self.matched
    }

    pub fn total_found(&self) -> usize {
        self.extra.len() + self.matched
    }
}
