# Expected output for test_split

This directory contains the expected output structure after running:

```bash
surc split test_split_input.toml --config test_split_config.toml
```

## Expected structure:

```
test_split_output/
├── surv.toml
└── design/
    ├── common/
    │   └── auth.toml
    ├── backend/
    │   ├── user.toml
    │   └── order.toml
    └── frontend/
        └── ui.toml
```

## Expected content validation:

1. **surv.toml** should contain:
   - project.name = "test-split-project"
   - 3 packages: common, backend, frontend
   - Correct dependency graph: frontend → backend → common

2. **design/common/auth.toml** should contain:
   - package = "common"
   - namespace = "app.common"
   - mod.auth
   - schema.Token, schema.TokenValidation (dependency closure)
   - func.validateToken

3. **design/backend/user.toml** should contain:
   - package = "backend"
   - namespace = "app.api"
   - require = ["mod.auth"]
   - mod.user_api
   - schema.User, schema.UserRequest
   - func.createUser, func.getUser
   - schema.Token, schema.TokenValidation (copied due to shared_symbols=copy)
   - func.validateToken (dependency closure)

4. **design/backend/order.toml** should contain:
   - package = "backend"
   - namespace = "app.api"
   - require = ["mod.auth"]
   - mod.order_api
   - schema.Order, schema.OrderRequest
   - func.createOrder, func.getOrder
   - schema.Token, schema.TokenValidation (copied)
   - func.validateToken (dependency closure)

5. **design/frontend/ui.toml** should contain:
   - package = "frontend"
   - namespace = "app.ui"
   - require = ["mod.user_api"]
   - mod.user_ui
   - schema.UserView, schema.UI
   - func.renderUserList

## Warnings expected:

- W_SHARED_SYMBOL_COPIED for schema.Token, schema.TokenValidation, func.validateToken
  (copied to both user.toml and order.toml)

## Test validation:

After split, should pass:
```bash
surc project-check test_split_output/surv.toml
```
