# surc - Surv IR Compiler & Toolchain

`surc` is a compiler and toolchain for Surv IR (Intermediate Representation), a declarative language for describing system architectures, and Deploy IR for deployment pipelines.

## Quick Start

```bash
# Install
cargo install --path .

# Validate a Surv IR file, project manifest (surv.toml), or Deploy IR file
surc check examples/todo_api.toml
surc check design/surv.toml
surc check examples/deploy.toml

# Inspect a module or project overview
surc inspect mod.todo_api examples/todo_api.toml
surc inspect design/surv.toml

# Extract minimal IR fragment for an edit scope
surc slice mod.todo_api examples/todo_api.toml --with-defs

# Detect drift between IR and implementation
surc diff examples/todo_api.toml .

# Generate Surv IR skeleton from an existing codebase
surc init-ir . --exclude-tests > skeleton.toml

# Visualize architecture
surc export html design/surv.toml > output.html

# Generate CI/CD pipeline
surc codegen github-actions examples/deploy.toml > .github/workflows/deploy.yml
```

## Documentation Index

### Getting Started
- **[Installation & Usage](#installation)** - How to install and use `surc`
- **[Quick Examples](#examples)** - Simple examples to get started

### Specifications
- **[Surv IR Specification v1.1](Spec-Surv-IR-v1.1.md)** - Complete language specification
- **[Surv Behavior IR Specification v0.1 (Draft)](behavior.md)** - State & behavioral specification
- **[Deploy IR Specification v0.1](../Surv%20Deploy%20IR%20Specification%20v0.1.md)** - Deployment pipeline specification

### Guides
- **[Pattern Catalog](docs/PATTERN_CATALOG.md)** - Common architecture patterns with templates
- **[Export Guide](docs/EXPORT_GUIDE.md)** - Visualization options (Mermaid, HTML)
- **[Deploy IR Guide](docs/DEPLOY_IR_GUIDE.md)** - Deployment pipeline guide
- **[Interactive HTML Guide](docs/INTERACTIVE_HTML_GUIDE.md)** - Interactive visualization features

### For LLM/AI Agents
- **[Writing Surv IR](docs/SystemPrompt-Surv-IR-Author.md)** - System prompt for LLMs
- **[Pattern Examples](examples/patterns/)** - Copy-paste ready patterns

### Development Guides
- **[IR-Driven Development](docs/IR_DRIVEN_DEVELOPMENT.md)** - Keep IR and code in sync
- **[Implementation Planning](docs/IMPLEMENTATION_PLANNING.md)** - Generate plans from IR, track progress
- **[Deploy IR for Implementation](docs/DEPLOY_IR_FOR_IMPLEMENTATION.md)** - Use Deploy IR concepts for implementation planning

### Examples
- **[Surv IR Examples](examples/)** - Complete working examples
  - `todo_api.toml` - Simple REST API
  - `user_api.toml` - User CRUD API
  - `auth.toml` - Authentication module
- **[Pattern Templates](examples/patterns/)** - Reusable patterns
  - `rest_api.toml` - REST API pattern
  - `database.toml` - Repository pattern
  - `event_driven.toml` - Event processing
  - `data_pipeline.toml` - ETL pipeline
- **[Deploy Examples](examples/)** - CI/CD examples
  - `deploy.toml` - Multi-stage deployment

---

## Installation

### From source
```bash
git clone <repo>
cd surc
cargo install --path .
```

### Verify installation
```bash
surc --help
```

---

## Command Reference

| Command | Description |
| :--- | :--- |
| `surc check <path> [--ast]` | Validate a single Surv IR file, project manifest (`surv.toml`), or Deploy IR file |
| `surc inspect [target] <path>` | Inspect a module (`schemas`, `funcs`, `pipeline`, `status`) or project/file overview |
| `surc slice <target> <path>` | Extract the minimal IR fragment for a target (`--with-defs`, `--closure`) |
| `surc refs <target> <path>` | List all references to a `schema.*`, `func.*`, or `mod.*` symbol |
| `surc trace <target> <path>` | Trace upstream/downstream pipeline & module data flow |
| `surc diff <ir> <workspace>` | Detect drift between Surv IR and implementation (alias: `diff-impl`) |
| `surc init-ir <workspace>` | Generate a Surv IR design skeleton from source code (`surc skeleton`) |
| `surc skeleton code <ir>` | Generate implementation code stubs (`rust` / `ts`) from Surv IR |
| `surc export <type> <path>` | Export Mermaid diagrams or interactive D3.js HTML |
| `surc codegen <platform> <file>` | Generate CI/CD configuration (`github-actions`, `gitlab-ci`) |
| `surc status <subcommand>` | Manage implementation status inside Surv IR files |

---

## Usage

### 1. Validation & AST Dump (`surc check`)
`surc check` automatically detects whether the input is a single Surv IR file, a multi-package `surv.toml` manifest, or a Deploy IR file:

```bash
# Check a single Surv IR file
surc check examples/todo_api.toml

# Check an entire multi-package project
surc check design/surv.toml

# Check a Deploy IR pipeline file
surc check examples/deploy.toml

# Output parsed AST as JSON
surc check examples/todo_api.toml --ast
```

### 2. Inspection & Scoping (`inspect`, `slice`, `refs`, `trace`)
```bash
# Inspect a module (argument order is flexible; works with single files or surv.toml)
surc inspect mod.todo_api examples/todo_api.toml
surc inspect todo_api examples/todo_api.toml

# Inspect project packages or single-file summary
surc inspect design/surv.toml
surc inspect examples/todo_api.toml

# Extract minimal IR fragment for a target
surc slice mod.todo_api examples/todo_api.toml
surc slice mod.todo_api examples/todo_api.toml --with-defs --closure

# List all references to a symbol
surc refs schema.Todo examples/todo_api.toml
surc refs schema.Todo examples/todo_api.toml --kind func

# Trace pipeline flow (upstream/downstream)
surc trace func.createTodo examples/todo_api.toml --both
surc trace mod.todo_api examples/todo_api.toml
```

### 3. Implementation Drift & Skeleton Generation (`diff`, `init-ir`, `skeleton`)
```bash
# Detect drift between IR and implementation (Rust, TypeScript, C, Zig)
surc diff examples/todo_api.toml .
surc diff design/surv.toml . --mod ui_workspace_pane --lang ts
surc diff design.toml . --format json

# Generate Surv IR design skeleton from an existing codebase (Code -> IR)
surc init-ir . --exclude-tests > design_skeleton.toml
surc skeleton design . --lang rust --exclude-tests

# Generate implementation stubs from Surv IR (IR -> Code)
surc skeleton code examples/todo_api.toml --lang rust
surc skeleton code examples/todo_api.toml ./src --lang ts
```

### 4. Visualization (`surc export`)
```bash
# Export module pipeline or detailed module graph as Mermaid
surc export pipeline examples/todo_api.toml todo_api
surc export module-detail examples/todo_api.toml todo_api

# Export module dependency graph or schema relationship graph (works on surv.toml or single file)
surc export modules design/surv.toml
surc export schemas examples/todo_api.toml

# Export interactive HTML visualization (works on surv.toml, single IR file, or deploy.toml)
surc export html design/surv.toml > viz.html
surc export html examples/todo_api.toml > todo_viz.html
surc export html examples/deploy.toml > deploy_viz.html

# Export Deploy IR pipeline as Mermaid
surc export deploy-mermaid examples/deploy.toml
```

### 5. CI/CD Code Generation (`surc codegen`)
```bash
# Generate GitHub Actions workflow
surc codegen github-actions examples/deploy.toml > .github/workflows/deploy.yml

# Generate GitLab CI configuration
surc codegen gitlab-ci examples/deploy.toml > .gitlab-ci.yml
```

### 6. Status Management (`surc status`)
```bash
# Initialize status section for all modules
surc status init examples/todo_api.toml

# Sync status section (add missing modules as 'todo')
surc status sync examples/todo_api.toml

# Update module status
surc status set mod.todo_api examples/todo_api.toml --state partial --coverage 0.6 --notes "create/get done"

# List all modules with status
surc status list examples/todo_api.toml

# Show detailed status for a module
surc status show mod.todo_api examples/todo_api.toml
```

Status states:
- `todo` - Not started (☐)
- `skeleton` - Types and boundaries only (◯)
- `partial` - Partially implemented (◐)
- `done` - Fully implemented (✓)
- `blocked` - Blocked, needs redesign (✗)

---

## Project Structure

### Single File Projects

For simple projects, use a single Surv IR file:
```bash
surc check api.toml
surc inspect mod.my_module api.toml
surc slice mod.my_module api.toml --with-defs
surc export html api.toml > viz.html
```

### Multi-Package Projects

For larger projects, create a `surv.toml` manifest:

```toml
[project]
name = "my-project"

[paths]
ir_root = "design"

[packages.backend]
root = "design/backend"
namespace = "app.backend"
depends = []

[packages.frontend]
root = "design/frontend"
namespace = "app.frontend"
depends = ["backend"]
```

Then run the same commands against `surv.toml`:
```bash
surc check surv.toml
surc inspect surv.toml
surc diff surv.toml .
surc export modules surv.toml
surc export html surv.toml > viz.html
```

---

## Features

### ✅ Surv IR
- **Unified Validation**: Single command (`surc check`) for files, multi-package projects, and Deploy IR
- **Structural Querying**: `inspect`, `slice`, `refs`, and `trace` for navigating and extracting IR scopes
- **Implementation Sync**: `surc diff` and `surc init-ir` / `surc skeleton` for bidirectional IR ↔ Code workflows
- **Status Tracking**: Track implementation state per module directly in IR files
- **Visualization**: Mermaid diagrams and interactive D3.js graphs

### ✅ Deploy IR
- **Security Checks**: Secret scope, production gates, rollback validation
- **DAG Analysis**: Cycle detection, reachability analysis
- **Code Generation**: GitHub Actions, GitLab CI
- **Visualization**: Interactive pipeline graphs

---

## Common Mistakes

❌ **Using `[schema.Book]` instead of `schema.Book` in references**
```toml
output = ["[schema.Book]"]  # Wrong
output = ["schema.Book"]     # Correct
```

❌ **Missing schema/func prefix**
```toml
pipeline = ["createUser"]           # Wrong
pipeline = ["func.createUser"]      # Correct
```

### Getting Help

```bash
surc --help                    # General help
surc check --help              # Check command help
surc inspect --help            # Inspect command help
surc slice --help              # Slice command help
surc diff --help               # Diff command help
surc skeleton --help           # Skeleton / init-ir help
surc export --help             # Export subcommand help
```
