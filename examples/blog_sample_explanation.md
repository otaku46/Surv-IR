# Blog Sample: User Service Design

This document explains the `blog_sample_user_service.toml` example and demonstrates Surv IR's capabilities.

## Overview

The sample describes a simple **User Creation Service** with the following characteristics:

- Core domain logic (user validation and creation)
- Event-driven notification system
- HTTP REST API orchestration
- Explicit data flow between functions
- Cross-module dependencies

## Architecture Diagram

```
User Request
    ↓
[POST /users] → user_http_api
    ↓
validateAndCreateUser (domain logic)
    ↓ User
emitUserCreatedEvent (event generation)
    ↓ UserCreatedEvent
sendWelcomeEmail (notification)
    ↓ EmailNotification
buildCreateUserResponse (response)
    ↓
User Response
```

## Key Concepts Demonstrated

### 1. **Intent-Based Design**

Each function has an explicit `intent`:

```toml
[func.validateAndCreateUser]
intent = "Validate user request and create user in database"
input = ["schema.CreateUserRequest"]
output = ["schema.User"]
```

This is **not** just documentation—it's a machine-readable contract that:
- Clarifies the function's responsibility
- Enables validation of data flow
- Helps LLMs understand what to implement

### 2. **Data Boundaries (Schemas)**

Schemas represent the **boundaries between functions**:

```toml
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", email = "string", name = "string", created_at = "timestamp"}
```

Each schema explicitly declares:
- **kind**: How the data is used (node, edge, value)
- **role**: Semantic meaning (entity, event, request, response, etc.)
- **fields**: Exact structure

This enables:
- Type checking across function boundaries
- Detection of data flow errors
- Automatic code generation

### 3. **Explicit Dependencies**

Functions are connected by **schema output/input matching**:

```
func.validateAndCreateUser outputs [schema.User]
    ↓
func.emitUserCreatedEvent inputs [schema.User]
```

The checker verifies this chain is unbroken. If you remove a schema or disconnect the flow, `surc check` will catch it immediately.

### 4. **Module Boundaries**

Modules group related schemas/functions:

```toml
[mod.user_domain]
purpose = "User creation and domain logic"
schemas = ["schema.User", "schema.CreateUserRequest", "schema.UserCreatedEvent"]
funcs = ["func.validateAndCreateUser", "func.emitUserCreatedEvent"]
pipeline = ["func.validateAndCreateUser", "func.emitUserCreatedEvent"]
```

This declares:
- **What the module owns** (schemas/funcs)
- **How they connect** (pipeline)
- **What it exposes** (boundary)

### 5. **Cross-Cutting Concerns**

The `notification_service` module handles a side effect:

```toml
[mod.notification_service]
purpose = "Email notification handling"
boundary = {events = ["user.created"]}
```

This explicitly states "I respond to user.created events" — making side effects **visible** in the architecture.

## Checking Validity

Run validation:

```bash
surc check examples/blog_sample_user_service.toml
```

Output:
```
✓ No issues found
```

The checker verifies:
1. All referenced schemas exist
2. All referenced functions exist
3. Pipeline data flows correctly (output → input matching)
4. No dangling references
5. Module completeness

## Visualizing the Pipeline

Extract the pipeline for the HTTP API module:

```bash
surc export pipeline examples/blog_sample_user_service.toml user_http_api
```

This generates a Mermaid diagram showing the execution flow.

## Extracting Minimal Fragments

To implement the `notification_service` module, you only need:

```bash
surc slice mod.notification_service examples/blog_sample_user_service.toml --with-defs
```

This outputs:
- All schemas used by the module
- All functions used by the module
- Their definitions

**No manual searching needed** — the tool extracts exactly what you need.

## Finding References

To see everywhere that uses the `User` schema:

```bash
surc refs schema.User examples/blog_sample_user_service.toml
```

Output:
```
schema.User is referenced by:
  - func.validateAndCreateUser (output)
  - func.emitUserCreatedEvent (input)
  - func.buildCreateUserResponse (input)
  - mod.user_domain (schemas)
  - mod.user_http_api (schemas)
```

## Why This Matters

### Before Surv IR
- Design: "Create user, then send email, then return response"
- Reality: Implemented as scattered code
- Problem: **Global structure is implicit**

### With Surv IR
- Design: Explicit in `blog_sample_user_service.toml`
- Validation: `surc check` passes ✓
- Reality: Implementation must match the checked design
- Benefit: **Global structure is explicit and verified**

## Extension Example

To add a "user welcome SMS", you would:

1. Add schema:
```toml
[schema.SMSNotification]
kind = "node"
role = "entity"
fields = {id = "uuid", phone = "string", status = "string"}
```

2. Add function:
```toml
[func.sendWelcomeSMS]
intent = "Send SMS to new user's phone"
input = ["schema.UserCreatedEvent"]
output = ["schema.SMSNotification"]
```

3. Update pipeline in `user_http_api`:
```toml
pipeline = ["func.validateAndCreateUser", "func.emitUserCreatedEvent",
            "func.sendWelcomeEmail", "func.sendWelcomeSMS", "func.buildCreateUserResponse"]
```

4. Validate:
```bash
surc check examples/blog_sample_user_service.toml
```

If the data flow is broken, you **find out immediately**. No runtime surprises.

## Summary

This example shows how Surv IR enables:

- ✅ **Intent clarity**: Functions state what they do
- ✅ **Type safety**: Data boundaries are checked
- ✅ **Dependency tracing**: Every reference is verifiable
- ✅ **Design-first development**: Structure is confirmed before coding
- ✅ **AI collaboration**: LLMs can implement from a verified spec

This is the core value proposition of Surv IR.
