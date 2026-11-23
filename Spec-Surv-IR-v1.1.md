# Surv IR Specification v1.1

**Surv IR** (Intermediate Representation) is a declarative, TOML-based language for describing system architectures through data schemas, functions, module compositions, and implementation bindings.

---

## Table of Contents

1. [Overview](#overview)
2. [File Structure](#file-structure)
3. [Headers](#headers)
4. [Core Constructs](#core-constructs)
   - [Schema](#schema)
   - [Func](#func)
   - [Mod](#mod)
5. [Metadata, Dependencies & Status](#metadata-dependencies--status)
   - [Meta Section](#meta-section)
   - [Import & Require](#import--require)
   - [Status Section](#status-section)
   - [Implementation Metadata & Mapping](#implementation-metadata--mapping)
6. [Project Organization](#project-organization)
   - [Packages & Manifests (`surv.toml`)](#packages--manifests-survtoml)
   - [Namespaces](#namespaces)
7. [Reference Resolution](#reference-resolution)
8. [Validation Rules](#validation-rules)
9. [Tooling (`surc` CLI)](#tooling-surc-cli)
10. [Examples](#examples)
11. [Behavior IR](#behavior-ir)

---

## Overview

Surv IR provides a single source of architectural truth across three core layers:
- **Schemas (`[schema.*]`)** — Data structures, relationships, and boundaries
- **Functions (`[func.*]`)** — Transformations and operations over schemas
- **Modules (`[mod.*]`)** — Cohesive subsystems, pipelines, and external boundaries

Alongside Structural Surv IR (this specification), the Surv ecosystem includes:
- **[Behavior IR](behavior.md)** — Dynamic state machines, transitions, invariants, and temporal properties over Surv IR symbols
- **Deploy IR** — Declarative CI/CD and deployment pipelines (`[deploy.*]`)

**Design Principles:**
- Declarative over imperative
- Language-agnostic architecture description
- Explicit separation (and automated drift detection) between design and implementation
- Deterministic, scriptable tooling for both humans and AI agents

---

## File Structure

A Surv IR file is a valid TOML document containing optional top-level headers followed by sections:

```toml
# Optional file-level headers (must appear before any [section])
package = "backend"
namespace = "app.user"
import = ["common"]
require = ["mod.shared_auth"]

# Optional metadata
[meta]
name = "user_service"
version = "0.1.0"
description = "User management service design"

# Schema definitions
[schema.User]
kind = "node"
role = "entity"
fields = { id = "uuid", name = "string", email = "string" }

# Function definitions
[func.createUser]
intent = "Create a new user"
input = ["schema.CreateUserRequest"]
output = ["schema.User"]

# Module definitions
[mod.user_api]
purpose = "User management HTTP API"
schemas = ["schema.User", "schema.CreateUserRequest"]
funcs = ["func.createUser"]
pipeline = ["func.createUser"]
boundary = { http = ["POST /users"] }
```

---

## Headers

Headers must appear at the top of a file before any `[section]` header.

### `package`
Optional. Declares which package (defined in `surv.toml`) this file belongs to.

```toml
package = "backend"
```

### `namespace`
Optional. Prefixes all local identifiers defined in the file when resolved across a project.

```toml
namespace = "app.user"
```

- `schema.Profile` → resolved globally as `app.user.schema.Profile`
- `func.authenticate` → resolved globally as `app.user.func.authenticate`
- `mod.user_api` → resolved globally as `app.user.mod.user_api`

### `import`
Optional array of strings. Imports packages (and optionally aliases them) for cross-package symbol resolution.

```toml
import = ["common", "auth as security"]
```

### `require` (or `requires`)
Optional array of strings. Declares module-level dependencies required by modules in this file. Every entry **must** start with `mod.`.

```toml
require = ["mod.user_auth", "mod.database"]
```

---

## Core Constructs

### Schema

Schemas describe data structures, graph relationships, and state boundaries.

#### Syntax

```toml
[schema.SchemaName]
kind = "node" | "edge" | "boundary" | "value"
role = "entity" | "event" | "request" | "response" | "context" | "error" | ...
type = "UnderlyingType"                # Optional: type alias or target name
from = "schema.Source"                 # Required when kind = "edge"
to = "schema.Target"                   # Required when kind = "edge"
base = "schema.Parent"                 # Optional: base schema reference
over = ["schema.A", "schema.B"]        # Required when kind = "boundary" (or union)
label = "Human-readable description"   # Optional
fields = { field1 = "type1", field2 = "type2" }

# Implementation metadata (optional)
impl.bind = "ActualTypeName"
impl.lang = "rust" | "ts" | "c" | "zig" | "either"
impl.path = "module::path"
```

#### Schema Kinds

| Kind | Description | Required Fields |
| :--- | :--- | :--- |
| `node` | Standalone entity, DTO, event, or value object | `fields` (optional) |
| `edge` | Directed relationship between two schemas | `from`, `to` |
| `boundary` | Collection/snapshot view or union over existing schemas | `over` (`["schema.X", ...]`) |
| `value` | Primitive wrapper or scalar value type | `type` or `fields` |

#### Field Types

Field types inside `fields = { ... }` use a concise type notation:
- **Primitives**: `string`, `int`, `float`, `bool`, `uuid`, `timestamp`, `bytes`, `any`
- **Schema references**: `schema.OtherSchema` or `OtherSchema`
- **Arrays / Lists**: `string[]`, `schema.User[]`, or `[User]`
- **Optional / Nullable**: `string?`, `schema.User?`
- **Inline unions / enums**: `"todo|partial|done"`

#### Schema Roles

`role` is an advisory semantic tag used by visualization and analysis tools:
- `entity`, `event`, `request`, `response`, `query`, `context`, `error`, `diagnostic`, `config`, `report`

#### Example

```toml
[schema.User]
kind = "node"
role = "entity"
fields = { id = "uuid", name = "string", email = "string", created_at = "timestamp" }

[schema.Follows]
kind = "edge"
role = "relationship"
from = "schema.User"
to = "schema.User"
fields = { since = "timestamp" }

[schema.UsersSnapshot]
kind = "boundary"
role = "context"
over = ["schema.User"]
label = "Active users in storage"
```

---

### Func

Functions describe stateless transformations or stateful operations over schemas.

#### Syntax

```toml
[func.FunctionName]
intent = "Human-readable description of what this function does"
input = ["schema.Input1", "schema.Input2"]
output = ["schema.Output1", "schema.Output2"]
design_notes = "Optional implementation notes or edge cases"

# Implementation metadata (optional)
impl.bind = "actual_function_name"
impl.lang = "rust" | "ts" | "c" | "zig" | "either"
impl.path = "module::path"
```

#### Fields

- **`intent`**: Describes the responsibility of the function.
- **`input`**: Array of `schema.*` references consumed by the function.
- **`output`**: Array of `schema.*` references produced by the function.
- **`design_notes`**: Optional architectural or implementation notes.

---

### Mod

Modules group schemas, functions, and execution pipelines into cohesive architectural units.

#### Syntax

```toml
[mod.ModuleName]
purpose = "Description of this module's responsibility"
submods = ["childModule"]
schemas = ["schema.A", "schema.B"]
funcs = ["func.X", "func.Y"]
pipeline = ["func.X -> func.Y"]
boundary = { http = ["POST /users"], events = ["user.created"] }
```

#### Fields

- **`purpose`**: High-level description of the module's responsibility.
- **`submods`**: Optional array of child module names (`["api"]` refers to `[mod.ModuleName.api]`).
- **`schemas`**: Array of `schema.*` references used by this module.
- **`funcs`**: Array of `func.*` references belonging to this module.
- **`pipeline`**: Execution and data-flow steps within the module.
- **`boundary`**: Optional external interface metadata (HTTP routes, events, CLI entrypoints, etc.).

#### Nested Modules

Modules can be nested hierarchically using dot notation `[mod.parent.child]`. Declaring `submods = ["child"]` in `[mod.parent]` enables static verification that `[mod.parent.child]` is defined.

```toml
[mod.user]
purpose = "User domain root"
submods = ["api", "auth"]

[mod.user.api]
purpose = "User HTTP endpoints"

[mod.user.auth]
purpose = "User authentication handlers"
```

#### Pipeline Syntax

`pipeline` supports sequential lists, ASCII arrow chains (`->`), parallel blocks, and conditional branches:

```toml
# 1. Sequential array
pipeline = ["func.validate", "func.transform", "func.save"]

# 2. Arrow chain (parsed as a sequential chain)
pipeline = ["func.validate -> func.transform -> func.save"]

# 3. Parallel execution
pipeline = [
    "func.fetchUser",
    { parallel = ["func.fetchOrders", "func.fetchPreferences"] },
    "func.aggregateData"
]

# 4. Conditional branching
pipeline = [
    "func.checkCache",
    { branch = { condition = "cache_miss", on_true = "func.fetchFromDB", on_false = "func.returnCached" } }
]
```

> **Note:** Always use ASCII `->` for pipeline chains.

---

## Metadata, Dependencies & Status

### Meta Section

Provides file-level descriptive metadata:

```toml
[meta]
name = "user_service_design"
version = "0.2.0"
description = "REST API and domain logic for user management"
```

---

### Import & Require

#### `import` (Package Scope & Aliasing)

Declares external packages (from `surv.toml`) whose symbols can be referenced in the current file:

```toml
import = ["common", "billing as pay"]
```

- Allows referencing symbols in package `common` by package name (`common.schema.Error`) or namespace (`app.common.schema.Error`), or via alias (`pay.schema.Invoice`).
- Checked against `surv.toml` package declarations and `depends` constraints.

#### `require` (Module Dependency)

Declares that modules in this file depend on other modules:

```toml
require = ["mod.auth", "mod.storage"]
```

- Each entry must start with `"mod."` (or `"<package_or_namespace>.mod.<name>"`).
- Validated across the project for existence and circular dependency freedom.

---

### Status Section

Tracks implementation progress directly inside Surv IR files:

```toml
[status]
updated_at = "2026-11-18"

[status.mod.user_api]
state = "done" | "partial" | "skeleton" | "todo" | "blocked"
coverage = 0.75
notes = "Create and get endpoints implemented"
```

#### Status States

| State | Icon | Meaning |
| :--- | :---: | :--- |
| `todo` | ☐ | Not started |
| `skeleton` | ◯ | Types and boundaries defined, logic unimplemented |
| `partial` | ◐ | Partially implemented |
| `done` | ✓ | Fully implemented |
| `blocked` | ✗ | Blocked or awaiting redesign |

---

### Implementation Metadata & Mapping

Surv IR symbols (`[schema.*]` and `[func.*]`) can carry implementation bindings for static/LSP drift detection (`surc diff`) and stub generation (`surc skeleton`).

#### Inline `impl.*` Attributes

- **`impl.bind`**: Actual symbol name in source code if different from the IR name.
- **`impl.lang`**: Target implementation language: `"rust"`, `"ts"`, `"c"`, `"zig"`, `"either"`, or omit for default workspace languages.
- **`impl.path`**: Implementation module/container path (matched by path segments across `::`, `.`, or `/`, e.g., `"models::user"` or `"ui.workspace.tree"`).

```toml
[schema.User]
kind = "node"
fields = { id = "string", name = "string" }
impl.bind = "UserModel"
impl.lang = "rust"
impl.path = "models::user"

[func.authenticate]
intent = "Authenticate user credentials"
input = ["schema.Credentials"]
output = ["schema.User"]
impl.bind = "authenticate_user"
impl.lang = "rust"
impl.path = "services::auth"
```

#### External Mapping File (`.surv/mapping.toml`)

For larger projects or automated Code ↔ IR workflows, symbol bindings can also be stored in a mapping TOML file passed via `surc diff <ir> <workspace> --mapping .surv/mapping.toml`:

```toml
version = "0.1"

[[entries]]
stable_id = "func:services::auth::authenticate_user"
surv_ref = "func.authenticate"
impl_lang = "rust"
impl_path = "services::auth::authenticate_user"
source_file = "src/services/auth.rs"
symbol_name = "authenticate_user"
kind = "Function"
confidence = 1.0
```

---

## Project Organization

### Packages & Manifests (`surv.toml`)

Multi-file projects are coordinated by a `surv.toml` manifest at the project root:

```toml
[project]
name = "my_app"

[paths]
ir_root = "design"

[packages.common]
root = "design/common"
namespace = "app.common"
depends = []

[packages.backend]
root = "design/backend"
namespace = "app.backend"
depends = ["common"]

[packages.frontend]
root = "design/frontend"
namespace = "app.ui"
depends = ["common", "backend"]

# Optional: explicit file list. When present, only these files are loaded.
[files]
"design/common/auth.toml" = { description = "Shared auth types" }

# Optional: explicit implementation types for semantic IR types.
[diff.type_map]
any = ["Any", "serde_json::Value"]
```

- **`[project]`**: Project name and optional description.
- **`[paths].ir_root`**: Base directory scanned recursively for `.toml` IR files.
- **`[files]`**: Optional explicit list of IR files, relative to `ir_root` (or the manifest directory as a fallback). `description` is informational.
- **`[diff.type_map]`**: Maps IR semantic types to implementation types. `any` has no default meaning; without an entry, an `any` comparison is reported as `unverified`, not drift. Qualified targets are matched by their full path, so `any = ["serde_json::Value"]` does not accept `other::Value`; unqualified targets match only unqualified implementation types.
- **`[packages.<name>]`**:
  - `root`: Directory path (relative to `surv.toml`) owning this package's IR files.
  - `namespace`: Default namespace for files in this package.
  - `depends`: List of package names this package is allowed to reference.

### Namespaces

If a file declares `namespace = "app.backend"` (or inherits it from its package), all definitions in that file belong to `app.backend`:
- Local references (`"schema.User"`) automatically resolve within the same file/package first.
- Cross-package references use either the target package prefix (`"common.schema.Id"`), import alias, or full namespace (`"app.common.schema.Id"`).

---

## Reference Resolution

When resolving a reference (`schema.*`, `func.*`, or `mod.*`):

1. **Local Package / Namespace Match**: Searches symbols defined in the current file and package first.
2. **Imported Package / Alias Match**: If prefixed with an imported package name or `import = ["pkg as alias"]`, resolves within that target package (and verifies that the dependency is declared in `surv.toml`).
3. **Global Unique Match**: Falls back to global symbol table lookup; reports an `Ambiguous*` diagnostic if multiple packages define the same unqualified symbol.

---

## Validation Rules

`surc check <path>` performs static analysis depending on the target type:

### Single-File Checks (`check_surv_file`)
1. **Undefined References**: All `schema.*`, `func.*`, and `submods` referenced in functions, edges, boundaries, and modules must exist.
2. **Pipeline Integrity**:
   - Every function in `pipeline` must be listed in the module's `funcs`.
   - Adjacent pipeline steps (`A -> B`) are checked for schema flow compatibility (warning if `A.output` and `B.input` are disjoint when both are non-empty).
   - Detects cycles in sequential pipeline definitions.
3. **Unused Definitions**: Emits warnings for `schema.*` or `func.*` sections not referenced by any module or schema.

### Project-Wide Checks (`check_project`)
1. **Package Assignment**: Validates that declared `package = "..."` headers match `surv.toml`.
2. **Duplicate Symbols**: Detects colliding symbol definitions within the same package and namespace.
3. **Cross-Package Resolution & Imports**: Verifies all cross-file `schema.*`, `func.*`, and `require = ["mod.*"]` references and checks for module dependency cycles.

### Implementation Drift Checks (`surc diff`)
1. **Missing / Extra / Ambiguous Symbols**: Matches IR symbols against source code via static analysis (Tree-sitter for Rust/TypeScript, built-in analyzers for C/Zig) or LSP (`--strategy lsp`).
2. **Function Signature Comparison**: Verifies parameter count and input/output schema compatibility against implementation function signatures.
3. **Schema Field Comparison**: Verifies struct/interface fields (missing fields, extra fields, and field type mismatches).
4. **Type Comparison**: Compares `?`/`[]` with `Option`/`Vec` structurally, expands uniquely resolvable Rust/TypeScript type aliases, compares named schema references by name, and maps inline unions to enum variants (including serde renames). Unknown aliases, enum variants, payload enums, and unmapped `any` types are reported as `unverified`, not drift. `any` mappings are declared in `surv.toml` under `[diff.type_map]`.

---

## Tooling (`surc` CLI)

```bash
# 1. Unified validation (Structural, Behavior, Deploy IR, or a project manifest)
surc check api.toml
surc check surv.toml
surc check deploy.toml
surc check behavior/order.behavior.toml
surc check api.toml --ast              # Dump parsed AST as JSON

# 2. Inspection & structural queries
surc inspect mod.user_api api.toml     # Module overview (schemas, funcs, pipeline, status)
surc inspect surv.toml                 # Project package & namespace overview
surc slice mod.user_api api.toml --with-defs --closure
surc refs schema.User surv.toml
surc trace func.createUser api.toml --both

# 3. Implementation drift detection & skeleton generation
surc diff surv.toml ./src
surc diff api.toml ./src --mod user_api --lang rust
surc init-ir ./src --exclude-tests > skeleton.toml
surc skeleton code api.toml --lang rust

# 4. Visualization & CI codegen
surc export pipeline api.toml user_api
surc export modules surv.toml
surc export schemas api.toml
surc export html surv.toml > viz.html
surc codegen github-actions deploy.toml > .github/workflows/deploy.yml

# 5. Implementation status tracking
surc status init api.toml
surc status sync api.toml
surc status set mod.user_api api.toml --state partial --coverage 0.7
surc status list api.toml

# 6. Behavior verification
surc verify behavior/order.behavior.toml --format json
```

---

## Examples

### Example 1: REST API with Pipeline & Status

```toml
[meta]
name = "book_api"
version = "0.1.0"

[schema.Book]
kind = "node"
role = "entity"
fields = { id = "uuid", title = "string", author = "string", isbn = "string" }
impl.lang = "rust"
impl.path = "models::book"

[schema.CreateBookRequest]
kind = "node"
role = "request"
fields = { title = "string", author = "string", isbn = "string" }

[func.validateBook]
intent = "Validate book request payload"
input = ["schema.CreateBookRequest"]
output = ["schema.CreateBookRequest"]

[func.createBook]
intent = "Persist a new book entity"
input = ["schema.CreateBookRequest"]
output = ["schema.Book"]
impl.bind = "create_book"
impl.lang = "rust"
impl.path = "handlers::book"

[mod.book_api]
purpose = "Book management REST API"
schemas = ["schema.Book", "schema.CreateBookRequest"]
funcs = ["func.validateBook", "func.createBook"]
pipeline = ["func.validateBook -> func.createBook"]
boundary = { http = ["POST /books"] }

[status]
updated_at = "2026-11-18"

[status.mod.book_api]
state = "partial"
coverage = 0.8
notes = "POST /books implemented"
```

### Example 2: Multi-Package Project

```toml
# --- surv.toml ---
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
```

```toml
# --- ir/catalog/products.toml ---
package = "catalog"
namespace = "ecom.catalog"

[schema.Product]
kind = "node"
role = "entity"
fields = { id = "uuid", name = "string", price = "float" }

[mod.catalog_service]
purpose = "Product catalog domain"
schemas = ["schema.Product"]
```

```toml
# --- ir/orders/orders.toml ---
package = "orders"
namespace = "ecom.orders"
import = ["catalog"]
require = ["mod.catalog_service"]

[schema.OrderItem]
kind = "node"
role = "value"
fields = { product = "catalog.schema.Product", quantity = "int" }

[schema.Order]
kind = "node"
role = "entity"
fields = { id = "uuid", items = "OrderItem[]" }

[mod.order_service]
purpose = "Order processing service"
schemas = ["schema.OrderItem", "schema.Order"]
```

---

## Behavior IR

Structural IR declares schemas, functions, and modules. Behavior IR describes how a system can move between states and which conditions must hold throughout those transitions. Keep Behavior models in separate `*.behavior.toml` files. A project can set `[paths].behavior_root = "behavior"` in `surv.toml` to discover models in a dedicated directory. Each model can optionally link to a structural module with `owner = "mod.name"` and link an action to structural functions with `ref = "func.name"` or `refs = ["func.name"]`. Run with the containing project manifest to resolve those references.

The `vars` table declares state variables and their domains. Set an initial value for every variable in `init`. Each action has an optional `guard` and an `update` table. An omitted guard enables the action in every state. All update expressions read the state before the action, and variables omitted from the update keep their values. An invariant must hold in every reachable state, including the initial state. A state with no enabled actions is a deadlock unless its `terminal` expression is true.

### Example 3: Order Lifecycle

Save this standalone model as `behavior/order.behavior.toml`. From the initial `pending` state, payment and cancellation are both possible. The model checks that a payment can be recorded at most once.

```toml
[behavior.order]
terminal = "status == 'paid' or status == 'cancelled'"

[behavior.order.vars]
status = "enum(pending, paid, cancelled)"
payment_count = "int(0..2)"

[behavior.order.init]
status = "pending"
payment_count = 0

[behavior.order.action.pay]
guard = "status == 'pending'"
update = { status = "'paid'", payment_count = "payment_count + 1" }

[behavior.order.action.cancel]
guard = "status == 'pending'"
update = { status = "'cancelled'" }

[behavior.order.invariant.no_double_payment]
expr = "payment_count <= 1"
```

```bash
surc check behavior/order.behavior.toml
surc verify behavior/order.behavior.toml
```

The model returns `PASS`. Remove the guard from `pay` and rerun verification to get a two-step counterexample in which `pay` fires twice. Verification returns `UNKNOWN` if an exploration limit leaves states unexamined. See the [Behavior IR specification](behavior.md) for expression syntax, exploration limits, and report formats.
