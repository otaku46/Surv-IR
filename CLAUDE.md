# Claude Code Integration Guide

This guide is specifically for using Surv IR with Claude Code (Anthropic's AI coding assistant).

---

## Quick Setup

### 1. Add Surv IR to Your Project

If using Claude Code in a project with Surv IR:

```bash
# Initialize status tracking
surc status init design/api.toml

# Validate your IR
surc check design/api.toml
```

### 2. Context for Claude

When starting a Claude conversation, provide:

1. **IR file path**: "I'm working with `design/api.toml`"
2. **Current focus**: "Implementing `mod.user_api`"
3. **Status**: Run `surc status list design/api.toml`

---

## Common Claude Workflows

### Workflow 1: "Implement this Surv IR module"

**User:**
```
Please implement mod.user_api from design/api.toml in Rust
```

**Claude should:**

1. **Read the IR file**
   ```bash
   surc inspect mod.user_api design/api.toml
   ```

2. **Understand the contract**
   - Schemas to implement
   - Functions to implement
   - Pipeline (execution flow)

3. **Generate code** matching the IR

4. **Update status**
   ```bash
   surc status set mod.user_api design/api.toml --state done --coverage 1.0
   ```

### Workflow 2: "Design a feature in Surv IR"

**User:**
```
Design a notification system in Surv IR
```

**Claude should:**

1. **Create IR file**
   ```toml
   [schema.Notification]
   kind = "node"
   role = "event"
   fields = {id = "uuid", message = "string", user_id = "uuid"}

   [func.sendNotification]
   intent = "Send notification to user"
   input = ["schema.Notification"]
   output = ["schema.NotificationResult"]

   [mod.notification_service]
   purpose = "User notification system"
   schemas = ["schema.Notification"]
   funcs = ["func.sendNotification"]
   pipeline = ["func.sendNotification"]
   ```

2. **Validate**
   ```bash
   surc check notification.toml
   ```

3. **Visualize**
   ```bash
   surc export pipeline notification.toml notification_service
   ```

4. **Show to user** with explanation

### Workflow 3: "Update existing IR"

**User:**
```
Add email notifications to the notification system
```

**Claude should:**

1. **Read current IR**
   ```bash
   surc inspect mod.notification_service design/notification.toml
   ```

2. **Make incremental changes**
   ```toml
   # Add new schema
   [schema.EmailNotification]
   kind = "node"
   role = "event"
   fields = {email = "string", subject = "string", body = "string"}

   # Add new function
   [func.sendEmail]
   intent = "Send email notification"
   input = ["schema.EmailNotification"]
   output = ["schema.EmailResult"]

   # Update module
   [mod.notification_service]
   schemas = ["schema.Notification", "schema.EmailNotification"]
   funcs = ["func.sendNotification", "func.sendEmail"]
   pipeline = ["func.sendNotification", "func.sendEmail"]
   ```

3. **Validate**
   ```bash
   surc check design/notification.toml
   ```

### Workflow 4: "Check implementation status"

**User:**
```
What's the status of our implementation?
```

**Claude should:**

```bash
surc status list design/api.toml
```

**Output example:**
```
Modules in design/api.toml:

  mod.user_api         ✓ done      100%  Fully implemented
  mod.notification     ◐ partial    60%  Email pending
  mod.analytics        ☐ todo        0%  Not started

Last updated: 2026-01-15
```

---

## Integration with Development Workflow

### Design → Implement → Track

```bash
# 1. Claude designs in IR
cat > design/feature.toml << 'EOF'
[schema.NewFeature]
...
EOF

surc check design/feature.toml

# 2. Claude implements
cat > src/feature.rs << 'EOF'
pub struct NewFeature { ... }
EOF

# 3. Track progress
surc status set mod.feature design/feature.toml --state done

# 4. (Future) Verify alignment
surc diff-impl design/feature.toml ./src --mod feature
```

---

## Using Surv IR for Understanding Code

### When User Says: "Explain this codebase"

1. **Check for Surv IR files**
   ```bash
   find . -name "*.toml" -path "*/design/*"
   # or
   ls design/*.toml
   # or
   ls surv.toml
   ```

2. **If IR exists, use it!**
   ```bash
   # Project overview
   surc project-check surv.toml
   surc export modules surv.toml

   # Specific module
   surc inspect mod.interesting_module design/api.toml
   ```

3. **Explain based on IR**
   - "This system has 3 modules: user_api, notification, analytics"
   - "The user_api module handles: ..."
   - "The data flow is: func.validate → func.create → func.notify"

### When User Says: "Where is X implemented?"

1. **Check IR first**
   ```bash
   grep -r "schema.X\|func.X" design/
   ```

2. **Use impl.bind if available**
   ```toml
   [func.createUser]
   impl.bind = "create_user_handler"  # Look for this in code
   impl.path = "handlers::user"       # In this module
   ```

3. **Search code**
   ```bash
   grep -r "create_user_handler" src/
   ```

---

## Best Practices for Claude

### 1. Always Validate

After creating/modifying IR:
```bash
surc check <file.toml>
```

If validation fails, **fix errors before proceeding**.

### 2. Visualize Complex Modules

For modules with >5 functions:
```bash
surc export pipeline design/api.toml complex_module
```

Show Mermaid diagram to user for clarity.

### 3. Use Status Tracking

When implementing iteratively:
```bash
# Initial
surc status init design/api.toml

# After each module
surc status set mod.user_api design/api.toml --state partial --coverage 0.5

# When done
surc status set mod.user_api design/api.toml --state done --coverage 1.0
```

### 4. Reference by Location

When discussing code, reference IR elements:
```
The User schema is defined in design/api.toml:42
The createUser function (design/api.toml:67) validates...
```

### 5. Keep IR and Code in Sync

**When code changes:**
1. Update implementation
2. Update IR if contract changed
3. Update status
4. Validate

```bash
# After code changes
surc check design/api.toml
surc status set mod.user_api design/api.toml --coverage 0.8
```

---

## Explaining Surv IR Concepts to Users

### When User Asks: "What is this .toml file?"

**Answer:**
> This is a Surv IR file—a design document that describes your system architecture in a structured, machine-readable format. It defines:
> - **Schemas**: Your data structures (like User, Post, etc.)
> - **Functions**: Operations on that data (like createUser, deletePost)
> - **Modules**: How these are grouped into cohesive units
>
> You can validate it with `surc check`, visualize it, and track implementation progress.

### When User Asks: "Why use Surv IR?"

**Benefits to highlight:**
1. **Single source of truth** for architecture
2. **Validates** design before coding
3. **Visualizes** system structure
4. **Tracks** implementation progress
5. **Bridges** design and implementation
6. **Language-agnostic** (works with Rust, TypeScript, etc.)

### When User Asks: "How is this different from comments?"

**Key differences:**
- **Structured**: Machine-readable, validated
- **Visual**: Auto-generates diagrams
- **Tracked**: Built-in status tracking
- **Contract**: Defines interfaces formally
- **Evolvable**: Easy to refactor and update

---

## Common Patterns to Recommend

### Pattern 1: REST API

When user wants to build an API:

```toml
[schema.Book]
kind = "node"
role = "entity"
fields = {id = "uuid", title = "string", author = "string"}

[func.createBook]
intent = "Create a new book"
input = ["schema.CreateBookRequest"]
output = ["schema.Book"]

[mod.book_api]
purpose = "Book management REST API"
schemas = ["schema.Book"]
funcs = ["func.createBook"]
boundary = {http = ["POST /books"]}
```

### Pattern 2: Event Processing

When user has event-driven needs:

```toml
[schema.OrderPlaced]
kind = "node"
role = "event"
fields = {order_id = "uuid"}

[func.processOrder]
intent = "Handle order placement"
input = ["schema.OrderPlaced"]
output = ["schema.PaymentRequest"]

[mod.order_processor]
purpose = "Order event handler"
boundary = {events = ["order.placed"]}
```

### Pattern 3: Data Pipeline

When user needs ETL/data processing:

```toml
[func.extract]
intent = "Extract from source"
output = ["schema.RawData"]

[func.transform]
intent = "Clean and transform"
input = ["schema.RawData"]
output = ["schema.CleanData"]

[func.load]
intent = "Load to destination"
input = ["schema.CleanData"]

[mod.etl]
purpose = "ETL pipeline"
pipeline = ["func.extract", "func.transform", "func.load"]
```

---

## Troubleshooting with Claude

### Error: "schema X not found"

**Claude should:**
1. Check if schema is defined: `grep -n "schema.X" design/api.toml`
2. If missing, ask user: "Should I define schema.X?"
3. If typo, fix the reference

### Error: "Module Y never referenced"

**Claude should:**
1. Explain: "This module isn't used by other modules"
2. Ask: "Is this a top-level module (entry point)?"
3. If not, suggest: "Should this be added to another module's dependencies?"

### Error: "Circular dependency"

**Claude should:**
1. Explain the cycle
2. Suggest: "We can break this by introducing an intermediate module"
3. Show restructured design

---

## Project Structure Recommendations

### Small Projects (1-3 modules)

```
project/
├── design/
│   └── api.toml          # Single IR file
├── src/
│   └── main.rs
└── README.md
```

### Medium Projects (4-10 modules)

```
project/
├── design/
│   ├── user.toml
│   ├── auth.toml
│   └── posts.toml
├── src/
│   ├── user/
│   ├── auth/
│   └── posts/
└── surv.toml             # Project manifest
```

### Large Projects (10+ modules)

```
project/
├── surv.toml             # Project manifest
├── design/
│   ├── backend/
│   │   ├── api.toml
│   │   └── database.toml
│   └── frontend/
│       └── ui.toml
└── src/
    ├── backend/
    └── frontend/
```

**Manifest example:**
```toml
[project]
name = "my-app"

[paths]
ir_root = "design"

[packages.backend]
root = "design/backend"
namespace = "api"

[packages.frontend]
root = "design/frontend"
namespace = "ui"
depends = ["backend"]
```

---

## Advanced Features

### Using with Deploy IR

Surv IR can be paired with Deploy IR for deployment pipelines:

```toml
# deploy.toml
[deploy.pipeline]
name = "my-app"

[deploy.job.build]
requires = []
runs = ["cargo build --release"]

[deploy.job.deploy]
requires = ["job.build"]
runs = ["kubectl apply -f deploy.yaml"]
```

```bash
surc deploy-check deploy.toml
surc codegen github-actions deploy.toml > .github/workflows/deploy.yml
```

### Status-Driven Development

**Recommend this workflow:**

1. Design all modules (set all to `todo`)
2. Implement one module at a time
3. Update status after each
4. Track overall progress

```bash
# See overall progress
surc status list design/api.toml

# Output shows:
#   mod.user_api    ✓ done      100%
#   mod.auth        ◐ partial    60%
#   mod.posts       ☐ todo        0%
```

### Implementation Metadata

**When code is implemented, add metadata:**

```toml
[func.createUser]
intent = "Create user account"
impl.bind = "create_user_handler"  # Actual function name
impl.lang = "rust"                  # Language
impl.path = "handlers::user"       # Module path
```

**This enables future drift detection:**
```bash
surc diff-impl design/api.toml ./src --mod user_api
```

---

## Quick Reference Card

### Essential Commands

```bash
# Validation
surc check <file.toml>
surc project-check surv.toml

# Inspection
surc inspect <module> <file.toml>

# Visualization
surc export pipeline <file.toml> <module>
surc export html surv.toml > viz.html

# Status Tracking
surc status init <file.toml>
surc status set <module> <file.toml> --state <state>
surc status list <file.toml>

# Future: Drift Detection
surc diff-impl <design.toml> <workspace> --mod <module>
```

### Essential Syntax

```toml
# Schema
[schema.Name]
kind = "node" | "edge" | "value"
role = "entity" | "event" | "request" | "response"
fields = {field1 = "type1", field2 = "type2"}

# Function
[func.name]
intent = "What this does"
input = ["schema.Input"]
output = ["schema.Output"]

# Module
[mod.name]
purpose = "What this module does"
schemas = ["schema.A", "schema.B"]
funcs = ["func.X", "func.Y"]
pipeline = ["func.X", "func.Y"]
```

---

## Integration Tips

### 1. Auto-validate Before Committing

Suggest adding to git hooks:
```bash
#!/bin/bash
# .git/hooks/pre-commit
surc project-check surv.toml || exit 1
```

### 2. Generate Documentation

```bash
# Create visual docs
surc export html surv.toml > docs/architecture.html

# Generate markdown
surc export modules surv.toml > docs/modules.md
```

### 3. Track in README

Suggest adding to README.md:
```markdown
## Architecture

See [design/](design/) for Surv IR specifications.

Current implementation status:
\`\`\`bash
surc status list design/api.toml
\`\`\`
```

---

## Summary for Claude

When working with Surv IR:

1. ✅ **Always validate** with `surc check`
2. ✅ **Inspect before implementing** with `surc inspect`
3. ✅ **Track progress** with `surc status`
4. ✅ **Visualize when helpful** with `surc export`
5. ✅ **Follow patterns** from examples/
6. ✅ **Keep IR and code in sync**
7. ✅ **Use inline arrays** for module definitions
8. ✅ **Reference correctly** (`schema.X`, not `[schema.X]`)

**Key insight:** Surv IR is a **contract** between design and implementation. Treat it as the source of truth for architecture discussions.
