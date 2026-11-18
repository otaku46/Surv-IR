# Surv IR / surc Toolchain Improvement Roadmap

## Current State

Surv IR already has the foundations of a useful design toolchain:

- Core IR sections: `schema`, `func`, `mod`, `status`, imports, requires, packages.
- Validation: single-file `check` and project-level `project-check`.
- Navigation: `inspect`, `slice`, `refs`, `trace`, `deps`.
- Visualization: Mermaid and HTML export.
- Project operations: `split`, package assignment, deploy IR checks, CI codegen.
- Implementation sync: `diff-impl` with static analysis, signature comparison, JSON/Markdown/GitHub Actions output, threshold, and skeleton output.

The main gap is not feature count. The gap is making the whole system stable, predictable, and smooth enough that IR and code can evolve together without manual bookkeeping.

## High Priority

### 1. Mapping Layer for `diff-impl`

Introduce `.surv/mapping.toml` to connect IR symbols to implementation symbols.

Purpose:

- Avoid fragile name-only matching.
- Support same-name functions in different modules.
- Enable IR->Code and Code->IR workflows through the same identity layer.

Completion criteria:

- `diff-impl` resolves symbols through mapping before fallback matching.
- Mapping entries include confidence when inferred.
- JSON output shows which mapping rule matched each symbol.

### 2. Code-to-IR `design-skeleton`

Extend skeleton generation from function-only output to full design skeleton output.

Output:

- `[schema.*]` from structs/enums/interfaces/type aliases.
- `[func.*]` from functions/methods.
- `[mod.*]` from implementation module paths.
- Root module with `submods`.
- Optional mapping entries.

Completion criteria:

- Output is parseable Surv IR.
- Test functions can be excluded.
- Duplicate functions are controlled by path-aware deduplication.

### 3. Schema Field Comparison

Compare IR schema fields with implementation types.

Scope:

- Rust `struct` fields.
- TypeScript `interface` and object type fields.
- Field missing/extra/type mismatch categories.

Completion criteria:

- `diff-impl` reports schema field drift separately from signature drift.
- JSON output is stable enough for CI.

### 4. Project-Level `diff-impl`

Allow `diff-impl` to accept `surv.toml` manifests, not only single design files.

Purpose:

- Check multi-package IR projects.
- Resolve package namespace and `require` relationships.
- Compute module-level drift rates.

Completion criteria:

- `surc diff-impl surv.toml <workspace>` works.
- `--mod` can target project modules.
- Output groups drift by package/module.

### 5. Diagnostics and Error IDs

Make diagnostics stable and explainable.

Changes:

- Assign diagnostic IDs such as `E_SCHEMA_MISSING`, `E_FUNC_AMBIGUOUS`, `E_IMPL_SIGNATURE_MISMATCH`.
- Use consistent severity levels.
- Include location, related symbol, and suggested next action where available.

Completion criteria:

- Human-readable output and JSON output share diagnostic IDs.
- Existing checker diagnostics and `diff-impl` diagnostics use the same style.

### 6. Formatter: `surc fmt`

Add a canonical formatter for Surv IR.

Purpose:

- Reduce churn from generated skeletons.
- Make round-trip Code->IR output reviewable.
- Stabilize tests and snapshots.

Completion criteria:

- Formats section order consistently.
- Formats arrays, inline fields, and nested pipeline structures.
- Does not alter semantic content.

## Medium Priority

### 1. `surc explain <diagnostic-id>`

Provide targeted explanations and examples for diagnostics.

Completion criteria:

- Common checker and `diff-impl` errors have explanations.
- Output includes short fix examples.

### 2. `surc watch`

Continuously run checks while IR or code changes.

Completion criteria:

- Watches IR files and implementation files.
- Debounces changes.
- Reports only changed diagnostics when possible.

### 3. Output Consistency

Unify output behavior across `inspect`, `slice`, `refs`, `trace`, `deps`, and `diff-impl`.

Completion criteria:

- Common `--format text|json|md` behavior.
- Stable JSON shapes for machine consumption.
- Consistent exit code rules.

### 4. Status Integration

Connect `status` with `diff-impl`.

Purpose:

- Update module implementation status from drift data.
- Compute coverage and drift rate per module.
- Show stale modules in status output.

Completion criteria:

- `surc status` can include drift summary.
- `diff-impl` can emit status-update suggestions without mutating files.

### 5. Stable Machine-Readable Schema

Define JSON schemas for command outputs.

Completion criteria:

- `diff-impl --format json` has a documented schema.
- `check` and `project-check` outputs can be consumed by tooling.

## Long Term

### 1. IR-to-Code Patch Generation

Generate implementation patches from missing IR symbols.

Constraints:

- No source mutation unless explicitly requested.
- Patches must be reviewable.
- Mapping chooses target files/modules.

### 2. Rename Detection

Detect likely renames instead of reporting delete/add pairs.

Signals:

- Stable mapping ID.
- Similar signature.
- Same module path.
- Similar schema usage.

### 3. Generated Layer and Overlay Layer

Separate machine-generated implementation facts from human design intent.

Model:

- `design.generated.toml`: regenerated from code.
- `design.overlay.toml`: user-authored grouping and intent.
- Project loader merges both.

### 4. Editor / LSP Integration

Expose Surv IR relationships in editors.

Capabilities:

- Jump from `[func.*]` to implementation.
- Show drift inline.
- Complete schema and function references.
- Preview module dependency graph.

### 5. Unified Graph View

Combine design graph and implementation graph.

Purpose:

- Show IR modules, schemas, functions, implementation files, and drift in one graph.
- Help users see where design and code diverge.

### 6. CI Templates

Generate CI configuration that includes Surv checks.

Capabilities:

- `surc check`.
- `surc project-check`.
- `surc diff-impl --threshold`.
- Artifact upload for Markdown/HTML reports.

## Non-Goals for Now

- Do not reimplement full Rust or TypeScript type checking.
- Do not force implementation file layout to match Surv IR module layout.
- Do not automatically rewrite source code as part of `diff-impl`.
- Do not require humans to manually maintain large mapping files.
- Do not treat generated skeletons as authoritative design without review.
- Do not prioritize broad language support before Rust and TypeScript are solid.

## Suggested Order

1. Implement `diff-impl` mapping layer.
2. Add `design-skeleton` output with schemas, funcs, mods, and root submods.
3. Add path-aware deduplication and test exclusion.
4. Add schema field extraction and comparison.
5. Add project-level `diff-impl`.
6. Add `surc fmt`.
7. Standardize diagnostics and output schemas.
