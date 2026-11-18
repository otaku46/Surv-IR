mod lsp_client;
mod mapping;
mod matcher;
pub mod reporter;
mod static_analysis;
mod types;

pub use mapping::{find_by_surv_ref, load_mapping};
pub use matcher::{diff_impl, extract_ir_for_code_skeleton, scan_workspace_for_skeleton};
pub use types::{
    DedupMode, DesignSkeletonOptions, DiffResult, ExpectedSymbol, FoundSymbol, MappingEntry,
    MappingFile, SymbolKind,
};
