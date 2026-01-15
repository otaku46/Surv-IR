# Surv IR Pattern Catalog

This catalog provides reusable patterns for common architectural scenarios. Use these patterns as templates when writing Surv IR with LLMs or coding agents.

## Table of Contents

1. [REST API Module](#pattern-1-rest-api-module)
2. [Database Access Layer](#pattern-2-database-access-layer)
3. [Event-Driven Processing](#pattern-3-event-driven-processing)
4. [Data Pipeline](#pattern-4-data-pipeline)
5. [GraphQL API](#pattern-5-graphql-api)
6. [Authentication & Authorization](#pattern-6-authentication--authorization)
7. [Background Job Processor](#pattern-7-background-job-processor)
8. [Microservice Integration](#pattern-8-microservice-integration)

---

## Pattern 1: REST API Module

**Use when:** Building HTTP REST API endpoints

**Key characteristics:**
- Request/Response schemas for each endpoint
- Input validation functions
- Business logic functions
- HTTP boundary declarations
- Linear pipeline: validate → execute → serialize

**Structure:**

```toml
# 1. Define domain entities
[schema.Entity]
kind = "node"
role = "entity"
fields = {id = "uuid", ...}

# 2. Define request/response schemas
[schema.EntityRequest]
kind = "node"
role = "request"
fields = {...}

[schema.EntityResponse]
kind = "node"
role = "response"
fields = {...}

[schema.Error]
kind = "node"
role = "error"
fields = {code = "int", message = "string"}

# 3. Define handler functions
[func.getEntity]
intent = "Retrieve entity by ID"
input = ["schema.EntityRequest"]
output = ["schema.EntityResponse", "schema.Error"]

[func.createEntity]
intent = "Create new entity"
input = ["schema.EntityRequest"]
output = ["schema.EntityResponse", "schema.Error"]

# 4. Define the module
[mod.entity_api]
purpose = "Entity management REST API"
schemas = ["schema.Entity", "schema.EntityRequest", "schema.EntityResponse", "schema.Error"]
funcs = ["func.getEntity", "func.createEntity", "func.updateEntity", "func.deleteEntity"]
boundary = {http = ["GET /entities/:id", "POST /entities", "PUT /entities/:id", "DELETE /entities/:id"]}
pipeline = ["func.validateRequest", "func.getEntity", "func.serializeResponse"]
```

**Example:** `examples/patterns/rest_api.toml`

**Common mistakes:**
- ❌ Forgetting Error schema in output
- ❌ Not matching boundary.http paths to actual functions
- ❌ Missing validation step in pipeline

---

## Pattern 2: Database Access Layer

**Use when:** Abstracting database operations (repository pattern)

**Key characteristics:**
- Entity schemas representing tables
- CRUD operation functions
- Database boundary
- Optional connection pool schema

**Structure:**

```toml
# 1. Define entity (table schema)
[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", name = "string", email = "string", created_at = "timestamp"}

# 2. Define query parameter schemas
[schema.UserQuery]
kind = "node"
role = "query"
fields = {id = "uuid?", email = "string?"}

# 3. Define repository functions
[func.findUserById]
intent = "Find user by ID"
input = ["schema.UserQuery"]
output = ["schema.User", "schema.NotFound"]

[func.createUser]
intent = "Insert new user"
input = ["schema.User"]
output = ["schema.User", "schema.DbError"]

[func.updateUser]
intent = "Update existing user"
input = ["schema.User"]
output = ["schema.User", "schema.DbError"]

[func.deleteUser]
intent = "Delete user by ID"
input = ["schema.UserQuery"]
output = ["schema.Success", "schema.DbError"]

# 4. Define repository module
[mod.user_repository]
purpose = "User data access layer"
schemas = ["schema.User", "schema.UserQuery", "schema.DbError", "schema.NotFound"]
funcs = ["func.findUserById", "func.createUser", "func.updateUser", "func.deleteUser"]
boundary = {db = ["users"]}
```

**Example:** `examples/patterns/database.toml`

**Common patterns:**
- Optional fields in query schemas (use `?` suffix)
- Separate error types: `NotFound`, `DbError`, `ValidationError`
- Transaction boundary can be represented with `boundary.tx`

---

## Pattern 3: Event-Driven Processing

**Use when:** Processing asynchronous events from queues/streams

**Key characteristics:**
- Event schemas with metadata (timestamp, trace_id, etc.)
- Event handler functions
- Queue/topic boundaries
- Pipeline: parse → validate → process → emit

**Structure:**

```toml
# 1. Define event schemas
[schema.UserCreatedEvent]
kind = "node"
role = "event"
fields = {user_id = "uuid", email = "string", timestamp = "timestamp", trace_id = "string"}

[schema.WelcomeEmailEvent]
kind = "node"
role = "event"
fields = {user_id = "uuid", email = "string"}

# 2. Define handler functions
[func.handleUserCreated]
intent = "Handle user creation event"
input = ["schema.UserCreatedEvent"]
output = ["schema.WelcomeEmailEvent", "schema.Error"]

[func.sendWelcomeEmail]
intent = "Send welcome email to new user"
input = ["schema.WelcomeEmailEvent"]
output = ["schema.Success", "schema.Error"]

# 3. Define event processor module
[mod.user_event_processor]
purpose = "Process user lifecycle events"
schemas = ["schema.UserCreatedEvent", "schema.WelcomeEmailEvent"]
funcs = ["func.handleUserCreated", "func.sendWelcomeEmail"]
boundary = {queue = ["user.created", "email.welcome"]}
pipeline = ["func.handleUserCreated", "func.sendWelcomeEmail"]
```

**Example:** `examples/patterns/event_driven.toml`

**Best practices:**
- Include `trace_id` for distributed tracing
- Include `timestamp` for event ordering
- Use past-tense event names (`UserCreated`, not `CreateUser`)
- Design for idempotency

---

## Pattern 4: Data Pipeline

**Use when:** ETL (Extract, Transform, Load) workflows

**Key characteristics:**
- Source and destination schemas
- Transformation functions
- Clear pipeline flow
- Multiple intermediate schemas

**Structure:**

```toml
# 1. Define source schema
[schema.RawData]
kind = "node"
role = "input"
fields = {raw = "string", timestamp = "timestamp"}

# 2. Define intermediate schemas
[schema.ParsedData]
kind = "node"
role = "intermediate"
fields = {id = "uuid", value = "float", timestamp = "timestamp"}

[schema.EnrichedData]
kind = "node"
role = "intermediate"
fields = {id = "uuid", value = "float", category = "string", timestamp = "timestamp"}

# 3. Define destination schema
[schema.AggregatedData]
kind = "node"
role = "output"
fields = {category = "string", sum = "float", count = "int", date = "date"}

# 4. Define transformation functions
[func.parse]
intent = "Parse raw data into structured format"
input = ["schema.RawData"]
output = ["schema.ParsedData", "schema.ParseError"]

[func.enrich]
intent = "Enrich data with additional context"
input = ["schema.ParsedData"]
output = ["schema.EnrichedData"]

[func.aggregate]
intent = "Aggregate data by category"
input = ["schema.EnrichedData"]
output = ["schema.AggregatedData"]

# 5. Define pipeline module
[mod.data_pipeline]
purpose = "ETL pipeline for data processing"
schemas = ["schema.RawData", "schema.ParsedData", "schema.EnrichedData", "schema.AggregatedData"]
funcs = ["func.parse", "func.enrich", "func.aggregate"]
pipeline = ["func.parse", "func.enrich", "func.aggregate"]
boundary = {input = ["s3://raw-data"], output = ["s3://processed-data"]}
```

**Example:** `examples/patterns/data_pipeline.toml`

**Tips:**
- Use `role` to track data flow: `input` → `intermediate` → `output`
- Each transformation should be a pure function
- Consider error handling at each stage

---

## Pattern 5: GraphQL API

**Use when:** Building GraphQL API with nested resolvers

**Key characteristics:**
- Type schemas matching GraphQL schema
- Resolver functions for each field
- Connection/Edge types for pagination
- Query/Mutation/Subscription separation

**Structure:**

```toml
# 1. Define GraphQL types
[schema.User]
kind = "node"
role = "type"
fields = {id = "ID", name = "String", email = "String", posts = "[Post]"}

[schema.Post]
kind = "node"
role = "type"
fields = {id = "ID", title = "String", author = "User", comments = "[Comment]"}

[schema.UserConnection]
kind = "node"
role = "connection"
fields = {edges = "[UserEdge]", pageInfo = "PageInfo"}

# 2. Define resolver functions
[func.resolveUser]
intent = "Resolve user field"
input = ["schema.QueryContext"]
output = ["schema.User"]

[func.resolveUserPosts]
intent = "Resolve user.posts field"
input = ["schema.User"]
output = ["[schema.Post]"]

# 3. Define query/mutation modules
[mod.user_queries]
purpose = "User query resolvers"
schemas = ["schema.User", "schema.UserConnection"]
funcs = ["func.resolveUser", "func.resolveUsers"]
boundary = {graphql = ["Query.user", "Query.users"]}

[mod.user_mutations]
purpose = "User mutation resolvers"
schemas = ["schema.User", "schema.CreateUserInput"]
funcs = ["func.createUser", "func.updateUser"]
boundary = {graphql = ["Mutation.createUser", "Mutation.updateUser"]}
```

**Example:** `examples/patterns/graphql_api.toml`

**GraphQL-specific notes:**
- Use `Connection` and `Edge` types for pagination
- Separate Query, Mutation, and Subscription modules
- Include `Context` schema for resolver context

---

## Pattern 6: Authentication & Authorization

**Use when:** Implementing auth flows

**Key characteristics:**
- User/Session/Token schemas
- Auth functions (login, verify, refresh)
- Permission checking
- Middleware pattern in pipeline

**Structure:**

```toml
# 1. Define auth schemas
[schema.Credentials]
kind = "node"
role = "input"
fields = {email = "string", password = "string"}

[schema.User]
kind = "node"
role = "entity"
fields = {id = "uuid", email = "string", password_hash = "string", roles = "[string]"}

[schema.Session]
kind = "node"
role = "entity"
fields = {user_id = "uuid", token = "string", expires_at = "timestamp"}

[schema.AuthToken]
kind = "node"
role = "output"
fields = {access_token = "string", refresh_token = "string", expires_in = "int"}

# 2. Define auth functions
[func.login]
intent = "Authenticate user and create session"
input = ["schema.Credentials"]
output = ["schema.AuthToken", "schema.AuthError"]

[func.verifyToken]
intent = "Verify JWT token and return user"
input = ["schema.AuthToken"]
output = ["schema.User", "schema.AuthError"]

[func.checkPermission]
intent = "Check if user has required permission"
input = ["schema.User", "schema.Permission"]
output = ["schema.Authorized", "schema.Forbidden"]

# 3. Define auth module
[mod.authentication]
purpose = "User authentication and authorization"
schemas = ["schema.Credentials", "schema.User", "schema.Session", "schema.AuthToken"]
funcs = ["func.login", "func.verifyToken", "func.checkPermission", "func.refreshToken"]
boundary = {http = ["POST /auth/login", "POST /auth/refresh"]}
pipeline = ["func.validateCredentials", "func.login", "func.createSession"]
```

**Example:** `examples/patterns/auth.toml`

**Security considerations:**
- Never include `password` or `password_hash` in output schemas
- Use separate `Credentials` schema for input
- Include token expiration handling
- Consider rate limiting (can be modeled as a function)

---

## Pattern 7: Background Job Processor

**Use when:** Processing long-running or scheduled tasks

**Key characteristics:**
- Job schemas with status tracking
- Worker functions
- Queue boundaries
- Retry logic representation

**Structure:**

```toml
# 1. Define job schemas
[schema.Job]
kind = "node"
role = "entity"
fields = {id = "uuid", type = "string", payload = "json", status = "enum", retries = "int", created_at = "timestamp"}

[schema.EmailJob]
kind = "node"
role = "job"
fields = {to = "string", subject = "string", body = "string"}

[schema.JobResult]
kind = "node"
role = "output"
fields = {job_id = "uuid", status = "enum", completed_at = "timestamp", error = "string?"}

# 2. Define worker functions
[func.processEmailJob]
intent = "Process email sending job"
input = ["schema.EmailJob"]
output = ["schema.JobResult"]

[func.retryJob]
intent = "Retry failed job with exponential backoff"
input = ["schema.Job"]
output = ["schema.Job"]

# 3. Define worker module
[mod.email_worker]
purpose = "Background email job processor"
schemas = ["schema.Job", "schema.EmailJob", "schema.JobResult"]
funcs = ["func.processEmailJob", "func.retryJob", "func.markComplete"]
boundary = {queue = ["jobs.email"]}
pipeline = ["func.processEmailJob", "func.markComplete"]
```

**Example:** `examples/patterns/background_jobs.toml`

**Job processing patterns:**
- Include retry count and status in Job schema
- Use separate schemas for different job types
- Model retry logic as a function
- Consider dead letter queue (DLQ) boundary

---

## Pattern 8: Microservice Integration

**Use when:** Calling external services or APIs

**Key characteristics:**
- External API schemas
- Client functions with timeouts
- Circuit breaker pattern
- Fallback mechanisms

**Structure:**

```toml
# 1. Define external API schemas
[schema.PaymentRequest]
kind = "node"
role = "request"
fields = {amount = "float", currency = "string", customer_id = "string"}

[schema.PaymentResponse]
kind = "node"
role = "response"
fields = {transaction_id = "string", status = "enum", timestamp = "timestamp"}

[schema.PaymentServiceError]
kind = "node"
role = "error"
fields = {code = "string", message = "string", retryable = "bool"}

# 2. Define client functions
[func.processPayment]
intent = "Call payment service to process transaction"
input = ["schema.PaymentRequest"]
output = ["schema.PaymentResponse", "schema.PaymentServiceError"]

[func.checkCircuitBreaker]
intent = "Check if circuit breaker allows request"
input = []
output = ["schema.Allowed", "schema.CircuitOpen"]

[func.fallbackPayment]
intent = "Fallback when payment service unavailable"
input = ["schema.PaymentRequest"]
output = ["schema.QueuedPayment"]

# 3. Define integration module
[mod.payment_integration]
purpose = "Payment service integration client"
schemas = ["schema.PaymentRequest", "schema.PaymentResponse", "schema.PaymentServiceError"]
funcs = ["func.processPayment", "func.checkCircuitBreaker", "func.fallbackPayment"]
boundary = {http = ["https://api.payment-service.com"]}
pipeline = ["func.checkCircuitBreaker", "func.processPayment", "func.fallbackPayment"]
requires = ["mod.circuit_breaker"]
```

**Example:** `examples/patterns/microservice_integration.toml`

**Integration best practices:**
- Model timeouts and retries as functions
- Include circuit breaker pattern
- Define fallback behavior
- Use `requires` for shared modules (e.g., circuit breaker)

---

## Using Patterns with LLMs

### Prompt Template

```
I need to build a [PATTERN NAME] for [DESCRIPTION].

Please use the [PATTERN NAME] pattern from the Surv IR Pattern Catalog.

Requirements:
- [Requirement 1]
- [Requirement 2]

Generate the Surv IR file following the pattern structure, then validate it with `surc check`.
```

### Example Prompt

```
I need to build a REST API Module for a book library system.

Please use the REST API Module pattern from the Surv IR Pattern Catalog.

Requirements:
- Books have: id, title, author, isbn, published_year
- Support CRUD operations: get, create, update, delete
- Endpoints: GET /books/:id, POST /books, PUT /books/:id, DELETE /books/:id

Generate the Surv IR file following the pattern structure, then validate it with `surc check`.
```

### Combining Patterns

Patterns can be combined by using multiple modules:

```toml
# REST API calls Database Layer
[mod.user_api]
# ... REST API pattern
requires = ["mod.user_repository"]

[mod.user_repository]
# ... Database Layer pattern
```

```toml
# Event handler calls Microservice Integration
[mod.order_event_processor]
# ... Event-Driven pattern
requires = ["mod.payment_integration"]

[mod.payment_integration]
# ... Microservice Integration pattern
```

---

## Pattern Selection Guide

| Scenario | Recommended Pattern | Secondary Pattern |
|----------|-------------------|-------------------|
| HTTP endpoints | REST API | GraphQL API |
| Database access | Database Layer | - |
| Message queue consumer | Event-Driven | Background Job |
| ETL workflow | Data Pipeline | - |
| User login | Authentication | REST API |
| Scheduled tasks | Background Job | - |
| External API call | Microservice Integration | - |
| GraphQL server | GraphQL API | Database Layer |

---

## Next Steps

1. Browse `examples/patterns/` for complete examples
2. Copy a pattern and customize for your needs
3. Validate with `surc check <file>`
4. Visualize with `surc export pipeline <file> <module>`

For more details, see:
- [Surv IR Specification](Spec-Surv-IR-v1.1.md)
- [Export Guide](EXPORT_GUIDE.md)
- [Deploy IR Guide](DEPLOY_IR_GUIDE.md)
