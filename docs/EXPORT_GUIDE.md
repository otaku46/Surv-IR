# Surv IR Export Guide

This guide demonstrates how to use the export functionality to visualize Surv IR definitions as Mermaid diagrams.

## Installation

Build the project:
```bash
cargo build --release
```

The binary will be available at `target/release/surc`.

## Export Commands

### 1. Pipeline Visualization

Export a module's pipeline to show function execution flow:

```bash
surc export pipeline <file.toml> <module-name>
```

**Example:**
```bash
surc export pipeline examples/user_api.toml user_http_api
```

**Output:**
```mermaid
---
title: Pipeline - user_http_api
---
flowchart LR
    f0["create_user<br/><small>Construct User from CreateUserRequest</small>"]
    f1["save_user<br/><small>Persist User to DB and update users_snapshot</small>"]
    f0 -->|user| f1

    classDef error fill:#ffdddd,stroke:#ff0000
```

### 2. Schema Graph

Export all schemas and their relationships:

```bash
surc export schemas <surv.toml>
```

**Example:**
```bash
surc export schemas examples/surv.toml
```

**Output:**
```mermaid
---
title: Schema Graph
---
flowchart TD
    schema_create_user_req["create_user_req<br/><small>node/query</small>"]:::node
    schema_user["user<br/><small>node/data</small>"]:::node
    schema_users_snapshot["users_snapshot<br/><small>boundary/context</small>"]:::boundary
    schema_users_snapshot -.-> schema_user

    classDef node fill:#d4e6f1,stroke:#2980b9
    classDef edge fill:#d5f4e6,stroke:#27ae60
    classDef boundary fill:#fdeaa8,stroke:#f39c12
    classDef space fill:#e8daef,stroke:#8e44ad
```

### 3. Module Dependencies

Export module dependency graph showing `require` relationships:

```bash
surc export modules <surv.toml>
```

**Example:**
```bash
surc export modules examples/surv.toml
```

**Output:**
```mermaid
---
title: Module Dependencies
---
flowchart TD
    mod_auth["auth"]
    mod_user_http_api["user_http_api"]
    mod_user_http_api --> mod_auth

    classDef error fill:#ffdddd,stroke:#ff0000
```

### 4. Module Detail View

Export a detailed view of a single module showing all schemas and functions:

```bash
surc export module-detail <file.toml> <module-name>
```

**Example:**
```bash
surc export module-detail examples/user_api.toml user_http_api
```

**Output:**
```mermaid
---
title: Module - user_http_api
---
flowchart TD
    MOD[["user_http_api"]]
    schema_user["schema: user"]:::schema
    MOD -.-> schema_user
    schema_create_user_req["schema: create_user_req"]:::schema
    MOD -.-> schema_create_user_req
    schema_users_snapshot["schema: users_snapshot"]:::schema
    MOD -.-> schema_users_snapshot
    func_create_user["func: create_user"]:::func
    MOD --> func_create_user
    func_save_user["func: save_user"]:::func
    MOD --> func_save_user
    func_get_user["func: get_user"]:::func
    MOD --> func_get_user

    classDef schema fill:#d4e6f1,stroke:#2980b9
    classDef func fill:#d5f4e6,stroke:#27ae60
    classDef error fill:#ffdddd,stroke:#ff0000
```

## Viewing Diagrams

### Option 1: Mermaid Live Editor
1. Copy the output
2. Visit https://mermaid.live/
3. Paste the diagram code

### Option 2: GitHub/GitLab
Save the output in a Markdown file with triple backticks:

````markdown
```mermaid
---
title: Pipeline - user_http_api
---
flowchart LR
    f0["create_user"]
    f1["save_user"]
    f0 --> f1
```
````

GitHub and GitLab will render it automatically.

### Option 3: VSCode Extension
Install the "Markdown Preview Mermaid Support" extension and view `.md` files with diagrams.

## Output to File

Redirect output to a file:

```bash
surc export pipeline examples/user_api.toml user_http_api > docs/pipeline.md
```

## Error Visualization

The exporter highlights errors with red styling:
- **Undefined functions** in pipelines: marked with ⚠
- **Missing schemas**: shown in red
- **Type mismatches**: displayed as dotted lines with warnings

## Color Coding

### Schema Types
- **Node** (blue): `#d4e6f1` / `#2980b9`
- **Edge** (green): `#d5f4e6` / `#27ae60`
- **Boundary** (yellow): `#fdeaa8` / `#f39c12`
- **Space** (purple): `#e8daef` / `#8e44ad`

### Module Elements
- **Schemas**: Blue
- **Functions**: Green
- **Errors**: Red

## Tips

1. **Pipeline validation**: Pipeline exports show schema compatibility between adjacent functions
2. **Cycle detection**: Module dependency graphs highlight circular dependencies
3. **Documentation generation**: Use exports as part of CI/CD to auto-generate architecture docs
4. **Quick debugging**: Export to quickly visualize system structure during development

## Example Workflow

```bash
# 1. Check the IR file is valid
surc check examples/user_api.toml

# 2. Export pipeline for documentation
surc export pipeline examples/user_api.toml user_http_api > docs/pipelines/user_api.md

# 3. Export full project schema graph
surc export schemas examples/surv.toml > docs/schemas.md

# 4. Export module dependencies
surc export modules examples/surv.toml > docs/architecture.md
```
