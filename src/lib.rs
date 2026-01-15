pub mod ast;
pub mod checker;
pub mod codegen;
pub mod deploy;
pub mod diagnostic;
pub mod diff_impl;
pub mod export;
pub mod imports;
pub mod loader;
pub mod manifest;
pub mod package;
pub mod project;
pub mod project_checker;
mod simple_toml;
pub mod symbol;

pub mod parser;

pub use ast::*;
pub use checker::{check_surv_ast, check_surv_file};
pub use deploy::{check_deploy_file, parse_deploy_file};
pub use diagnostic::Diagnostic;
pub use export::{HtmlExporter, MermaidExporter};
pub use imports::{parse_imports_with_alias, FileImportContext, ImportEntry};
pub use loader::load_project;
pub use manifest::Manifest;
pub use package::{assign_packages_to_files, PackageAssignment};
pub use parser::{parse_file, parse_surv_file, parse_surv_ir};
pub use project::{ModRef, NormalizedRequire, ProjectAST};
pub use project_checker::check_project;
pub use symbol::{
    build_symbol_table, build_symbol_table_with_packages, resolve_schema_and_func_references,
    SymbolEntry, SymbolKind, SymbolTable,
};
