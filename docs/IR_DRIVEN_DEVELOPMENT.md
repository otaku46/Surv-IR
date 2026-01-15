# IR-Driven Development

This guide describes how to keep Surv IR and implementation in sync during development.

## The Problem

When implementing from Surv IR, you often discover:
- Edge cases not in the original design
- GUI adjustments and tweaks
- Performance optimizations
- Error handling details

If you only update the code, **IR diverges from reality**.

## Solution: Continuous IR Updates

### Workflow

```
1. Write Surv IR (design)
   ↓
2. surc check (validate)
   ↓
3. surc export (visualize & review)
   ↓
4. Implement code
   ↓
5. Discover missing cases → UPDATE IR (not just code!)
   ↓
6. surc check (validate updated IR)
   ↓
7. Repeat
```

### Example: Adding Error Handling

**Initial IR (incomplete):**
```toml
[func.createUser]
intent = "Create new user"
input = ["schema.UserRequest"]
output = ["schema.User"]
```

**During implementation, you discover:**
- Email might be duplicate → need `DuplicateEmailError`
- Password might be weak → need `WeakPasswordError`
- Database might fail → need `DatabaseError`

**✅ Update IR immediately:**
```toml
[schema.DuplicateEmailError]
kind = "node"
role = "error"
fields = {email = "string", message = "string"}

[schema.WeakPasswordError]
kind = "node"
role = "error"
fields = {message = "string", requirements = "[string]"}

[schema.DatabaseError]
kind = "node"
role = "error"
fields = {message = "string", code = "string"}

[func.createUser]
intent = "Create new user with validation"
input = ["schema.UserRequest"]
output = ["schema.User", "schema.DuplicateEmailError", "schema.WeakPasswordError", "schema.DatabaseError"]
```

Now run:
```bash
surc check user_api.toml
surc export pipeline user_api.toml user_module
```

The visualization now shows error paths!

### Example: GUI Adjustments

**Scenario:** Implementing an IDE, you realize the "Save File" function needs:
- Unsaved changes dialog
- Auto-format option
- Backup creation

**Don't just add code - update IR:**

```toml
[schema.UnsavedChangesDialog]
kind = "node"
role = "ui"
fields = {message = "string", options = "[string]"}

[schema.SaveOptions]
kind = "node"
role = "input"
fields = {auto_format = "bool", create_backup = "bool"}

[func.showUnsavedDialog]
intent = "Show unsaved changes confirmation"
input = ["schema.Document"]
output = ["schema.UserChoice"]

[func.formatDocument]
intent = "Format document before saving"
input = ["schema.Document"]
output = ["schema.Document"]

[func.createBackup]
intent = "Create backup file"
input = ["schema.Document"]
output = ["schema.BackupFile"]

[func.saveFile]
intent = "Save file with optional formatting and backup"
input = ["schema.Document", "schema.SaveOptions"]
output = ["schema.Success", "schema.FileSystemError"]

[mod.file_operations]
purpose = "File save operations with UI"
schemas = ["schema.Document", "schema.SaveOptions", "schema.UnsavedChangesDialog", "schema.BackupFile"]
funcs = ["func.showUnsavedDialog", "func.formatDocument", "func.createBackup", "func.saveFile"]
pipeline = ["func.showUnsavedDialog", "func.formatDocument", "func.createBackup", "func.saveFile"]
```

## IR Annotations for Implementation Details

### Using Comments

For implementation-specific notes:
```toml
[func.renderEditor]
intent = "Render editor canvas with syntax highlighting"
input = ["schema.Document", "schema.Theme"]
output = ["schema.Canvas"]
# Implementation note: Uses WebGL for performance
# TODO: Add line number caching
# Performance: Should render <16ms for 60fps
```

### Using Metadata

For structured metadata:
```toml
[func.searchFiles]
intent = "Search files by content"
input = ["schema.SearchQuery"]
output = ["schema.SearchResults"]

[func.searchFiles.meta]
algorithm = "ripgrep"
performance = "O(n) where n = total file size"
max_files = 10000
timeout_ms = 5000
```

## Tracking Implementation Status

### Option 1: Add Status in IR

```toml
[mod.editor_core]
purpose = "Core editor functionality"
schemas = ["schema.Document", "schema.Selection", "schema.Cursor"]
funcs = ["func.insertText", "func.deleteText", "func.moveCursor"]
pipeline = ["func.insertText"]

[mod.editor_core.status]
implemented = ["func.insertText", "func.deleteText"]
in_progress = ["func.moveCursor"]
not_started = []
```

### Option 2: Use External Progress File

`progress.toml`:
```toml
[implementation]
project = "ide"
last_updated = "2024-01-13"

[[completed]]
module = "mod.editor_core"
functions = ["func.insertText", "func.deleteText"]
tests_passing = true

[[in_progress]]
module = "mod.lsp_client"
functions = ["func.sendRequest"]
blockers = ["Need to implement JSON-RPC framing"]

[[not_started]]
module = "mod.debugger"
reason = "Waiting for DAP spec review"
```

Then:
```bash
surc export modules surv.toml > architecture.md
# Manually annotate with progress.toml data
```

### Option 3: Generate Progress Report

New command idea:
```bash
surc progress surv.toml progress.toml
# Output:
# Module Coverage:
#   editor_core: 2/3 functions (66%)
#   lsp_client:  1/5 functions (20%)
#   debugger:    0/8 functions (0%)
# Overall: 3/16 functions (18%)
```

## Best Practices

### 1. Update IR Before Committing Code

```bash
# Bad workflow
git add src/user.rs
git commit -m "Add duplicate email check"

# Good workflow
vim api/user.toml  # Update IR first
surc check api/user.toml
git add api/user.toml src/user.rs
git commit -m "Add duplicate email check

Updated user.toml to include DuplicateEmailError schema"
```

### 2. Use IR for Code Review

Reviewer checklist:
- [ ] Does PR include IR changes?
- [ ] Does `surc check` pass?
- [ ] Does visualization match description?
- [ ] Are new error cases in IR?

### 3. Generate Implementation Checklist from IR

```bash
# Future command idea
surc todo surv.toml > implementation-checklist.md

# Output:
# ## Modules
# - [ ] mod.editor_core (3 functions)
#   - [ ] func.insertText
#   - [ ] func.deleteText
#   - [ ] func.moveCursor
# - [ ] mod.lsp_client (5 functions)
#   - [ ] func.initialize
#   ...
```

### 4. Detect IR-Code Drift

```bash
# Future command idea
surc diff surv.toml src/

# Output:
# Warning: func.createUser exists in IR but not found in src/
# Warning: handleError() exists in src/user.rs but not in IR
```

## IR Evolution Pattern

As you implement:

```
Version 1: Basic IR (design phase)
  ↓ implement & discover
Version 2: IR + error handling
  ↓ implement & discover
Version 3: IR + error handling + performance notes
  ↓ implement & discover
Version 4: IR + error handling + performance + edge cases
```

Keep committing IR updates alongside code:
```
git log --oneline
abc1234 Add retry logic for network errors (IR + code)
def5678 Optimize rendering with canvas caching (IR + code)
ghi9012 Handle empty document edge case (IR + code)
```

## Conclusion

**IR is not just initial design - it's living documentation.**

When you discover something during implementation:
1. ✅ Update IR
2. ✅ Validate with `surc check`
3. ✅ Visualize with `surc export`
4. ✅ Commit IR + Code together

This way, IR remains the **single source of truth** for architecture.
