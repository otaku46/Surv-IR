# Agent Guide for Surv IR Development

This document provides guidance for AI agents (Claude, ChatGPT, etc.) working with the Surv IR toolchain.

---

## Quick Start

### What is Surv IR?

Surv IR is a declarative language for describing system architectures:
- **Schemas**: Data structures (types, entities, events)
- **Functions**: Transformations and operations
- **Modules**: Compositions of schemas and functions

### Basic Workflow

1. **Validate**: `surc check <file.toml>`
2. **Inspect**: `surc inspect <module> <file.toml>`
3. **Visualize**: `surc export pipeline <file.toml> <module>`
4. **Track Progress**: `surc status list <file.toml>`

---

## Common Tasks

### Task 1: Creating a New Surv IR File

```toml
# Start with schemas
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string", email = "string"}

# Add functions
[func.createUser]
intent = "Create a new user"
input = ["schema.UserRequest"]
output = ["schema.User"]

# Compose into modules
[mod.user_api]
purpose = "User management API"
schemas = ["schema.User"]
funcs = ["func.createUser"]
pipeline = ["func.createUser"]
boundary = {http = ["POST /users"]}
```

**Validation:**
```bash
surc check user_api.toml
```

### Task 2: Adding Implementation Metadata

When you implement code based on IR:

```toml
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string"}
impl.bind = "UserModel"      # Actual type name in code
impl.lang = "rust"            # Language constraint
impl.path = "models::user"   # Module path

[func.createUser]
intent = "Create user"
input = ["schema.UserRequest"]
output = ["schema.User"]
impl.bind = "create_user"    # Actual function name
impl.lang = "rust"
impl.path = "services::user"
```

### Task 3: Tracking Implementation Status

```bash
# Initialize status tracking
surc status init design.toml

# Mark module as partially implemented
surc status set mod.user_api design.toml --state partial --coverage 0.6 --notes "create/get done"

# View progress
surc status list design.toml
```

Status states:
- `todo`: Not started
- `skeleton`: Types defined, no logic
- `partial`: Partially implemented
- `done`: Fully implemented
- `blocked`: Blocked, needs redesign

### Task 4: Inspecting a Module

```bash
surc inspect mod.user_api design.toml
```

Shows:
- Purpose
- Schemas used
- Functions defined
- Pipeline (execution flow)
- Implementation status (if tracked)

### Task 5: Visualizing Architecture

```bash
# Single module pipeline
surc export pipeline design.toml user_api

# Project-wide (requires surv.toml manifest)
surc export modules surv.toml          # Module dependency graph
surc export schemas surv.toml          # Schema relationship graph
surc export html surv.toml > viz.html  # Interactive visualization
```

---

## Important Rules

### 1. Schema Definition Rules

**DO:**
```toml
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string"}
```

**DON'T:**
```toml
# ❌ Wrong: Don't use brackets in references
output = ["[schema.User]"]

# ✅ Correct
output = ["schema.User"]
```

### 2. Reference Format

Always use the format: `<category>.<name>`

```toml
# Correct references
input = ["schema.User"]
output = ["schema.UserResponse"]
pipeline = ["func.validate", "func.create"]

# Fully qualified (with namespace)
input = ["auth.schema.Token"]
```

### 3. Namespace Behavior

When a file declares `namespace = "X"`:
- All local identifiers are **prefixed** with namespace
- `schema.User` → `X.schema.User`

```toml
namespace = "user"

[schema.Profile]
# Resolved as: user.schema.Profile

[func.getProfile]
# Resolved as: user.func.getProfile
```

### 4. Array Formatting

**Use inline arrays** for module definitions:

```toml
# ✅ Correct
schemas = ["schema.User", "schema.Post"]

# ❌ May not parse correctly
schemas = [
    "schema.User",
    "schema.Post"
]
```

### 5. Field Types

Available primitive types:
- `string`, `int`, `float`, `bool`
- `uuid`, `timestamp`
- `schema.OtherSchema` (references)
- `string[]` (arrays)
- `string?` (optional)
- `"option1|option2"` (enums)

---

## Working with Projects

### Single File Projects

For simple projects:
```bash
surc check api.toml
surc inspect mod.user_api api.toml
```

### Multi-File Projects

Create a `surv.toml` manifest:

```toml
[project]
name = "my-project"

[paths]
ir_root = "design"

[packages.backend]
root = "design/api"
namespace = "app.api"

[packages.frontend]
root = "design/ui"
namespace = "app.ui"
depends = ["backend"]
```

Then use project commands:
```bash
surc project-check surv.toml
surc export html surv.toml > viz.html
```

---

## Common Patterns

### Pattern 1: REST API

```toml
[schema.Book]
kind = "node"
role = "entity"
fields = {id = "uuid", title = "string", author = "string"}

[schema.CreateBookRequest]
kind = "node"
role = "request"
fields = {title = "string", author = "string"}

[func.createBook]
intent = "Create a new book"
input = ["schema.CreateBookRequest"]
output = ["schema.Book"]

[mod.book_api]
purpose = "Book REST API"
schemas = ["schema.Book", "schema.CreateBookRequest"]
funcs = ["func.createBook"]
pipeline = ["func.createBook"]
boundary = {http = ["POST /books"]}
```

### Pattern 2: Event-Driven

```toml
[schema.OrderPlaced]
kind = "node"
role = "event"
fields = {order_id = "uuid", timestamp = "timestamp"}

[func.handleOrderPlaced]
intent = "Process order placement"
input = ["schema.OrderPlaced"]
output = ["schema.PaymentRequest"]

[mod.order_handler]
purpose = "Order event processing"
schemas = ["schema.OrderPlaced", "schema.PaymentRequest"]
funcs = ["func.handleOrderPlaced"]
pipeline = ["func.handleOrderPlaced"]
boundary = {events = ["order.placed"]}
```

### Pattern 3: Data Pipeline

```toml
[schema.RawData]
kind = "node"
role = "data"
fields = {content = "string"}

[schema.CleanedData]
kind = "node"
role = "data"
fields = {content = "string", metadata = "object"}

[func.extract]
intent = "Extract data from source"
output = ["schema.RawData"]

[func.transform]
intent = "Clean and transform data"
input = ["schema.RawData"]
output = ["schema.CleanedData"]

[func.load]
intent = "Load into destination"
input = ["schema.CleanedData"]
output = ["schema.LoadResult"]

[mod.etl_pipeline]
purpose = "ETL data pipeline"
pipeline = ["func.extract", "func.transform", "func.load"]
```

---

## Error Messages and Fixes

### Error: "schema X is defined but never referenced"

**Cause:** Schema not used in any module.

**Fix:** Either add to a module or remove if unused:
```toml
[mod.my_module]
schemas = ["schema.X"]  # Add here
```

### Error: "func X is defined but never referenced"

**Fix:** Add to module's `funcs` array:
```toml
[mod.my_module]
funcs = ["func.X"]
```

### Error: "Reference not found: schema.Y"

**Cause:** Typo or missing schema definition.

**Fix:** Check spelling or add the schema:
```toml
[schema.Y]
kind = "node"
role = "entity"
fields = {...}
```

### Error: "Circular dependency detected"

**Cause:** Module depends on itself (directly or indirectly).

**Fix:** Restructure dependencies to break the cycle.

---

## Best Practices

### 1. Start with Schemas

Define your data model first:
```toml
[schema.User]
...
[schema.Post]
...
[schema.Comment]
...
```

### 2. Then Add Functions

Define operations on your data:
```toml
[func.createUser]
...
[func.createPost]
...
```

### 3. Compose into Modules

Group related functionality:
```toml
[mod.user_management]
schemas = [...]
funcs = [...]
pipeline = [...]
```

### 4. Validate Early and Often

Run `surc check` after each major change.

### 5. Use Descriptive Names

```toml
# ✅ Good
[schema.UserRegistrationRequest]
[func.validateEmailAndCreateUser]

# ❌ Bad
[schema.URR]
[func.doStuff]
```

### 6. Document Intent

```toml
[func.processPayment]
intent = "Process payment via Stripe API, handle failures with retry logic"
design_notes = "Uses exponential backoff, max 3 retries"
```

### 7. Track Implementation Progress

```bash
# As you implement, update status
surc status set mod.user_api design.toml --state partial --coverage 0.5
```

---

## Integration with Code

### Workflow 1: Design-First (Recommended)

1. **Design in Surv IR** (this defines the contract)
   ```bash
   surc check design.toml
   surc export html surv.toml > viz.html
   ```

2. **Implement in code** (Rust, TypeScript, etc.)
   ```rust
   // Implement based on IR
   pub struct User { ... }
   pub fn create_user(...) { ... }
   ```

3. **Track progress**
   ```bash
   surc status set mod.user_api design.toml --state partial
   ```

4. **Verify alignment** (future: use `surc diff-impl`)
   ```bash
   # Check drift between IR and code
   surc diff-impl design.toml ./src --mod user_api
   ```

### Workflow 2: Code-First (Reverse Engineering)

1. **Write code first**
2. **Extract to Surv IR** (document the architecture)
3. **Use IR for refactoring** and evolution

---

## Resources

### Documentation
- **Specification**: `Spec-Surv-IR-v1.1.md` - Complete language reference
- **Pattern Catalog**: `PATTERN_CATALOG.md` - Reusable templates
- **Export Guide**: `EXPORT_GUIDE.md` - Visualization options
- **README**: `README.md` - Quick start and examples

### Examples
- `examples/todo_api.toml` - Simple REST API
- `examples/patterns/` - Common patterns
- `examples/deploy.toml` - Deployment pipeline

### CLI Help
```bash
surc --help
surc check --help
surc export --help
surc status --help
surc diff-impl --help
```

---

## Tips for AI Agents

### When Creating Surv IR:

1. **Ask about the domain** before writing IR
   - What entities exist?
   - What operations are needed?
   - What are the boundaries (HTTP, events, etc.)?

2. **Start simple**, then refine
   - Basic schemas first
   - Add functions
   - Group into modules
   - Validate frequently

3. **Use appropriate roles**
   - `entity` for domain objects
   - `request`/`response` for API contracts
   - `event` for messages
   - `context` for application state

4. **Think about pipelines**
   - Show data flow through functions
   - Make execution order clear

5. **Validate before presenting**
   ```bash
   surc check <file.toml>
   ```

### When Implementing from IR:

1. **Read the IR carefully**
   ```bash
   surc inspect mod.target_module design.toml
   ```

2. **Check dependencies**
   - What schemas does this module use?
   - What functions must be implemented?

3. **Follow the pipeline**
   - Pipeline shows intended execution flow
   - Implement functions in pipeline order

4. **Update status as you go**
   ```bash
   surc status set mod.target design.toml --state partial
   ```

### When Updating IR:

1. **Check current state**
   ```bash
   surc inspect mod.existing design.toml
   ```

2. **Make changes incrementally**

3. **Validate after each change**
   ```bash
   surc check design.toml
   ```

4. **Update visualizations**
   ```bash
   surc export pipeline design.toml module_name
   ```

---

## Common Agent Workflows

### Workflow: "Design a new feature"

```bash
# 1. User describes feature
# 2. Agent creates IR file
cat > new_feature.toml << 'EOF'
[schema.Feature]
...
[func.implementFeature]
...
[mod.feature_module]
...
EOF

# 3. Validate
surc check new_feature.toml

# 4. Show to user
surc inspect mod.feature_module new_feature.toml
surc export pipeline new_feature.toml feature_module
```

### Workflow: "Understand existing architecture"

```bash
# 1. Check all modules
surc project-check surv.toml

# 2. List modules
# (Parse output of export or check)

# 3. Inspect specific module
surc inspect mod.interesting_module design.toml

# 4. Visualize relationships
surc export modules surv.toml
surc export schemas surv.toml
```

### Workflow: "Implement from IR"

```bash
# 1. Read module definition
surc inspect mod.user_api design.toml

# 2. Implement in code
# (Generate Rust/TypeScript/etc.)

# 3. Track progress
surc status set mod.user_api design.toml --state done --coverage 1.0

# 4. Verify (future)
surc diff-impl design.toml ./src --mod user_api
```

---

## Gotchas and Edge Cases

### 1. Namespace Confusion

```toml
namespace = "api"

[schema.User]  # This is api.schema.User, not schema.User!
```

**Solution:** Use fully qualified names when referencing across files.

### 2. Multiline Arrays

```toml
# May fail to parse
schemas = [
    "schema.A",
    "schema.B"
]

# Use inline instead
schemas = ["schema.A", "schema.B"]
```

### 3. Missing Prefixes

```toml
# ❌ Wrong
pipeline = ["validateUser", "createUser"]

# ✅ Correct
pipeline = ["func.validateUser", "func.createUser"]
```

### 4. Case Sensitivity

Surv IR is case-sensitive:
```toml
[schema.User]   # Different from [schema.user]
```

---

## Summary Checklist

When working with Surv IR, always:

- ✅ Validate with `surc check`
- ✅ Use correct reference format (`schema.X`, `func.Y`)
- ✅ Keep arrays inline for module definitions
- ✅ Include `intent` for all functions
- ✅ Use appropriate `kind` and `role` for schemas
- ✅ Track implementation status
- ✅ Document with visualizations
- ✅ Follow the specification in `Spec-Surv-IR-v1.1.md`

---

## Need Help?

- Check `Spec-Surv-IR-v1.1.md` for detailed syntax
- Look at `examples/` for working code
- Run `surc <command> --help` for command documentation
- Use `surc check` to validate and get error messages
