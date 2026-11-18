# Package Management & File Splitting Proposal

## Current Situation

### Existing Features

✅ **Already Available:**
- `surc project-check surv.toml` - Validates multi-file projects
- `surc export modules surv.toml` - Shows module dependency graph
- `surc export schemas surv.toml` - Shows schema relationships
- Package system with namespace support

❌ **Missing:**
- Command to split large IR files into packages
- Package-level dependency visualization
- Package-scoped module inspection

---

## Problem Statement

### When IR Files Get Too Large

```toml
# design/monolith.toml (1000+ lines)
[schema.User]
...
[schema.Post]
...
[schema.Comment]
...
# ... 50+ schemas
# ... 100+ functions
# ... 20+ modules
```

**Problems:**
1. Hard to navigate
2. Slow to parse
3. Difficult to collaborate (git conflicts)
4. Can't separate concerns

---

## Proposed Solutions

### Solution 1: `surc split` Command

**Automatically split a large IR file into packages.**

#### Usage

```bash
# Split by module
surc split design/monolith.toml --by-module --output design/

# Output:
# design/
#   ├── user/
#   │   └── user.toml        # mod.user_api, related schemas/funcs
#   ├── post/
#   │   └── post.toml        # mod.post_api, related schemas/funcs
#   └── common/
#       └── common.toml      # Shared schemas

# Generate manifest
surc split design/monolith.toml --by-module --output design/ --generate-manifest

# Output:
# surv.toml (manifest)
# design/user/user.toml
# design/post/post.toml
# design/common/common.toml
```

#### Split Strategies

**Strategy 1: By Module** (Default)
```bash
surc split design/api.toml --by-module
```
- Each `mod.*` becomes a separate file
- Related schemas/funcs included
- Shared items go to `common/`

**Strategy 2: By Domain**
```bash
surc split design/api.toml --by-domain --domains "user,post,auth"
```
- User specifies domain groupings
- Schemas/funcs/mods grouped by domain

**Strategy 3: By Namespace**
```bash
surc split design/api.toml --by-namespace
```
- Already has `namespace = "X"` declarations
- Split based on existing namespaces

#### Implementation Sketch

```rust
// src/split_commands.rs
pub fn run_split(args: &[String]) -> Result<(), Box<dyn Error>> {
    let input_file = &args[0];
    let strategy = parse_strategy(args); // --by-module, --by-domain, etc.
    let output_dir = parse_output_dir(args); // --output design/

    // 1. Parse input file
    let ast = parse_file(input_file)?;

    // 2. Group sections by strategy
    let groups = match strategy {
        Strategy::ByModule => group_by_module(&ast),
        Strategy::ByDomain(domains) => group_by_domain(&ast, domains),
        Strategy::ByNamespace => group_by_namespace(&ast),
    };

    // 3. Write each group to separate file
    for (name, sections) in groups {
        write_ir_file(&output_dir.join(format!("{}.toml", name)), sections)?;
    }

    // 4. Generate manifest if requested
    if generate_manifest {
        generate_surv_toml(&output_dir, &groups)?;
    }

    Ok(())
}

fn group_by_module(ast: &SurvFile) -> HashMap<String, Vec<Section>> {
    let mut groups: HashMap<String, Vec<Section>> = HashMap::new();
    let mut common = Vec::new();

    // Find all modules
    let modules: Vec<&ModSection> = ast.sections.iter()
        .filter_map(|s| if let Section::Mod(m) = s { Some(m) } else { None })
        .collect();

    for module in modules {
        let mut group = Vec::new();

        // Add module itself
        group.push(Section::Mod(module.clone()));

        // Add referenced schemas
        for schema_ref in &module.schemas {
            if let Some(schema) = find_schema(ast, schema_ref) {
                group.push(Section::Schema(schema.clone()));
            }
        }

        // Add referenced functions
        for func_ref in &module.funcs {
            if let Some(func) = find_func(ast, func_ref) {
                group.push(Section::Func(func.clone()));
            }
        }

        groups.insert(module.name.clone(), group);
    }

    // Find orphaned schemas/funcs (not referenced by any module)
    // Put them in "common"

    groups.insert("common".to_string(), common);
    groups
}
```

---

### Solution 2: `surc deps` Command

**Show package and module dependencies.**

#### Usage

```bash
# Show all package dependencies
surc deps surv.toml

# Output:
# Packages:
#   frontend (namespace: app.ui)
#     └─> backend
#   backend (namespace: app.api)
#     └─> common
#   common (namespace: app.common)

# Show modules in a package
surc deps surv.toml --package backend

# Output:
# Package: backend (app.api)
# Modules:
#   mod.user_api
#     └─> mod.auth (from common)
#   mod.post_api
#     └─> mod.user_api
#     └─> mod.auth (from common)

# Show dependencies for specific module
surc deps surv.toml --module mod.user_api

# Output:
# mod.user_api (in backend package)
# Dependencies:
#   └─> mod.auth (in common package)
# Dependents:
#   └─> mod.post_api (in backend package)

# Show cross-package dependencies only
surc deps surv.toml --cross-package

# Output:
# Cross-package dependencies:
#   frontend.mod.ui → backend.mod.user_api
#   backend.mod.user_api → common.mod.auth

# Export as graph
surc deps surv.toml --format mermaid > deps.md
surc deps surv.toml --format dot > deps.dot
```

#### Implementation Sketch

```rust
// src/deps_commands.rs
pub fn run_deps(args: &[String]) -> Result<(), Box<dyn Error>> {
    let manifest_path = &args[0];
    let options = parse_options(args);

    // Load project
    let manifest = load_manifest(manifest_path)?;
    let project = load_project(&manifest)?;

    match options.scope {
        Scope::Packages => show_package_deps(&manifest, &project),
        Scope::Package(name) => show_package_modules(&manifest, &project, &name),
        Scope::Module(name) => show_module_deps(&project, &name),
        Scope::CrossPackage => show_cross_package_deps(&manifest, &project),
    }
}

fn show_package_deps(manifest: &Manifest, project: &ProjectAST) {
    println!("Packages:");

    for (pkg_name, pkg) in &manifest.packages {
        println!("  {} (namespace: {})", pkg_name, pkg.namespace.as_deref().unwrap_or(""));

        for dep in &pkg.depends {
            println!("    └─> {}", dep);
        }
    }
}

fn show_module_deps(project: &ProjectAST, module_name: &str) {
    let normalized_reqs = project.collect_normalized_requires();

    // Find dependencies (modules this module requires)
    let deps: Vec<_> = normalized_reqs.iter()
        .filter(|req| req.from_mod == module_name)
        .collect();

    // Find dependents (modules that require this module)
    let dependents: Vec<_> = normalized_reqs.iter()
        .filter(|req| req.to_mod == module_name)
        .collect();

    println!("{}", module_name);

    if !deps.is_empty() {
        println!("Dependencies:");
        for dep in deps {
            println!("  └─> {}", dep.to_mod);
        }
    }

    if !dependents.is_empty() {
        println!("Dependents:");
        for dependent in dependents {
            println!("  └─> {}", dependent.from_mod);
        }
    }
}

fn show_cross_package_deps(manifest: &Manifest, project: &ProjectAST) {
    // Build package mapping (module -> package)
    let mut module_to_package: HashMap<String, String> = HashMap::new();

    for (pkg_name, pkg) in &manifest.packages {
        let pkg_files = find_files_in_package(pkg);
        for file_path in pkg_files {
            let ast = parse_file(&file_path)?;
            for section in &ast.sections {
                if let Section::Mod(m) = section {
                    module_to_package.insert(format!("mod.{}", m.name), pkg_name.clone());
                }
            }
        }
    }

    // Find cross-package edges
    let normalized_reqs = project.collect_normalized_requires();

    println!("Cross-package dependencies:");
    for req in normalized_reqs {
        let from_pkg = module_to_package.get(&req.from_mod);
        let to_pkg = module_to_package.get(&req.to_mod);

        if from_pkg != to_pkg && from_pkg.is_some() && to_pkg.is_some() {
            println!("  {}.{} → {}.{}",
                from_pkg.unwrap(), req.from_mod,
                to_pkg.unwrap(), req.to_mod);
        }
    }
}
```

---

### Solution 3: Enhanced `surc inspect`

**Add package-aware inspection.**

```bash
# Current (file-scoped)
surc inspect mod.user_api design/user.toml

# New: Package-scoped
surc inspect mod.user_api surv.toml --show-package

# Output:
# Module: mod.user_api
# Package: backend (namespace: app.api)
# File: design/backend/user.toml
#
# Purpose: User management API
#
# Schemas (2):
#   - app.api.schema.User (local)
#   - app.common.schema.Auth (from common package)
#
# Functions (3):
#   - app.api.func.createUser (local)
#   - app.api.func.getUser (local)
#
# Dependencies:
#   - mod.auth (common package)
```

---

## Workflow Examples

### Example 1: Split Large File

**Before:**
```
project/
├── design.toml (2000 lines)
└── src/
```

**Split:**
```bash
surc split design.toml --by-module --output design/ --generate-manifest
```

**After:**
```
project/
├── surv.toml (manifest)
├── design/
│   ├── user/
│   │   └── user.toml
│   ├── post/
│   │   └── post.toml
│   ├── auth/
│   │   └── auth.toml
│   └── common/
│       └── common.toml
└── src/
```

**Generated surv.toml:**
```toml
[project]
name = "my-app"

[paths]
ir_root = "design"

[packages.user]
root = "design/user"
namespace = "app.user"
depends = ["common", "auth"]

[packages.post]
root = "design/post"
namespace = "app.post"
depends = ["common", "user"]

[packages.auth]
root = "design/auth"
namespace = "app.auth"
depends = ["common"]

[packages.common]
root = "design/common"
namespace = "app.common"
```

### Example 2: Understand Dependencies

```bash
# Overview
surc deps surv.toml

# Detailed package view
surc deps surv.toml --package backend

# Specific module
surc deps surv.toml --module mod.user_api

# Cross-package only
surc deps surv.toml --cross-package

# Visualize
surc deps surv.toml --format mermaid > deps.md
```

### Example 3: Package-Aware Development

```bash
# 1. Check which package a module belongs to
surc inspect mod.user_api surv.toml --show-package

# 2. See all modules in a package
surc deps surv.toml --package backend

# 3. Find cross-package dependencies
surc deps surv.toml --cross-package

# 4. Validate entire project
surc project-check surv.toml
```

---

## Implementation Priority

### Phase 1: `surc deps` (High Priority)

**Why first:**
- Users need visibility into existing structure
- Helps understand current dependencies
- Foundation for splitting decisions

**Estimated effort:** 1-2 days

**Commands:**
```bash
surc deps surv.toml                      # Package overview
surc deps surv.toml --package <name>     # Package modules
surc deps surv.toml --module <name>      # Module dependencies
surc deps surv.toml --cross-package      # Cross-package edges
```

### Phase 2: Enhanced `surc inspect` (Medium Priority)

**Why second:**
- Natural extension of existing command
- Complements `surc deps`

**Estimated effort:** 1 day

**Enhancement:**
```bash
surc inspect mod.user_api surv.toml --show-package
```

### Phase 3: `surc split` (Lower Priority)

**Why last:**
- More complex (file generation, validation)
- Can be done manually once deps are visible
- Nice-to-have automation

**Estimated effort:** 2-3 days

**Commands:**
```bash
surc split design.toml --by-module --output design/
surc split design.toml --by-domain --domains "user,post,auth"
```

---

## Manual Workflow (Today)

**Without new commands, you can still split manually:**

### Step 1: Identify Modules

```bash
# List all modules
surc parse design.toml | jq '.sections[] | select(.Mod) | .Mod.name'
```

### Step 2: Extract Module Groups

For each module, manually create a file with:
- The module definition
- Referenced schemas
- Referenced functions

### Step 3: Create Manifest

```toml
[project]
name = "my-app"

[paths]
ir_root = "design"

[packages.package1]
root = "design/package1"
namespace = "app.package1"
```

### Step 4: Validate

```bash
surc project-check surv.toml
```

---

## Recommendations

### For Your Plasm Project

If `plasm_ide_design.toml` is getting large:

**Option 1: Manual Split (Now)**

```
plasm/
├── surv.toml
└── ir/
    ├── ui/
    │   ├── workspace.toml
    │   ├── editor.toml
    │   └── chat.toml
    ├── backend/
    │   ├── file_ops.toml
    │   └── tauri_commands.toml
    └── common/
        └── types.toml
```

**Option 2: Wait for `surc deps` (Soon)**

```bash
# Understand current structure
surc deps surv.toml

# Find natural split points
surc deps surv.toml --cross-package
```

**Option 3: Wait for `surc split` (Later)**

```bash
# Automatic split
surc split ir/plasm_ide_design.toml --by-module --output ir/
```

---

## Summary

### ✅ Existing (Use Today)
- `surc project-check surv.toml`
- `surc export modules surv.toml`
- Manual file splitting

### 🆕 Proposed (Implement)

**Phase 1: `surc deps`** (High priority)
- Package dependencies
- Module dependencies
- Cross-package analysis

**Phase 2: Enhanced `surc inspect`** (Medium priority)
- Package-aware inspection

**Phase 3: `surc split`** (Nice-to-have)
- Automated file splitting

---

## Next Steps

1. **Implement `surc deps`** - Most useful immediately
2. **Document manual splitting** - For urgent needs
3. **Consider `surc split`** - If demand is high

Would you like me to implement `surc deps` first?
