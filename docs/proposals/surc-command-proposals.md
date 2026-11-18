surc Command Proposals (Draft)
==============================

Goals
-----
- Make it easy to extract the minimal IR needed for an edit.
- Reduce the need to split files just for navigation.
- Keep outputs script-friendly and stable.

Non-Goals
---------
- Replace `surc inspect` (keep it as the human-readable overview command).
- Add full graph visualization (already covered by `export`).

Scope
-----
The commands below operate on a single IR file or a project manifest.
Targets are fully-qualified references: `schema.*`, `func.*`, `mod.*`.
When using a project manifest, `package.*` targets are allowed for manifest-aware commands.


Command: `surc slice`
---------------------
Purpose: Emit the smallest IR fragment needed to work on a target.

Syntax:
- `surc slice <target> <file>`
- Optional flags:
  - `--include schemas,funcs,mods` (default: all)
  - `--with-defs` (include definitions, not only names; implies `--format toml`)
  - `--closure` (include transitive dependencies)
  - `--mod <mod>` (required when target is `func.*` and appears in multiple modules)
  - `--format list|toml|json` (default: list)

Behavior:
- For `mod.*`, include its `schemas`, `funcs`, and `pipeline` items.
- For `func.*`, include its input/output schemas and pipeline adjacency.
  Pipeline adjacency is resolved within `--mod` context when a function appears in multiple modules.
  If the target appears in multiple modules and `--mod` is missing, return an error listing candidates.
  If the target appears in exactly one module, use it implicitly.
  If `--mod` is provided and the target is not in that module, return an error.
  If the target is not `func.*` and `--mod` is provided, return an error.
- For `schema.*`, include direct references (fields -> schema refs).

Example:
- `surc slice mod.ui_chat_pane plasm_ide_design.toml --with-defs --format toml`


Command: `surc refs`
--------------------
Purpose: List all references to a given symbol.

Syntax:
- `surc refs <target> <file>`
- Optional flags:
  - `--format list|json` (default: list)
  - `--kind mod|func|schema` (filter by referrer kind)
  - `--include import,impl,boundary|all` (comma-separated, opt-in extra reference sites)

Behavior:
- Returns any `schema.*`/`func.*`/`mod.*` that references the target
  via `fields`, `input`, `output`, `schemas`, `funcs`, `pipeline`,
  `base`, `from`, `to`, `over`, `type`, or `require`.
  `import`, `impl.*`, and `boundary` are included only when requested via `--include`.
  `--include all` expands to `import,impl,boundary`.
  Only strings matching `schema.*`, `func.*`, or `mod.*` are treated as references.
  Output uses fully-qualified names after namespace/import resolution.

Example:
- `surc refs schema.Diagnostic plasm_ide_design.toml --kind func`


Command: `surc trace`
---------------------
Purpose: Follow data flow across a pipeline chain.

Syntax:
- `surc trace <target> <file>`
- Optional flags:
  - `--up` (upstream only)
  - `--down` (downstream only)
  - `--both` (default)
  - `--mod <mod>` (required when target is `func.*` and appears in multiple modules)
  - `--depth N`
  - `--format list|json` (default: list)

Behavior:
- For `func.*`, traverse the pipeline sequence within the resolved module context.
  If the target appears in multiple modules and `--mod` is missing, return an error listing candidates.
  If the target appears in exactly one module, use it implicitly.
  If `--mod` is provided and the target is not in that module, return an error.
  If the target is not `func.*` and `--mod` is provided, return an error.
- For `mod.*`, traverse its pipeline and adjacent modules via shared schemas.

Example:
- `surc trace func.backend_agent_run plasm_ide_design.toml --up --depth 3`


Output Conventions
------------------
- `list`: one item per line, prefixed by kind (e.g., `func.backend_chat_send`).
- `json`: stable keys for tooling; include `kind`, `name`, and `source`.
- `toml`: for `slice --with-defs`, emit a valid IR fragment with headers.


Compatibility Notes
-------------------
- `surc inspect` remains the default overview for a module.
- `surc slice` is the recommended "edit scope" command.
