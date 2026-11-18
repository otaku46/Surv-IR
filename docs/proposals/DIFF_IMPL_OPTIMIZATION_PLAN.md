# diff-impl Optimization Plan

## Problem Analysis

### Current Issue: LSP Overhead

Every `surc diff-impl` invocation:
1. Spawns new LSP server process (rust-analyzer / typescript-language-server)
2. Initializes LSP connection
3. **Indexes entire workspace** (30-60 seconds for medium projects)
4. Queries symbols
5. Shuts down LSP server

**Result:** Prohibitively slow for regular use.

---

## Solution Approaches

### Approach 1: Symbol Cache (Recommended First)

**Cache LSP query results to disk.**

#### Design

```rust
// src/diff_impl/cache.rs
pub struct SymbolCache {
    workspace_root: PathBuf,
    timestamp: SystemTime,
    symbols: HashMap<String, Vec<FoundSymbol>>, // "rust" | "ts"
}

impl SymbolCache {
    pub fn load(cache_path: &Path) -> Option<Self>;
    pub fn save(&self, cache_path: &Path) -> Result<()>;
    pub fn is_fresh(&self, max_age: Duration) -> bool;
}
```

#### CLI Usage

```bash
# First run: query LSP and cache results
surc diff-impl design.toml workspace --cache

# Subsequent runs: use cache (instant!)
surc diff-impl design.toml workspace --use-cache

# Force refresh
surc diff-impl design.toml workspace --refresh-cache

# Custom cache location
surc diff-impl design.toml workspace --cache-file .surv/symbols.json --max-age 3600
```

#### Cache File Format

```json
{
  "version": "1.0",
  "workspace": "/Users/.../plasm/Plasm/src-tauri",
  "timestamp": "2026-01-15T08:30:00Z",
  "languages": {
    "rust": {
      "indexed_at": "2026-01-15T08:30:00Z",
      "symbols": [
        {
          "name": "FileTreeNode",
          "kind": "Struct",
          "uri": "file:///Users/.../main.rs",
          "range": {"start_line": 42, "start_char": 0, ...},
          "container_name": null
        },
        ...
      ]
    },
    "typescript": { ... }
  }
}
```

#### Implementation Steps

1. **Add cache module** (`src/diff_impl/cache.rs`)
2. **Modify matcher** to check cache first
3. **Add CLI flags** (`--cache`, `--use-cache`, `--refresh-cache`, `--max-age`)
4. **Default location**: `.surv/symbol-cache.json`

**Estimated effort:** 2-3 hours

**Benefits:**
- ✅ First run slow, but subsequent runs instant
- ✅ Simple to implement
- ✅ No background processes
- ✅ Works with existing LSP integration

---

### Approach 2: LSP Daemon (Most Powerful)

**Keep LSP servers running in background.**

#### Design

```bash
# Start daemon
surc lsp-daemon start rust /path/to/workspace
# → Spawns rust-analyzer, keeps it running
# → Returns daemon ID: "rust-abc123"

# Query via daemon (fast!)
surc diff-impl design.toml workspace --daemon rust-abc123

# Stop daemon
surc lsp-daemon stop rust-abc123

# List running daemons
surc lsp-daemon list
```

#### Architecture

```rust
// src/diff_impl/daemon.rs
pub struct LspDaemon {
    id: String,
    language: String,
    workspace_root: PathBuf,
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

// Keep process alive across invocations
// Use Unix socket or named pipe for IPC
```

#### Persistence

```json
// ~/.surv/daemons.json
{
  "daemons": [
    {
      "id": "rust-abc123",
      "language": "rust",
      "workspace": "/Users/.../plasm/src-tauri",
      "pid": 12345,
      "socket": "/tmp/surc-daemon-abc123.sock",
      "started_at": "2026-01-15T08:00:00Z"
    }
  ]
}
```

#### CLI Usage

```bash
# Workflow 1: Manual daemon management
surc lsp-daemon start rust ../plasm/Plasm/src-tauri
# Output: Started rust-analyzer daemon: rust-abc123

surc diff-impl design.toml ../plasm --daemon rust-abc123
# Fast! Uses existing daemon

surc lsp-daemon stop rust-abc123

# Workflow 2: Auto-daemon (convenience)
surc diff-impl design.toml workspace --auto-daemon
# Starts daemon if not running, reuses if exists
```

**Estimated effort:** 1-2 days

**Benefits:**
- ✅ Fastest possible (after initial startup)
- ✅ Works like IDE (same experience)
- ✅ Multiple workspaces simultaneously

**Challenges:**
- ⚠️ More complex (process management, IPC)
- ⚠️ Need cleanup on crash
- ⚠️ Platform-specific (Unix sockets vs Windows named pipes)

---

### Approach 3: Static Analysis (No LSP)

**Parse source files directly without LSP.**

#### Design

```rust
// src/diff_impl/static_analyzer.rs
pub fn extract_rust_symbols(workspace: &Path) -> Vec<FoundSymbol> {
    // Use 'syn' crate to parse Rust files
    // Find: pub struct, pub fn, pub enum, pub type
}

pub fn extract_ts_symbols(workspace: &Path) -> Vec<FoundSymbol> {
    // Use 'swc' or 'tree-sitter' to parse TypeScript
    // Find: export type, export interface, export function
}
```

#### Example (Rust with syn)

```rust
use syn::{File, Item};

fn parse_rust_file(path: &Path) -> Vec<Symbol> {
    let content = fs::read_to_string(path)?;
    let ast = syn::parse_file(&content)?;

    let mut symbols = Vec::new();
    for item in ast.items {
        match item {
            Item::Struct(s) if is_public(&s.vis) => {
                symbols.push(Symbol {
                    name: s.ident.to_string(),
                    kind: "Struct",
                    ...
                });
            }
            Item::Fn(f) if is_public(&f.vis) => { ... }
            _ => {}
        }
    }
    symbols
}
```

#### CLI Usage

```bash
# Use static analysis instead of LSP
surc diff-impl design.toml workspace --static

# Much faster, no LSP needed
```

**Estimated effort:** 3-4 days (need parser for each language)

**Benefits:**
- ✅ No LSP required (instant startup)
- ✅ Simple, predictable
- ✅ No daemon/cache complexity

**Limitations:**
- ⚠️ Only finds exported/public symbols
- ⚠️ May miss complex cases (macros, re-exports)
- ⚠️ Need separate parser per language

---

## Recommended Implementation Order

### Phase 1.5: Symbol Cache (Priority 1)

**Why first:**
- Simplest to implement
- Biggest immediate impact
- No architectural changes
- Backward compatible

**Implementation:**
1. Add `cache.rs` module
2. Modify `matcher.rs` to check cache
3. Add CLI flags
4. Document usage

**Timeline:** 1 day

---

### Phase 2: Static Analysis for Rust (Priority 2)

**Why:**
- No LSP dependency
- Instant results
- Good enough accuracy for most cases

**Implementation:**
1. Add `syn` dependency
2. Implement Rust parser
3. Add `--static` flag
4. Fallback to LSP if needed

**Timeline:** 2-3 days

---

### Phase 3: LSP Daemon (Priority 3 - Future)

**Why later:**
- Most complex
- Requires process management
- Platform-specific code

**When to implement:**
- After cache + static analysis proven
- If ultra-fast iteration needed
- For IDE-like workflows

**Timeline:** 1-2 weeks (including testing)

---

## Quick Win: Implement Cache Now

### Minimal Implementation (2-3 hours)

```rust
// src/diff_impl/cache.rs
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, Duration};

#[derive(Serialize, Deserialize)]
pub struct SymbolCache {
    workspace: String,
    timestamp: u64, // UNIX timestamp
    rust_symbols: Vec<FoundSymbol>,
    ts_symbols: Vec<FoundSymbol>,
}

impl SymbolCache {
    pub fn load_from_file(path: &Path) -> Result<Self, Box<dyn Error>> {
        let content = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn is_fresh(&self, max_age_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        now - self.timestamp < max_age_secs
    }
}
```

### CLI Integration

```rust
// src/diff_impl/matcher.rs
pub fn diff_impl(
    ir_file: &Path,
    workspace_root: &Path,
    filter_mod: Option<&str>,
    language: &str,
    use_cache: bool,
    cache_file: Option<&Path>,
    max_cache_age: u64,
) -> Result<DiffResult, Box<dyn Error>> {
    let expected = extract_expected_symbols(ir_file, filter_mod)?;

    // Check cache first
    let found = if use_cache {
        if let Some(cache_path) = cache_file {
            if let Ok(cache) = SymbolCache::load_from_file(cache_path) {
                if cache.is_fresh(max_cache_age) {
                    eprintln!("Using cached symbols (age: {}s)",
                        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() - cache.timestamp);

                    match language {
                        "rust" => cache.rust_symbols,
                        "ts" => cache.ts_symbols,
                        "both" => {
                            let mut all = cache.rust_symbols;
                            all.extend(cache.ts_symbols);
                            all
                        }
                        _ => vec![]
                    }
                } else {
                    eprintln!("Cache expired, querying LSP...");
                    query_and_cache(workspace_root, language, cache_path)?
                }
            } else {
                eprintln!("No cache found, querying LSP...");
                query_and_cache(workspace_root, language, cache_path)?
            }
        } else {
            query_workspace_symbols(workspace_root, language, &expected)?
        }
    } else {
        query_workspace_symbols(workspace_root, language, &expected)?
    };

    let result = match_symbols(&expected, &found, language);
    Ok(result)
}
```

### Usage

```bash
# First run (slow, creates cache)
surc diff-impl design.toml workspace --cache-file .surv/symbols.json --max-age 3600

# Second run (instant!)
surc diff-impl design.toml workspace --use-cache --cache-file .surv/symbols.json

# After code changes, refresh
surc diff-impl design.toml workspace --cache-file .surv/symbols.json --refresh
```

---

## Performance Comparison

| Approach | First Run | Subsequent Runs | Accuracy | Complexity |
|----------|-----------|-----------------|----------|------------|
| **Current (LSP each time)** | 30-60s | 30-60s | 100% | Low |
| **+ Symbol Cache** | 30-60s | <1s | 100% | Low |
| **+ Static Analysis** | <5s | <5s | ~90% | Medium |
| **+ LSP Daemon** | 30-60s | <1s | 100% | High |

---

## Conclusion

**Immediate Action: Implement Symbol Cache**

This gives us:
- ✅ 95% of the benefit (instant subsequent runs)
- ✅ 5% of the complexity (simple JSON cache)
- ✅ Works with existing LSP integration
- ✅ No breaking changes

**After cache is working, consider:**
- Static analysis for Rust (if LSP still too slow)
- LSP daemon (if interactive workflow needed)

**For Plasm IDE right now:**
```bash
# First run
surc diff-impl ir/plasm_ide_design.toml ../Plasm --cache
# Wait 30-60s

# Development workflow (instant!)
# 1. Make code changes
# 2. Check drift
surc diff-impl ir/plasm_ide_design.toml ../Plasm --use-cache
# 3. If symbols added/removed, refresh cache
surc diff-impl ir/plasm_ide_design.toml ../Plasm --refresh-cache
```

This makes diff-impl **practical for daily use**! 🚀
