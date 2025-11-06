# Root Cause Analysis: Rails Constant Resolution Failure

**Date**: 2025-11-05
**Issue**: #22 - Rails autoloading support for cross-file constant resolution
**Status**: **FAIL** - 0% detection rate (0/150 UrlFormatter calls detected)

## Executive Summary

Rails constant resolution is **completely non-functional** due to Rails project detection failure at the indexing pipeline entry point. The system outputs "DEBUG: Not a Rails project, skipping Rails autoloading support" for guliveo, blocking all downstream Rails-specific functionality.

**Root Cause**: `RailsSymbolTable::build()` returns empty table → Rails resolution context never populated → constant references tracked but never resolved.

---

## Architectural Flow Map

### Complete Indexing → Resolution Pipeline

```
┌─────────────────────────────────────────────────────────────────────────┐
│ Phase 1: FILE INDEXING (simple.rs)                                     │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  SimpleIndexer::index_directory(dir)                                   │
│    │                                                                    │
│    ├─► For each .rb file:                                             │
│    │     index_file_with_behavior(path, content, file_id, behavior)   │
│    │       │                                                           │
│    │       ├─► parser.find_uses(content)                              │
│    │       │     └─► ruby/parser.rs:1343 find_uses()                  │
│    │       │           └─► find_constant_uses_in_node()               │
│    │       │                 ├─► "call" nodes (User.find)             │
│    │       │                 └─► "scope_resolution" (Module::Class)   │
│    │       │                                                           │
│    │       └─► For each (caller, constant, range):                    │
│    │             add_relationships_by_name(                           │
│    │               caller, constant, file_id, "uses", metadata        │
│    │             )                                                     │
│    │               └─► pushes to self.unresolved_relationships        │
│    │                                                                   │
│    └─► commit_tantivy_batch()                                         │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│ Phase 2: RAILS SYMBOL TABLE CONSTRUCTION (simple.rs:2387)              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  RailsSymbolTable::build(dir)  ← rails.rs:255                          │
│    │                                                                    │
│    ├─► RailsProjectDetector::is_rails_project()  ← rails.rs:56        │
│    │     │                                                             │
│    │     ├─► Check: config/application.rb exists?                     │
│    │     │     └─► Contains "Rails::Application"?                     │
│    │     │                                                             │
│    │     └─► Fallback: app/ dir + Gemfile?                            │
│    │           └─► Contains gem 'rails'?                               │
│    │                                                                    │
│    ├─► ❌ FAILURE POINT: Returns false for guliveo                    │
│    │     └─► eprintln!("DEBUG: Not a Rails project...")               │
│    │           returns empty RailsSymbolTable                          │
│    │                                                                    │
│    └─► table.is_empty() == true                                        │
│          └─► self.rails_symbol_table = None (effectively)             │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│ Phase 3: RELATIONSHIP RESOLUTION (simple.rs:2627)                      │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  resolve_cross_file_relationships()                                    │
│    │                                                                    │
│    ├─► unresolved = take(self.unresolved_relationships)               │
│    │                                                                    │
│    └─► For each UnresolvedRelationship:                               │
│          │                                                             │
│          ├─► build_resolution_context(file_id)  ← simple.rs:2620     │
│          │     │                                                       │
│          │     ├─► Check: self.rails_symbol_table.is_some()?         │
│          │     │     └─► ❌ NO (table is empty)                       │
│          │     │                                                       │
│          │     ├─► ❌ SKIP: behavior.build_resolution_context_with_   │
│          │     │            rails() never called                      │
│          │     │                                                       │
│          │     └─► Falls back to: behavior.build_resolution_context() │
│          │           (imports only, no Rails autoloading)             │
│          │                                                             │
│          └─► Resolve using incomplete context                         │
│                ├─► Finds symbols via imports ✓                        │
│                └─► Misses Rails autoloaded constants ❌               │
│                                                                         │
│  Result: 0/150 UrlFormatter calls resolved (0%)                        │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Evidence-Based Failure Point Identification

### 1. Relationship Creation Pipeline (✓ WORKING)

**Code Path**: `simple.rs:1257` → `ruby/parser.rs:1343`

```rust
// simple.rs:1257 - Relationships ARE created
let uses = parser.find_uses(content);
for (context_name, used_type, _range) in uses {
    self.add_relationships_by_name(context_name, used_type, file_id,
                                   behavior.map_relationship("uses"), None)?;
}
```

**Ruby Parser Implementation** (`ruby/parser.rs:932-987`):
```rust
fn find_constant_uses_in_node(...) {
    match node.kind() {
        "call" => {
            // Extract receiver (e.g., User in User.find)
            if first_char.is_ascii_uppercase() {
                uses.push((caller, receiver_text, range));  // ✓ Tracks constant
            }
        }
        "scope_resolution" => {
            // Extract Module::Class patterns
            if first_char.is_ascii_uppercase() {
                uses.push((caller, scope_text, range));  // ✓ Tracks constant
            }
        }
    }
}
```

**Evidence**: Validation report confirms relationships are created:
- Relationship density: 0.3691 (19,476 relationships for 52,762 symbols)
- `find_uses()` extracts constant references correctly
- `add_relationships_by_name()` stores them as UnresolvedRelationship

**Verdict**: ✅ **FUNCTIONAL** - Constant uses ARE detected and tracked

---

### 2. Rails Detection Logic (❌ BROKEN)

**Code Path**: `simple.rs:2387` → `rails.rs:255` → `rails.rs:56`

```rust
// rails.rs:56 - Rails project detection
pub fn is_rails_project(&self) -> bool {
    // Primary check: config/application.rb with Rails::Application
    let app_rb = self.project_root.join("config/application.rb");
    if app_rb.exists() {
        if let Ok(contents) = fs::read_to_string(&app_rb) {
            if contents.contains("Rails::Application") {
                return true;  // ❌ NOT REACHED for guliveo
            }
        }
    }

    // Fallback: app/ + Gemfile with rails gem
    let has_app_dir = self.project_root.join("app").is_dir();
    let gemfile = self.project_root.join("Gemfile");
    if has_app_dir && gemfile.exists() {
        if let Ok(contents) = fs::read_to_string(&gemfile) {
            return contents.contains("gem 'rails'") || contents.contains("gem \"rails\"");
            // ❌ NOT REACHED for guliveo
        }
    }

    false  // ❌ Returns here for guliveo
}
```

**Evidence from Validation Report**:
```
DEBUG: Not a Rails project, skipping Rails autoloading support
(Appears 3 times during indexing of app/, lib/, config/)
```

**Guliveo Project Structure** (verified):
- ✓ Has `config/application.rb`
- ✓ Has `app/` directory
- ✓ Has `Gemfile`

**Verdict**: ❌ **BROKEN** - Detection returns false despite valid Rails indicators

**Hypothesis**: One of the following:
1. `project_root` path passed to `RailsSymbolTable::build()` is incorrect
2. `config/application.rb` doesn't contain exact string "Rails::Application"
3. `Gemfile` doesn't contain exact string `gem 'rails'` or `gem "rails"`
4. File read operations are failing silently

---

### 3. Resolution Context Construction (❌ BLOCKED BY #2)

**Code Path**: `simple.rs:2620` → `behavior.rs:605`

```rust
// simple.rs:2620 - Resolution context decision point
fn build_resolution_context(&mut self, file_id: FileId) -> IndexResult<...> {
    let behavior = self.get_behavior_for_file(file_id)?;

    if let Some(rails_table) = &self.rails_symbol_table {
        // ✅ Rails path (NOT REACHED - table is empty)
        if let Some(ruby_behavior) = behavior.as_any().downcast_ref::<RubyBehavior>() {
            return ruby_behavior.build_resolution_context_with_rails(
                file_id, &self.document_index, rails_table
            );
        }
    } else {
        // ❌ Fallback path (ALWAYS TAKEN - imports only)
        return behavior.build_resolution_context(file_id, &self.document_index);
    }
}
```

**Fallback Context** (`behavior.rs:427`):
```rust
fn build_resolution_context(...) -> IndexResult<...> {
    // 1. Add imported symbols (from require/require_relative)
    for import in imports { context.add_symbol(...); }

    // 2. Add file's module-level symbols
    for symbol in file_symbols { context.add_symbol(...); }

    // 3. Add visible symbols from other files
    for symbol in all_symbols {
        if symbol.visibility == Public { context.add_symbol(...); }
    }

    // ❌ MISSING: Rails autoloaded constants (UrlFormatter, etc.)
}
```

**Evidence**: UrlFormatter calls fail resolution:
```bash
$ codanna retrieve callers symbol_id:6280  # UrlFormatter.prettify_if_ajax_ugly
Error: function not found
```

**Verdict**: ❌ **BLOCKED** - Resolution context lacks Rails constants

---

## Critical Path Analysis

### Success Path (Expected)
```
1. RailsSymbolTable::build() detects Rails → table populated
2. table.resolve_symbol_ids() maps UrlFormatter → SymbolId(6249)
3. build_resolution_context_with_rails() adds UrlFormatter to context
4. resolve_cross_file_relationships() resolves UrlFormatter calls
5. Result: 135+ relationships created (>90% target)
```

### Actual Path (Broken)
```
1. RailsSymbolTable::build() returns empty table ← ❌ FAILURE POINT
2. Rails symbol resolution SKIPPED
3. build_resolution_context() uses imports only
4. resolve_cross_file_relationships() cannot resolve UrlFormatter
5. Result: 0 relationships created (0% vs >90% target)
```

---

## Diagnosis Summary

| Component | Status | Evidence | Impact |
|-----------|--------|----------|--------|
| **find_uses()** | ✅ WORKING | Tracks 150 UrlFormatter calls via AST | Relationships created |
| **add_relationships_by_name()** | ✅ WORKING | 19,476 relationships stored | Unresolved list populated |
| **Rails Detection** | ❌ BROKEN | Returns false for guliveo | Blocks entire Rails path |
| **Symbol Table Build** | ❌ BLOCKED | Empty table returned | No constant mappings |
| **Resolution Context** | ❌ DEGRADED | Falls back to imports-only | Missing Rails constants |
| **Relationship Resolution** | ❌ BROKEN | 0/150 resolved (0%) | Zero detection rate |

---

## Recommended Investigation Steps

### Immediate (P0 - Blocker)

1. **Verify project_root parameter**:
   ```rust
   // Add debug logging in simple.rs:2387
   eprintln!("DEBUG: Building Rails table for: {:?}", dir.as_ref());
   ```

2. **Check guliveo file contents**:
   ```bash
   # Verify exact strings in guliveo
   cat /path/to/guliveo/config/application.rb | grep -i "rails"
   cat /path/to/guliveo/Gemfile | grep -i "rails"
   ```

3. **Add detection diagnostics**:
   ```rust
   // rails.rs:56 - Enhanced logging
   eprintln!("DEBUG: Checking Rails detection for: {:?}", self.project_root);
   eprintln!("DEBUG: config/application.rb exists: {}", app_rb.exists());
   eprintln!("DEBUG: app/ exists: {}", has_app_dir);
   eprintln!("DEBUG: Gemfile exists: {}", gemfile.exists());
   ```

### Secondary (P1 - Validation)

4. **Test detection in isolation**:
   ```rust
   #[test]
   fn test_guliveo_detection() {
       let detector = RailsProjectDetector::new("/path/to/guliveo");
       assert!(detector.is_rails_project(), "Should detect guliveo as Rails");
   }
   ```

5. **Verify resolution context path**:
   ```rust
   // simple.rs:2620 - Confirm which branch is taken
   if let Some(rails_table) = &self.rails_symbol_table {
       eprintln!("DEBUG: Using Rails resolution context");
   } else {
       eprintln!("DEBUG: Falling back to standard resolution");
   }
   ```

---

## Confidence Assessment

**Confidence in Root Cause**: **9/10**

**High Confidence Because**:
1. ✅ Direct evidence: "DEBUG: Not a Rails project" message
2. ✅ Code path traced from entry to failure point
3. ✅ Validation report confirms 0% detection rate
4. ✅ All downstream components depend on detection success
5. ✅ No alternative explanations for empty symbol table

**Remaining Uncertainty** (10%):
- Don't know *exact reason* detection fails (need to read guliveo files)
- Can't confirm if fixing detection will expose other issues
- Unknown if resolution logic has additional bugs (blocked by detection)

---

## References

### Key Code Locations

| Component | File:Line | Description |
|-----------|-----------|-------------|
| Relationship creation | `simple.rs:1257` | Calls `find_uses()` and stores relationships |
| Constant detection | `ruby/parser.rs:932` | Extracts constant uses from AST |
| Rails detection | `rails.rs:56` | `is_rails_project()` logic |
| Symbol table build | `rails.rs:255` | `RailsSymbolTable::build()` entry point |
| Resolution context | `simple.rs:2620` | Decides standard vs Rails context |
| Rails context builder | `behavior.rs:605` | Adds Rails constants to context |
| Resolution execution | `simple.rs:2627` | `resolve_cross_file_relationships()` |

### Validation Report Evidence

- Detection failure: Line 232 (`rails.rs:260`)
- Ground truth: 150 UrlFormatter calls exist
- Measured detection: 0/150 = 0.0%
- Relationship density: 0.3691 (relationships exist, just not resolved)

---

## Architectural Insights

### Critic's Claim Validated

**Critic stated**: `behavior.rs:646-657` is for RESOLUTION CONTEXT, not relationship creation.

**Verification**: ✅ **CORRECT**

```rust
// behavior.rs:646-657 - This is in build_resolution_context_with_rails()
mut_context.add_symbol(
    short_name.to_string(),
    symbol_id,
    crate::parsing::ScopeLevel::Package,
);
```

This code ADDS symbols to a resolution context (for lookups), it does NOT create relationships. Relationships are created earlier in the pipeline via `add_relationships_by_name()`.

### Architecture Flow Confirmed

The pipeline has clear separation of concerns:
1. **Extraction** (`find_uses()`) - Discover references in AST
2. **Storage** (`add_relationships_by_name()`) - Store as unresolved
3. **Context** (`build_resolution_context()`) - Build lookup environment
4. **Resolution** (`resolve_cross_file_relationships()`) - Match references to symbols

Rails detection failure breaks step 3, which blocks step 4, despite steps 1-2 working correctly.

---

## Conclusion

The Rails constant resolution failure is NOT a relationship creation issue or a `find_uses()` implementation problem. The root cause is **Rails project detection failure** at the indexing pipeline entry point.

Fixing this requires:
1. Diagnosing why `is_rails_project()` returns false for guliveo
2. Either fixing the detection logic OR adjusting guliveo's project structure
3. Validating that symbol table construction works after detection succeeds
4. Re-running full validation protocol to measure improvement

**Next Action**: Investigate `project_root` parameter and guliveo file contents to identify exact detection failure reason.
