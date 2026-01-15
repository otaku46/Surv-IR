# surc - Surv IR Compiler & Toolchain

`surc` is a compiler and toolchain for Surv IR (Intermediate Representation), a declarative language for describing system architectures, and Deploy IR for deployment pipelines.

## Quick Start

```bash
# Install
cargo install --path .

# Validate a Surv IR file
surc check examples/todo_api.toml

# Inspect a module
surc inspect mod.todo_api examples/todo_api.toml

# Visualize architecture
surc export html surv.toml > output.html

# Generate CI/CD pipeline
surc codegen github-actions deploy.toml > .github/workflows/deploy.yml
```

## Documentation Index

### Getting Started
- **[Installation & Usage](#installation)** - How to install and use surc
- **[Quick Examples](#examples)** - Simple examples to get started

### Specifications
- **[Surv IR Specification v1.1](Spec-Surv-IR-v1.1.md)** - Complete language specification
- **[Deploy IR Specification v0.1](../Surv%20Deploy%20IR%20Specification%20v0.1.md)** - Deployment pipeline specification

### Guides
- **[Pattern Catalog](PATTERN_CATALOG.md)** - 8 common patterns with templates (REST API, Database, Events, etc.)
- **[Export Guide](EXPORT_GUIDE.md)** - Visualization options (Mermaid, HTML)
- **[Deploy IR Guide](DEPLOY_IR_GUIDE.md)** - Deployment pipeline guide
- **[Interactive HTML Guide](INTERACTIVE_HTML_GUIDE.md)** - Interactive visualization features

### For LLM/AI Agents
- **[Writing Surv IR](SystemPrompt-Surv-IR-Author.md)** - System prompt for LLMs
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

## Usage

### Surv IR Commands

#### Validation
```bash
# Check a single file
surc check api.toml

# Check entire project
surc project-check surv.toml
```

#### Inspection
```bash
# Inspect a module's schemas, functions, and pipeline
surc inspect mod.todo_api examples/todo_api.toml

# Works with or without "mod." prefix
surc inspect todo_api examples/todo_api.toml

# Shows implementation status if [status] section exists
surc inspect mod.todo_api examples/todo_with_status.toml
```

#### Dependency Analysis
```bash
# Show package dependencies
surc deps surv.toml

# Show modules in a package
surc deps surv.toml --package backend

# Show dependencies for a module
surc deps surv.toml --module mod.user_api

# Show only cross-package dependencies
surc deps surv.toml --cross-package

# Export as Mermaid diagram
surc deps surv.toml --format mermaid > deps.md
```

#### Status Management
```bash
# Initialize status section for all modules
surc status init examples/todo_api.toml

# Sync status section (add missing modules as 'todo')
surc status sync examples/todo_api.toml

# Update module status
surc status set mod.todo_api examples/todo_api.toml --state partial
surc status set mod.todo_api examples/todo_api.toml --coverage 0.6 --notes "create/get done"

# List all modules with status
surc status list examples/todo_api.toml

# Show detailed status for a module
surc status show mod.todo_api examples/todo_api.toml
```

#### Visualization
```bash
# Export module pipeline
surc export pipeline api.toml user_api

# Export module dependencies (requires surv.toml)
surc export modules surv.toml

# Export schema relationships (requires surv.toml)
surc export schemas surv.toml

# Export interactive HTML (requires surv.toml)
surc export html surv.toml > viz.html
```

#### Parsing
```bash
# Output AST as JSON
surc parse api.toml
```

### Deploy IR Commands

#### Validation
```bash
# Check deployment pipeline
surc deploy-check deploy.toml
```

#### Visualization
```bash
# Export as Mermaid diagram
surc export deploy-mermaid deploy.toml

# Export as interactive HTML
surc export deploy-html deploy.toml > pipeline.html
```

#### Code Generation
```bash
# Generate GitHub Actions workflow
surc codegen github-actions deploy.toml > .github/workflows/deploy.yml

# Generate GitLab CI configuration
surc codegen gitlab-ci deploy.toml > .gitlab-ci.yml
```

---

## Examples

### Example 1: Simple Todo API

Create `todo.toml`:
```toml
[schema.Todo]
kind = "node"
role = "entity"
fields = {id = "uuid", title = "string", completed = "bool"}

[func.createTodo]
intent = "Create new todo"
input = ["schema.TodoRequest"]
output = ["schema.Todo"]

[mod.todo_api]
purpose = "Todo REST API"
schemas = ["schema.Todo"]
funcs = ["func.createTodo"]
boundary = {http = ["POST /todos"]}
pipeline = ["func.createTodo"]
```

Validate and inspect:
```bash
surc check todo.toml
surc inspect mod.todo_api todo.toml
surc export pipeline todo.toml todo_api
```

### Example 2: Using Patterns

Copy a pattern and customize:
```bash
cp examples/patterns/rest_api.toml my_api.toml
# Edit my_api.toml to match your domain
surc check my_api.toml
```

### Example 3: Tracking Implementation Status

Track implementation progress directly in your Surv IR files:

```bash
# Initialize status for all modules
surc status init my_api.toml

# After adding new modules, sync status section
surc status sync my_api.toml

# Update as you implement
surc status set mod.todo_api my_api.toml --state partial --coverage 0.6 --notes "create/get done"

# View progress
surc status list my_api.toml
# Output:
#   mod.todo_api             ◐ partial     60%  create/get done
#   Last updated: 2026-01-14

# View detailed status
surc status show mod.todo_api my_api.toml
# Output:
#   Module: mod.todo_api
#   Purpose: Todo API
#   Status:
#     State: partial
#     Coverage: 60%
#     Notes: create/get done
#     Updated: 2026-01-14
```

Status states:
- `todo` - Not started (☐)
- `skeleton` - Types and boundaries only (◯)
- `partial` - Partially implemented (◐)
- `done` - Fully implemented (✓)
- `blocked` - Blocked, needs redesign (✗)

### Example 4: Deploy Pipeline

Create `deploy.toml`:
```toml
[deploy.pipeline]
name = "my-app"

[deploy.job.build]
requires = []
runs = ["npm ci", "npm run build"]

[deploy.job.deploy]
requires = ["job.build"]
runs = ["kubectl apply -f deploy.yaml"]
uses_target = "target.prod"
```

Generate CI/CD:
```bash
surc deploy-check deploy.toml
surc codegen github-actions deploy.toml > .github/workflows/deploy.yml
```

---

## Project Structure

### Single File Projects

For simple projects, use a single Surv IR file:
```bash
surc check api.toml
surc inspect mod.my_module api.toml
surc export pipeline api.toml my_module
```

### Multi-File Projects

For larger projects, create a `surv.toml` manifest:

```toml
[project]
name = "my-project"

[files]
"api/user.toml" = {}
"api/auth.toml" = {}
"core/types.toml" = {}
```

Then use project-level commands:
```bash
surc project-check surv.toml
surc export modules surv.toml
surc export html surv.toml > viz.html
```

---

## Features

### ✅ Surv IR
- **Validation**: Static analysis with helpful diagnostics
- **Inspection**: View module schemas, functions, and pipeline
- **Status Tracking**: Track implementation state per module in IR files
- **Visualization**: Mermaid diagrams and interactive D3.js graphs
- **Pattern Library**: 8 common patterns ready to use
- **Multi-project**: Support for large codebases with manifests

### ✅ Deploy IR
- **Security Checks**: Secret scope, production gates, rollback validation
- **DAG Analysis**: Cycle detection, reachability analysis
- **Code Generation**: GitHub Actions, GitLab CI
- **Visualization**: Interactive pipeline graphs

---

## Tips

### Writing Surv IR

1. **Start with patterns**: Browse `examples/patterns/` for templates
2. **Validate early**: Run `surc check` after each section
3. **Inspect often**: Use `surc inspect` to see module structure
4. **Visualize often**: Use `surc export pipeline` to see the flow
5. **Use LLMs**: See `PATTERN_CATALOG.md` for LLM-friendly prompts

### Common Mistakes

❌ **Using `[schema.Book]` instead of `schema.Book` in references**
```toml
output = ["[schema.Book]"]  # Wrong
output = ["schema.Book"]     # Correct
```

❌ **Multiline arrays in module definitions**
```toml
schemas = [
    "schema.A",
    "schema.B"
]  # May not parse correctly

schemas = ["schema.A", "schema.B"]  # Correct
```

❌ **Missing schema/func prefix**
```toml
pipeline = ["createUser"]           # Wrong
pipeline = ["func.createUser"]      # Correct
```

### Getting Help

```bash
surc --help                    # General help
surc export --help             # Export subcommand help
surc check --help              # Check command help
surc inspect --help            # Inspect command help
```

See also:
- [Pattern Catalog](PATTERN_CATALOG.md) for design patterns
- [Specification](Spec-Surv-IR-v1.1.md) for complete syntax
- [Examples](examples/) for working code

---

## Contributing

Found a bug or have a feature request? Please open an issue!

---

## License

[Your license here]
