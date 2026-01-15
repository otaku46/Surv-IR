# Surv IR Specification v1.1

**Surv IR** (Intermediate Representation) is a declarative language for describing system architectures, focusing on data schemas, functions, and module compositions.

---

## Table of Contents

1. [Overview](#overview)
2. [File Structure](#file-structure)
3. [Headers](#headers)
4. [Core Constructs](#core-constructs)
   - [Schema](#schema)
   - [Func](#func)
   - [Mod](#mod)
5. [Advanced Features](#advanced-features)
   - [Meta Section](#meta-section)
   - [Import & Require](#import--require)
   - [Status Section](#status-section)
   - [Implementation Metadata](#implementation-metadata)
6. [Project Organization](#project-organization)
   - [Packages](#packages)
   - [Namespaces](#namespaces)
   - [Manifests](#manifests)
7. [Reference Resolution](#reference-resolution)
8. [Examples](#examples)

---

## Overview

Surv IR provides a standardized way to describe:
- **Data structures** (schemas)
- **Transformations** (functions)
- **Compositions** (modules)
- **Implementation metadata** (bindings, language constraints)

**Design Principles:**
- Declarative over imperative
- Language-agnostic architecture description
- Separation of design from implementation
- Tooling-friendly (parseable, validatable, visualizable)

---

## File Structure

A Surv IR file is a TOML document with sections:

```toml
# Optional headers
package = "my_package"
namespace = "api"

# Optional metadata
[meta]
name = "my_design"
version = "0.1.0"

# Schema definitions
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string"}

# Function definitions
[func.createUser]
intent = "Create a new user"
input = ["schema.UserRequest"]
output = ["schema.User"]

# Module definitions
[mod.user_api]
purpose = "User management API"
schemas = ["schema.User"]
funcs = ["func.createUser"]
pipeline = ["func.createUser"]
```

---

## Headers

Headers appear at the top of a file (before any `[section]`):

### `package`
Optional. Declares which package this file belongs to.

```toml
package = "backend"
```

### `namespace`
Optional. **Prefixes all local identifiers** in this file.

```toml
namespace = "user"
```

**Effect:** All schemas/funcs in this file are prefixed with the namespace:
- `schema.Profile` → resolved as `user.schema.Profile`
- `func.authenticate` → resolved as `user.func.authenticate`

**Important:** Namespace is a **prefix**, not a scope separator.

---

## Core Constructs

### Schema

Schemas describe data structures.

#### Syntax

```toml
[schema.SchemaName]
kind = "node" | "edge" | "value"
role = "entity" | "event" | "request" | "response" | "context" | ...
type = "..."           # Optional: for aliases/generics
from = "schema.X"      # Optional: for edges
to = "schema.Y"        # Optional: for edges
base = "schema.Z"      # Optional: inheritance
label = "description"  # Optional
fields = {field1 = "type1", field2 = "type2", ...}
over = ["schema.A", "schema.B"]  # Optional: union types

# Implementation metadata (optional)
impl.bind = "ActualTypeName"
impl.lang = "ts" | "rust" | "either"
impl.path = "module.path"
```

#### Field Types

Types use a simple syntax:
- Primitives: `string`, `int`, `float`, `bool`, `uuid`, `timestamp`
- References: `schema.OtherSchema`
- Arrays: `string[]`, `schema.User[]`
- Optional: `string?`, `schema.User?`
- Union: Use `over` field for schema unions

#### Schema Kinds

- **node**: Standalone entity or object
- **edge**: Relationship between two schemas (requires `from` and `to`)
- **value**: Value type or primitive wrapper

#### Schema Roles

Roles are semantic hints:
- `entity`: Domain object
- `event`: Event/message
- `request`: API request
- `response`: API response
- `context`: Application state
- `diagnostic`: Error/warning
- `report`: Analytics/reporting
- `config`: Configuration

**Roles are advisory**—they guide understanding but don't enforce constraints.

#### Example

```toml
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string", email = "string", created_at = "timestamp"}

[schema.UserCreatedEvent]
kind = "node"
role = "event"
fields = {user_id = "uuid", timestamp = "timestamp"}

[schema.Follows]
kind = "edge"
role = "relationship"
from = "schema.User"
to = "schema.User"
fields = {since = "timestamp"}
```

---

### Func

Functions describe transformations or operations.

#### Syntax

```toml
[func.FunctionName]
intent = "Human-readable description of what this function does"
input = ["schema.Input1", "schema.Input2", ...]
output = ["schema.Output1", "schema.Output2", ...]
design_notes = "Optional implementation notes"

# Implementation metadata (optional)
impl.bind = "actual_function_name"
impl.lang = "ts" | "rust" | "either"
impl.path = "module.path"
```

#### Fields

- **intent**: Required. Describes the purpose.
- **input**: Array of schema references consumed by this function.
- **output**: Array of schema references produced by this function.
- **design_notes**: Optional. Additional design context.

#### Example

```toml
[func.createUser]
intent = "Create a new user account"
input = ["schema.CreateUserRequest"]
output = ["schema.User", "schema.UserCreatedEvent"]
design_notes = "Validates email uniqueness before creation"

[func.getUserById]
intent = "Retrieve user by ID"
input = ["schema.UserId"]
output = ["schema.User"]
```

---

### Mod

Modules compose schemas and functions into coherent units.

#### Syntax

```toml
[mod.ModuleName]
purpose = "Description of this module's responsibility"
schemas = ["schema.A", "schema.B", ...]
funcs = ["func.X", "func.Y", ...]
pipeline = ["func.X", "func.Y", ...]  # Execution flow
boundary = {http = ["POST /users"], events = ["user.created"]}  # Optional
```

#### Fields

- **purpose**: Required. High-level description.
- **schemas**: Schemas owned or used by this module.
- **funcs**: Functions provided by this module.
- **pipeline**: Ordered sequence of functions showing data flow.
- **boundary**: Optional. External interfaces (HTTP endpoints, events, etc.)

#### Pipeline Syntax

Pipeline describes execution flow:

```toml
# Simple sequence
pipeline = ["func.validate", "func.transform", "func.save"]

# Alternative: inline with arrows (parsed as sequence)
pipeline = ["func.validate → func.transform → func.save"]
```

#### Example

```toml
[mod.user_registration]
purpose = "Handle user registration workflow"
schemas = ["schema.RegistrationRequest", "schema.User", "schema.WelcomeEmail"]
funcs = ["func.validateEmail", "func.createUser", "func.sendWelcomeEmail"]
pipeline = ["func.validateEmail", "func.createUser", "func.sendWelcomeEmail"]
boundary = {http = ["POST /register"]}
```

---

## Advanced Features

### Meta Section

Provides file-level metadata.

```toml
[meta]
name = "my_api_design"
version = "0.2.1"
description = "REST API for user management"
```

### Import & Require

#### Import (Name Resolution Aid)

Declares external symbols for easier referencing. **Syntactic convenience only.**

```toml
import = ["other_namespace.schema.User as ExtUser"]
```

**Effect:**
- Allows using `ExtUser` instead of `other_namespace.schema.User`
- Checker validates that imported symbols exist (warns if not found)
- Does NOT affect resolution—just an alias

#### Require (Semantic Dependency)

Declares that this file depends on external modules. **Semantic dependency.**

```toml
require = ["mod.auth", "mod.storage"]
```

**Important Rules:**
1. **Format:** Each element must be `"mod.<name>"` (single module reference)
2. **No chaining:** Don't use `"mod.a → mod.b"` syntax in require
3. **Resolution:** Validated by ProjectChecker across the entire project
4. **Errors:**
   - Unresolved require → error
   - Circular dependencies → error
5. **Expansion:** If all modules in a file share the same requires, ProjectChecker creates dependency edges for each module

**Example:**

```toml
require = ["mod.user_auth", "mod.database"]

[mod.user_api]
# Implicitly depends on mod.user_auth and mod.database

[mod.admin_api]
# Also implicitly depends on mod.user_auth and mod.database
```

---

### Status Section

Track implementation status per module (added in v1.1).

```toml
[status]
updated_at = "2026-01-14"

[status.mod.user_api]
state = "done" | "partial" | "skeleton" | "todo" | "blocked"
coverage = 0.0..1.0
notes = "Implementation notes"
```

#### States

- `todo`: Not started (☐)
- `skeleton`: Types/boundaries defined, no logic (◯)
- `partial`: Partially implemented (◐)
- `done`: Fully implemented (✓)
- `blocked`: Blocked, needs redesign (✗)

#### CLI Commands

```bash
surc status init <file.toml>         # Initialize status section
surc status sync <file.toml>         # Add missing modules as 'todo'
surc status set <mod> <file> --state partial --coverage 0.6
surc status list <file.toml>
surc status show <mod> <file.toml>
```

---

### Implementation Metadata

Surv IR can include hints for code generation and drift detection (added in v1.1).

#### `impl.bind`

Specify actual implementation name if different from IR name.

```toml
[func.createUser]
intent = "Create user"
impl.bind = "backend_create_user"  # Look for this name in code
```

#### `impl.lang`

Specify which language implements this symbol.

```toml
[func.readFile]
impl.lang = "rust"  # Only implemented in Rust

[schema.UIComponent]
impl.lang = "ts"    # Only in TypeScript

[func.sharedUtil]
impl.lang = "either"  # Can be in either language
```

Values: `"ts"`, `"rust"`, `"either"`, or omit for both.

#### `impl.path`

Specify namespace/module path in implementation.

```toml
[func.readFile]
impl.path = "file_ops::io"  # Rust: file_ops::io module

[schema.TreeNode]
impl.path = "ui.workspace.tree"  # TS: ui/workspace/tree
```

#### Use Case: Drift Detection

```bash
surc diff-impl design.toml ./src --mod user_api
```

Compares IR against actual code using LSP, detecting:
- **Missing**: In IR but not in code
- **Ambiguous**: Multiple candidates found
- **Extra**: In code but not in IR

---

## Project Organization

### Packages

Large projects can be split into packages.

#### surv.toml (Project Manifest)

```toml
[project]
name = "my_app"

[paths]
ir_root = "design"

[packages.frontend]
root = "design/ui"
namespace = "app.ui"
depends = ["common"]

[packages.backend]
root = "design/api"
namespace = "app.api"
depends = ["common"]

[packages.common]
root = "design/shared"
namespace = "app.common"
```

#### Package Anatomy

- **root**: Directory containing IR files for this package
- **namespace**: Default namespace for symbols in this package
- **depends**: Other packages this one depends on

### Namespaces

Namespaces prevent naming conflicts.

```toml
# In file under packages.frontend
package = "frontend"
namespace = "app.ui"

[schema.Button]  # Fully qualified: app.ui.Button
kind = "node"
fields = {label = "string"}
```

### Manifests

The manifest (`surv.toml`) is required for:
- Multi-package projects
- Cross-package references
- Project-wide exports (modules graph, HTML visualization)

#### Commands

```bash
surc project-check surv.toml           # Validate entire project
surc export modules surv.toml          # Module dependency graph
surc export schemas surv.toml          # Schema relationship graph
surc export html surv.toml > viz.html  # Interactive visualization
```

---

## Reference Resolution

### Namespace Behavior (Strict Rules)

#### 1. Namespace Prefixing

When a file declares `namespace = "X"`, **all local identifiers are prefixed**:

```toml
namespace = "user"

[schema.Profile]
# Resolved as: user.schema.Profile

[func.authenticate]
# Resolved as: user.func.authenticate
```

#### 2. Import (Syntactic Convenience)

Import declarations make name resolution easier but **do not affect actual resolution**.

```toml
import = ["auth.schema.Token as AuthToken"]

[func.validateToken]
input = ["AuthToken"]  # Same as "auth.schema.Token"
```

**Checker behavior:**
- Validates that imported symbols exist
- Warns if import references non-existent symbols
- Does not prevent resolution—purely for readability

#### 3. Require (Semantic Dependency)

Require declares **module dependencies** that must exist in the project.

```toml
require = ["mod.auth", "mod.storage"]
```

**Resolution:**
- Validated by `ProjectChecker` across entire project
- **Unresolved require** → Error
- **Circular dependencies** → Error
- If all modules in a file share the same requires, ProjectChecker expands this to edges between each module

**Format Rules:**
- Each element must be `"mod.<name>"` (single reference)
- No chaining syntax: ~~`"mod.a → mod.b"`~~ ❌

### Reference Syntax

```toml
# Local reference (in same file)
input = ["schema.User"]

# Namespaced reference (if file has namespace = "api")
# Local schema.User resolves to api.schema.User

# Fully qualified reference (across namespaces)
input = ["auth.schema.Token"]

# Using import alias
import = ["auth.schema.Token as AuthToken"]
input = ["AuthToken"]  # Equivalent to above
```

### Resolution Priority

1. **Exact match**: If reference includes namespace prefix, use that
2. **Local namespace**: If current file has namespace, prefix local references
3. **Cross-namespace**: Requires fully qualified name or import alias

---

## Examples

### Example 1: Simple API

```toml
[schema.Book]
kind = "node"
role = "entity"
fields = {id = "uuid", title = "string", author = "string", isbn = "string"}

[schema.CreateBookRequest]
kind = "node"
role = "request"
fields = {title = "string", author = "string", isbn = "string"}

[func.createBook]
intent = "Create a new book"
input = ["schema.CreateBookRequest"]
output = ["schema.Book"]

[func.getBookById]
intent = "Retrieve book by ID"
input = ["schema.BookId"]
output = ["schema.Book"]

[mod.book_api]
purpose = "Book management REST API"
schemas = ["schema.Book", "schema.CreateBookRequest"]
funcs = ["func.createBook", "func.getBookById"]
pipeline = ["func.createBook"]
boundary = {http = ["POST /books", "GET /books/:id"]}
```

### Example 2: Event-Driven System

```toml
[schema.Order]
kind = "node"
role = "entity"
fields = {id = "uuid", items = "OrderItem[]", total = "float"}

[schema.OrderPlaced]
kind = "node"
role = "event"
fields = {order_id = "uuid", timestamp = "timestamp"}

[schema.PaymentProcessed]
kind = "node"
role = "event"
fields = {order_id = "uuid", amount = "float"}

[func.placeOrder]
intent = "Place a new order"
input = ["schema.Order"]
output = ["schema.OrderPlaced"]

[func.processPayment]
intent = "Process payment for order"
input = ["schema.OrderPlaced"]
output = ["schema.PaymentProcessed"]

[mod.order_workflow]
purpose = "Order processing pipeline"
schemas = ["schema.Order", "schema.OrderPlaced", "schema.PaymentProcessed"]
funcs = ["func.placeOrder", "func.processPayment"]
pipeline = ["func.placeOrder", "func.processPayment"]
boundary = {events = ["order.placed", "payment.processed"]}
```

### Example 3: Multi-Package Project

```toml
# surv.toml
[project]
name = "ecommerce"

[paths]
ir_root = "ir"

[packages.catalog]
root = "ir/catalog"
namespace = "ecom.catalog"

[packages.orders]
root = "ir/orders"
namespace = "ecom.orders"
depends = ["catalog"]

# ir/catalog/products.toml
package = "catalog"
namespace = "ecom.catalog"

[schema.Product]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string", price = "float"}

# ir/orders/orders.toml
package = "orders"
namespace = "ecom.orders"
require = ["catalog"]

[schema.OrderItem]
kind = "node"
role = "value"
fields = {product = "ecom.catalog.schema.Product", quantity = "int"}

[schema.Order]
kind = "node"
role = "entity"
fields = {id = "uuid", items = "OrderItem[]"}
```

### Example 4: With Implementation Metadata

```toml
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string"}
impl.bind = "UserModel"
impl.lang = "rust"
impl.path = "models::user"

[func.authenticate]
intent = "Authenticate user credentials"
input = ["schema.Credentials"]
output = ["schema.AuthToken"]
impl.bind = "auth_service::authenticate"
impl.lang = "rust"

[func.renderUserProfile]
intent = "Render user profile UI"
input = ["schema.User"]
output = ["schema.ProfileView"]
impl.lang = "ts"
impl.path = "ui.profile"

[mod.auth]
purpose = "Authentication module"
schemas = ["schema.User", "schema.AuthToken"]
funcs = ["func.authenticate"]
pipeline = ["func.authenticate"]

[status]
updated_at = "2026-01-14"

[status.mod.auth]
state = "partial"
coverage = 0.7
notes = "Login flow done, registration pending"
```

---

## Validation Rules

The `surc check` command validates:

1. **Schema references**: All referenced schemas must exist
2. **Function references**: All referenced functions must exist
3. **Edge constraints**: Edges must have valid `from` and `to`
4. **Module completeness**: All schemas/funcs in pipeline must be declared
5. **Package consistency**: Files must belong to declared packages
6. **Namespace collisions**: No duplicate symbols within a namespace

---

## Tooling

### CLI Commands

```bash
# Single file operations
surc check api.toml                    # Validate
surc parse api.toml                    # Output AST as JSON
surc inspect mod.user_api api.toml     # Inspect module details
surc export pipeline api.toml user_api # Mermaid diagram

# Project operations (requires surv.toml)
surc project-check surv.toml           # Validate entire project
surc export modules surv.toml          # Module graph
surc export schemas surv.toml          # Schema graph
surc export html surv.toml > viz.html  # Interactive HTML

# Status tracking
surc status init api.toml
surc status set mod.user_api api.toml --state partial

# Drift detection
surc diff-impl design.toml ./src
surc diff-impl design.toml ./src --mod user_api --lang rust
```

---

## Version History

### v1.1 (2026-01-14)
- Added `[status]` section for implementation tracking
- Added `impl.bind`, `impl.lang`, `impl.path` for drift detection
- Added `surc diff-impl` command
- Improved documentation

### v1.0 (Initial Release)
- Core constructs: schema, func, mod
- Package and namespace support
- Manifest-based projects
- Visualization exports

---

## Future Considerations

- **Generics**: `schema.List<T>`
- **Constraints**: Field validators, invariants
- **Effects**: Side-effect annotations for functions
- **Deployment IR**: Separate spec for deployment pipelines (already implemented as Deploy IR)
- **Code generation**: Template-based code generation from IR

---

## License

[To be determined for OSS release]

