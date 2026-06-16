# diff-impl vNext: Bidirectional IR/Code Sync Plan

## Summary

`diff-impl` will evolve from a drift detector into the synchronization surface between Surv IR and implementation code. The core design is a mapping layer that connects IR symbols to Rust/TypeScript symbols so both directions stay smooth:

- `IR -> Code`: generate implementation skeletons or patches for missing symbols.
- `Code -> IR`: generate reviewable IR skeletons from existing implementation facts.
- `IR <-> Code`: compare through stable mapping entries rather than fragile name-only matching.

Rust/TypeScript physical modules and Surv IR design modules are not forced to match. The toolchain will preserve implementation-derived facts as leaf modules and allow higher-level design modules to group them through `submods`.

## Public Interfaces

Extend internal symbol models:

- `FoundSymbol`
  - Add `language`, `relative_path`, `module_path`, `impl_path`, `visibility`, `is_test`, `is_method`.
  - Keep `container_name` and `signature`.
  - Use `name + language + relative_path + range + container_name` as the default identity for deduplication.

- `ExpectedSymbol`
  - Add `stable_id`, `surv_path`, `module_refs`.
  - Keep `impl_bind`, `impl_lang`, `impl_path`, `input`, `output`.

Add mapping types:

```rust
pub struct MappingFile {
    pub version: String,
    pub entries: Vec<MappingEntry>,
}

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
```

Default mapping path: `.surv/mapping.toml`.

Mapping precedence:

1. Explicit mapping file entry.
2. `impl.path` / `impl.bind` in IR.
3. Inferred match by symbol name, kind, language, module path, and signature.

## CLI Changes

Keep the existing command shape:

```bash
surc diff-impl <design.toml|surv.toml> <workspace_root> [options]
```

Add options:

- `--mapping <file>`: read/write mapping entries.
- `--format design-skeleton`: emit `[schema.*]`, `[func.*]`, `[mod.*]`, and mapping facts.
- `--group-by file-module|crate-module|mapping`: choose Code->IR module grouping.
- `--exclude-tests`: omit test functions from skeleton and extra-symbol reporting.
- `--dedup name|path|none`: control skeleton duplicate handling.
- `--emit schemas,funcs,mods,mapping`: select emitted skeleton sections.

Keep existing output formats: `text`, `json`, `md`, `gha`, `skeleton`.

Future command split:

```bash
surc infer-design <workspace_root> --lang rust --out design.generated.toml
```

For vNext, implement this behavior inside `diff-impl --format design-skeleton` first.

## Implementation Phases

### Phase 1: Symbol Facts

Strengthen `static_analysis.rs`:

- Extract Rust free functions, inherent impl methods, trait methods, structs, enums, traits, and type aliases.
- Extract TypeScript function declarations, arrow functions, class methods, interfaces, type aliases, enums, and classes.
- Infer `module_path` from workspace-relative file paths.
- Infer `impl_path` from `module_path + container_name + symbol_name`.
- Mark `is_test` for Rust `#[test]`, `#[cfg(test)]`, and obvious `test_*` functions.
- Preserve visibility when available.
- Deduplicate identical query captures by `language + relative_path + range + name + container_name`.

Acceptance:

- `~/Prog/Comp` style crates no longer produce duplicate entries for the same trait/impl capture at the same range.
- `fmt`, `new`, and `run` remain representable as distinct implementation facts when they come from different modules or containers.

### Phase 2: Mapping Layer

Add `mapping.rs` under `src/diff_impl/`:

- Load and save `.surv/mapping.toml`.
- Build inferred mapping candidates with confidence scores.
- Resolve an expected IR symbol to implementation candidates using mapping-first matching.
- Emit unmatched implementation symbols with enough data to update the mapping.

Matching rules:

- Exact `stable_id` match wins.
- Exact `impl_path` match beats name-only match.
- Name-only match is ambiguous if multiple candidates remain.
- Signature compatibility increases confidence but does not replace mapping identity.

Acceptance:

- `func.runtime.run` and `func.main.run` can map to different Rust `run` functions.
- Missing, extra, ambiguous, and signature mismatch categories remain stable.

### Phase 3: Design Skeleton

Replace current function-only skeleton with `design-skeleton`:

- Emit `[schema.*]` for structs, enums, interfaces, type aliases, and classes.
- Emit `[func.*]` for functions and methods.
- Emit leaf `[mod.*]` sections from implementation module paths.
- Emit root `[mod.<crate_or_package>]` with `submods`.
- Add `impl.bind`, `impl.lang`, and `impl.path` for generated symbols.

Default naming:

- Free function: `func.<module_path>.<name>`.
- Method: `func.<module_path>.<container>.<name>`.
- Schema: `schema.<type_name>`.
- Leaf module: `mod.<module_path>`.

Example:

```toml
[func.engine.Evaluator.evaluate]
input = ["schema.ComputeId", "schema.ExecutionTrace"]
output = ["schema.Result_EvalResult_EvalError"]
impl.bind = "evaluate"
impl.lang = "rust"
impl.path = "engine::Evaluator::evaluate"

[mod.engine]
purpose = "TODO: describe implementation module engine"
funcs = ["func.engine.Evaluator.evaluate"]
schemas = ["schema.EvalResult", "schema.EvalError"]

[mod.comp]
purpose = "TODO: describe crate-level design"
submods = ["engine", "runtime", "backend"]
```

Acceptance:

- Running against a Rust crate produces parseable Surv IR.
- Generated module sections contain funcs/schemas from their implementation module.
- `--exclude-tests` removes test-only functions.
- `--dedup path` keeps distinct same-name functions in different modules.

### Phase 4: IR to Code Skeletons

Add code skeleton output for missing IR symbols:

- Generate Rust/TS function stubs from `[func.*]`.
- Generate Rust `struct`/`enum` or TS `interface` skeletons from `[schema.*]`.
- Use mapping or `impl.path` to choose target module path.
- Do not write source files in vNext; output patch-like text or files under a generated output directory only when explicitly requested later.

Acceptance:

- Missing function with `impl.path = "engine::evaluate"` generates a Rust stub under the inferred `engine` module.
- Missing schema with fields generates a struct/interface skeleton.
- Existing source files are never modified by `diff-impl`.

### Phase 5: Deep Diff

Extend comparison beyond existence:

- Compare function input/output schema names against normalized parameter/return types.
- Compare schema fields against Rust struct fields and TS interface/type fields.
- Normalize common wrappers: `Option<T>`, `Result<T,E>`, `Vec<T>`, `Box<T>`, `Arc<T>`, `Promise<T>`.
- Report schema field mismatch separately from signature mismatch.

Acceptance:

- Wrapper types do not cause false mismatches when the wrapped schema is correct.
- Field additions/removals/type changes are reported in JSON and text output.

### Phase 6: Reporting and CI

Update reports:

- JSON includes mapping confidence, module path, test status, dedup reason, and normalized types.
- Text/Markdown prioritize actionable drift.
- GitHub Actions annotations point to implementation files for code-side drift and IR files for missing expected symbols when location data exists.
- Drift threshold can be computed globally and per module.

Acceptance:

- Existing `text`, `json`, `md`, `gha`, and `skeleton` outputs remain valid.
- New `design-skeleton` output is deterministic.
- CI can fail only when drift exceeds `--threshold`.

## Test Plan

Add Rust fixtures covering:

- Free functions.
- Inherent impl methods.
- Trait methods and trait impls.
- Duplicate method names across modules.
- Common names such as `new`, `fmt`, `run`.
- Nested modules and `mod.rs`.
- `#[test]`, `#[cfg(test)]`, and `test_*` exclusion.
- Structs, enums, type aliases, and wrapper types.

Add TypeScript fixtures covering:

- Function declarations.
- Arrow functions assigned to variables.
- Class methods.
- Interfaces, type aliases, enums, and classes.

Add CLI snapshot tests for:

- `--format json`.
- `--format design-skeleton`.
- `--exclude-tests`.
- `--dedup path`.
- `--mapping .surv/mapping.toml`.
- `--emit schemas,funcs,mods,mapping`.

Regression checks:

- Existing `cargo test` passes.
- Existing `test/test_simple.toml` still produces no false positive drift.
- A large Rust crate fixture produces fewer duplicates than the current skeleton output.

## Assumptions and Defaults

- `diff-impl` remains the implementation namespace in vNext; a separate `infer-design` command can be added after behavior stabilizes.
- Generated design skeletons are reviewable drafts, not authoritative design by themselves.
- The toolchain should not force Rust/TS file layout to match Surv IR design modules.
- Source code mutation is out of scope for vNext.
- `.surv/mapping.toml` is generated and maintained by tooling, but remains human-readable for review.
