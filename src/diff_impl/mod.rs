mod lsp_client;
mod matcher;
pub mod reporter;
mod static_analysis;
mod types;

pub use matcher::diff_impl;
pub use types::{DiffResult, ExpectedSymbol, FoundSymbol, SymbolKind};
