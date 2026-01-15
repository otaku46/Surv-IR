# Deploy IR for Implementation Planning

## Concept

Deploy IR's concepts map directly to implementation planning:

| Deploy IR Concept | Implementation Planning |
|------------------|------------------------|
| **DAG** | Module/function dependencies |
| **Job** | Implementation task (function/module) |
| **Gate** | Code review / approval points |
| **Side effects** | Breaking changes / risky operations |
| **Target** | Implementation scope (feature/sprint/milestone) |
| **Rollback** | Error recovery / revert strategy |

## Example: Converting Surv IR to Implementation Plan

### Input: Surv IR (user_system.toml)

```toml
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string", email = "string"}

[schema.Token]
kind = "node"
role = "entity"
fields = {user_id = "uuid", token = "string", expires_at = "timestamp"}

[func.createUser]
intent = "Create new user"
input = ["schema.UserRequest"]
output = ["schema.User", "schema.DuplicateEmailError"]

[func.authenticate]
intent = "Authenticate user"
input = ["schema.Credentials"]
output = ["schema.Token", "schema.AuthError"]

[mod.user_repository]
purpose = "User data access"
schemas = ["schema.User"]
funcs = ["func.createUser"]
pipeline = ["func.createUser"]

[mod.auth_service]
purpose = "Authentication service"
schemas = ["schema.Token"]
funcs = ["func.authenticate"]
require = ["mod.user_repository"]
pipeline = ["func.authenticate"]
```

### Generated: Implementation Plan (impl_plan.toml)

```toml
[deploy.pipeline]
name = "user-system-implementation"
description = "Implementation plan for user authentication system"

# ============================================================================
# TARGETS: Implementation Scopes
# ============================================================================

[deploy.target.foundation]
kind = "staging"
description = "Foundation schemas (no dependencies)"

[deploy.target.mvp]
kind = "production"
description = "Minimum viable product"

# ============================================================================
# JOBS: Implementation Tasks
# ============================================================================

# Phase 1: Schemas (no dependencies)
[deploy.job.implement_user_schema]
requires = []
runs = [
  "Define User struct in src/models/user.rs",
  "Add field validation",
  "Write unit tests for User model"
]
produces = ["artifact.user_model"]
uses_target = "target.foundation"

[deploy.job.implement_token_schema]
requires = []
runs = [
  "Define Token struct in src/models/token.rs",
  "Add expiration logic",
  "Write unit tests for Token model"
]
produces = ["artifact.token_model"]
uses_target = "target.foundation"

# Phase 2: Functions (depend on schemas)
[deploy.job.implement_createUser]
requires = ["job.implement_user_schema"]
runs = [
  "Implement createUser in src/repository/user.rs",
  "Add database insert logic",
  "Handle DuplicateEmailError",
  "Write integration tests"
]
produces = ["artifact.create_user_func"]
uses_target = "target.foundation"
side_effects = ["database_write"]  # Risky operation

[deploy.job.implement_authenticate]
requires = ["job.implement_user_schema", "job.implement_token_schema"]
runs = [
  "Implement authenticate in src/services/auth.rs",
  "Add password hashing verification",
  "Generate JWT token",
  "Handle AuthError cases",
  "Write integration tests"
]
produces = ["artifact.auth_func"]
uses_target = "target.mvp"
needs_secrets = ["secret.JWT_SECRET"]
side_effects = ["security_critical"]  # Breaking changes affect auth

# Phase 3: Modules (integration)
[deploy.job.integrate_user_repository]
requires = ["job.implement_createUser"]
runs = [
  "Create mod.rs for user_repository",
  "Export public API",
  "Write module-level integration tests"
]
produces = ["artifact.user_repository_mod"]
uses_target = "target.foundation"

[deploy.job.integrate_auth_service]
requires = ["job.implement_authenticate", "job.integrate_user_repository"]
runs = [
  "Create mod.rs for auth_service",
  "Wire dependencies to user_repository",
  "Write end-to-end tests",
  "Update API documentation"
]
produces = ["artifact.auth_service_mod"]
uses_target = "target.mvp"

# ============================================================================
# ARTIFACTS: Deliverables
# ============================================================================

[deploy.artifact.user_model]
kind = "code"
paths = ["src/models/user.rs"]

[deploy.artifact.token_model]
kind = "code"
paths = ["src/models/token.rs"]

[deploy.artifact.create_user_func]
kind = "code"
paths = ["src/repository/user.rs"]

[deploy.artifact.auth_func]
kind = "code"
paths = ["src/services/auth.rs"]

[deploy.artifact.user_repository_mod]
kind = "module"
paths = ["src/repository/"]

[deploy.artifact.auth_service_mod]
kind = "module"
paths = ["src/services/"]

# ============================================================================
# SECRETS: Configuration
# ============================================================================

[deploy.secret.JWT_SECRET]
description = "Secret key for JWT token signing"
scope = ["job.implement_authenticate"]

# ============================================================================
# PERMISSIONS: Required Skills
# ============================================================================

[deploy.perm.database_access]
actions = ["read", "write"]
resources = ["database.users"]

[deploy.perm.crypto_knowledge]
actions = ["implement"]
resources = ["password_hashing", "jwt_generation"]

# ============================================================================
# GATE: Review Points
# ============================================================================

[deploy.gate]
require_manual_approval_for = ["target.mvp"]  # Review before MVP
require_tests_for = ["job.implement_createUser", "job.implement_authenticate"]

# ============================================================================
# ROLLBACK: Error Recovery
# ============================================================================

[deploy.rollback]
on = ["test_fail", "integration_fail"]
strategy = "revert_commit"
notify = ["team_lead"]
```

## Commands

### Generate Implementation Plan

```bash
# Convert Surv IR to implementation plan
surc impl-plan user_system.toml > impl_plan.toml
```

**Algorithm:**
1. Parse Surv IR and build dependency graph
2. Create jobs for schemas (Phase 1, no dependencies)
3. Create jobs for functions (Phase 2, depends on schemas)
4. Create jobs for modules (Phase 3, depends on functions)
5. Mark functions with error outputs as `side_effects = ["risky"]`
6. Mark modules with `boundary` as requiring manual approval
7. Output as Deploy IR format

### Validate Implementation Plan

```bash
surc deploy-check impl_plan.toml
```

Uses existing Deploy IR checker to:
- ✅ Validate DAG (no cycles)
- ✅ Check unreachable jobs
- ✅ Validate secret scope
- ✅ Ensure gates are properly placed

### Visualize Implementation Plan

```bash
# Mermaid diagram
surc export deploy-mermaid impl_plan.toml

# Interactive HTML
surc export deploy-html impl_plan.toml > plan.html
```

### Track Progress

Create `progress.toml`:
```toml
[completed]
jobs = ["job.implement_user_schema", "job.implement_token_schema"]

[in_progress]
jobs = ["job.implement_createUser"]

[blocked]
jobs = ["job.implement_authenticate"]
reason = "Waiting for JWT_SECRET configuration"
```

Then check status:
```bash
surc impl-progress impl_plan.toml progress.toml
```

**Output:**
```
Implementation Progress: user-system-implementation

Overall: ████░░░░░░░░░░░░░░░░ 40% (2/5 jobs)

By Target:
  foundation: ████████████░░░░░░░░ 67% (2/3 jobs)
  mvp:        ░░░░░░░░░░░░░░░░░░░░  0% (0/2 jobs)

Completed:
  ✅ job.implement_user_schema
  ✅ job.implement_token_schema

In Progress:
  🔨 job.implement_createUser (foundation)

Ready to Start (unblocked):
  None

Blocked:
  ⏸️ job.implement_authenticate
     Reason: Waiting for JWT_SECRET configuration
     Unblocks: job.integrate_auth_service

Next Recommended:
  1. Complete job.implement_createUser (blocks 1 downstream job)
```

## Advanced Features

### Critical Path Analysis

```bash
surc impl-critical-path impl_plan.toml progress.toml
```

**Output:**
```
Critical Path (longest dependency chain):

job.implement_user_schema (✅ done)
  → job.implement_createUser (🔨 in progress)
  → job.integrate_user_repository (⏸️ waiting)
  → job.implement_authenticate (⏸️ blocked)
  → job.integrate_auth_service (⏸️ waiting)

Total remaining: 4 jobs
Estimated if sequential: ~8-12 days

Parallelization opportunities:
  - job.implement_token_schema can run in parallel (already done ✅)
```

### Risk Assessment

```bash
surc impl-risks impl_plan.toml
```

**Output:**
```
Risk Assessment

High Risk Jobs (have side_effects):
  ⚠️ job.implement_createUser
     Side effect: database_write
     Mitigation: Requires comprehensive tests (gate enforced)

  ⚠️ job.implement_authenticate
     Side effect: security_critical
     Mitigation: Manual approval required (target.mvp gate)
     Missing: secret.JWT_SECRET not configured

Recommendations:
  1. Configure JWT_SECRET before starting authenticate
  2. Add rollback.rollback_db for database_write operations
  3. Consider adding gate for job.implement_createUser
```

### Sprint Planning

```bash
# Get next 3 unblocked, high-impact tasks
surc impl-next impl_plan.toml progress.toml --limit 3
```

**Output:**
```
Next Sprint: Top 3 Tasks

1. job.implement_createUser (HIGH PRIORITY)
   Status: In Progress
   Blocks: 2 downstream jobs
   Side effects: database_write (requires careful testing)
   Estimated: 2-3 days

2. job.implement_token_schema (MEDIUM PRIORITY)
   Status: Not Started
   Blocks: 1 downstream job
   No side effects
   Estimated: 1 day

3. job.integrate_user_repository (LOW PRIORITY)
   Status: Blocked by job.implement_createUser
   Will be unblocked soon
   Estimated: 1 day
```

## Mapping Rules

When converting Surv IR to Implementation Plan:

### 1. Schemas → Foundation Jobs

```toml
[schema.User]
# becomes...
[deploy.job.implement_user_schema]
requires = []
uses_target = "target.foundation"
```

### 2. Functions → Implementation Jobs

```toml
[func.createUser]
input = ["schema.UserRequest"]
output = ["schema.User", "schema.DuplicateEmailError"]
# becomes...
[deploy.job.implement_createUser]
requires = ["job.implement_user_schema"]  # from input/output schemas
side_effects = ["risky"]  # because has error output
```

### 3. Modules → Integration Jobs

```toml
[mod.user_repository]
require = ["mod.other"]
# becomes...
[deploy.job.integrate_user_repository]
requires = ["job.integrate_other"]  # from module dependencies
```

### 4. Error Outputs → Side Effects

```toml
output = ["schema.Success", "schema.DatabaseError"]
# becomes...
side_effects = ["database_operation"]
```

### 5. Boundary → Gates

```toml
boundary = {http = ["POST /users"]}
# becomes...
uses_target = "target.production"  # requires approval via gate
```

### 6. Module Dependencies → Job Dependencies

```toml
[mod.auth_service]
require = ["mod.user_repository"]
# becomes...
[deploy.job.integrate_auth_service]
requires = ["job.integrate_user_repository"]
```

## Benefits

### ✅ Automatic Dependency Ordering
No manual sorting needed - DAG analysis gives correct implementation order

### ✅ Progress Tracking
Reuse Deploy IR visualization and progress tools

### ✅ Risk Identification
Side effects and gates highlight risky operations

### ✅ Bottleneck Detection
Critical path analysis shows what to prioritize

### ✅ Team Coordination
Gates show where review/approval is needed

### ✅ Rollback Strategy
Explicit error recovery for failed implementations

## Implementation Strategy

### Phase 1: Converter (surc impl-plan)
- Parse Surv IR
- Build module dependency graph
- Generate Deploy IR with jobs for schemas, functions, modules
- Infer side_effects from error outputs
- Add gates for modules with boundaries

### Phase 2: Progress Tracker (surc impl-progress)
- Parse progress.toml
- Overlay on implementation plan
- Calculate completion percentage
- Identify blockers and next tasks
- Reuse Deploy IR visualizations

### Phase 3: Analysis Tools
- Critical path calculation
- Risk assessment
- Sprint planning recommendations
- Time estimation (based on historical data)

## Example Workflow

```bash
# 1. Design architecture
vim user_system.toml
surc check user_system.toml

# 2. Generate implementation plan
surc impl-plan user_system.toml > impl_plan.toml
surc deploy-check impl_plan.toml

# 3. Visualize plan
surc export deploy-html impl_plan.toml > plan.html
open plan.html

# 4. Start implementing
# ... work on tasks ...

# 5. Track progress
vim progress.toml  # Update completed jobs
surc impl-progress impl_plan.toml progress.toml

# 6. Plan next sprint
surc impl-next impl_plan.toml progress.toml --limit 5

# 7. Check risks
surc impl-risks impl_plan.toml
```

## Summary

By reusing Deploy IR concepts for implementation planning, we get:

- **Single framework** for both deployment and development
- **Proven DAG analysis** already implemented in Deploy IR checker
- **Visualization tools** (Mermaid, HTML) work out of the box
- **Clear semantics** for dependencies, risks, and approval points
- **Minimal new code** - mostly format conversion and progress tracking

The insight is: **Deploying code and implementing code are both DAG workflows with dependencies, risks, and approval points.**
