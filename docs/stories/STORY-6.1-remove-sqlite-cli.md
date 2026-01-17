# STORY-6.1: Remove SQLite from CLI Commands

> **NOTE**: This is a brownfield migration story. DuckAgentFS patterns are already established in STORY-4.3.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-6.1 |
| **Epic** | EPIC-SQLITE-REMOVAL |
| **Status** | Approved |
| **Priority** | High |
| **Dependencies** | STORY-4.3 (completed - established DuckDB mount pattern) |
| **Blocked By** | None |
| **Partial Blocks** | STORY-6.2 (timeline.rs uses ToolCalls which needs SDK migration) |

## User Story

**As a** developer maintaining the AgentFS codebase
**I want** to remove all SQLite/Turso code from CLI commands
**So that** the codebase is simplified with a single DuckDB backend

## Story Context

**Gap Identified:** CLI commands still use SQLite-based AgentFS via `open_agentfs()` helper and `AgentFSOptions`. This creates maintenance burden and confusion with dual backends.

**Migration Pattern:** Follow STORY-4.3 pattern established in `mount.rs`:
- Replace `AgentFSOptions::resolve()` with direct path resolution
- Replace `open_agentfs()` with `DuckAgentFS::open()`
- Remove Turso/SQLite imports

**Affected Files:**

| File | Current Usage | Migration Action |
|------|---------------|------------------|
| `cli/src/cmd/init.rs` | Creates SQLite DB via AgentFSOptions | Create DuckDB via DuckAgentFSConfig |
| `cli/src/cmd/fs.rs` | Opens AgentFS for file ops | Use DuckAgentFS |
| `cli/src/cmd/sync.rs` | Turso sync operations | Remove or stub (DuckDB has no remote sync) |
| `cli/src/cmd/timeline.rs` | Queries SQLite timeline | Query DuckDB journal |
| `cli/src/cmd/run_darwin.rs` | macOS sandbox with AgentFS | Use DuckAgentFS |
| `cli/src/cmd/mcp_server.rs` | MCP protocol with AgentFS | Use DuckAgentFS |
| `cli/src/cmd/nfs.rs` | NFS server with AgentFS | Use DuckAgentFS |
| `cli/src/main.rs` | Dispatches to AgentFS | Update dispatch logic |
| `cli/src/parser.rs` | May have SQLite-specific args | Update if needed |

## Acceptance Criteria

- [ ] AC1: `agentfs init` creates DuckDB databases (`.duckdb` extension)
- [ ] AC2: `agentfs fs` commands work with DuckDB databases
- [ ] AC3: `agentfs timeline` queries DuckDB journal tables
- [ ] AC4: `agentfs run` (Darwin) uses DuckAgentFS
- [ ] AC5: `agentfs mcp-server` uses DuckAgentFS
- [ ] AC6: `agentfs nfs` uses DuckAgentFS
- [ ] AC7: `agentfs sync` is removed or returns "not supported for DuckDB"
- [ ] AC8: `open_agentfs()` helper function removed from `init.rs`
- [ ] AC9: All SQLite/Turso imports removed from CLI command files
- [ ] AC10: All existing CLI tests pass

## Tasks / Subtasks

- [ ] Task 1: Update `init.rs` - Database Initialization (AC: 1, 8, 9)
  - [ ] Replace `AgentFSOptions` with `DuckAgentFSConfig`
  - [ ] Create `.duckdb` files instead of `.db`
  - [ ] Remove `open_agentfs()` helper function
  - [ ] Update schema initialization for DuckDB

- [ ] Task 2: Update `fs.rs` - File Operations (AC: 2, 9)
  - [ ] Audit current `fs.rs` to identify all AgentFS touchpoints
  - [ ] Replace AgentFS with DuckAgentFS
  - [ ] Update file operation handlers (cat, write, mkdir, etc.)
  - [ ] Remove SQLite imports

- [ ] Task 3: Update `timeline.rs` - Timeline Queries (AC: 3, 9)
  - [ ] Replace `AgentFSOptions::resolve()` with `resolve_db_path()`
  - [ ] Replace `open_agentfs()` with `DuckAgentFS::open()`
  - [ ] Use `duckfs.pool()` instead of `agentfs.get_pool()`
  - [ ] Remove SQLite imports
  - **NOTE:** `ToolCalls` struct still uses Turso internally (STORY-6.2 scope). Timeline will work but ToolCalls migration deferred to SDK story.

- [ ] Task 4: Handle `sync.rs` - Sync Command (AC: 7, 9)
  - [ ] **Decision: Option B** - Return user-friendly error message
  - [ ] Replace functions to return: `anyhow::bail!("Sync is not supported for DuckDB databases. Use external backup/restore tools.")`
  - [ ] Remove Turso sync imports (`AgentFSOptions`, `open_agentfs`)
  - [ ] Keep command structure for future DuckDB sync implementation

- [ ] Task 5: Update `run_darwin.rs` - macOS Sandbox (AC: 4, 9)
  - [ ] Replace AgentFS with DuckAgentFS
  - [ ] Update sandbox initialization
  - [ ] Remove SQLite imports

- [ ] Task 6: Update `mcp_server.rs` - MCP Protocol (AC: 5, 9)
  - [ ] Replace AgentFS with DuckAgentFS
  - [ ] Update MCP handlers
  - [ ] Remove SQLite imports

- [ ] Task 7: Update `nfs.rs` - NFS Server (AC: 6, 9)
  - [ ] Replace AgentFS with DuckAgentFS
  - [ ] Update NFS mount handlers
  - [ ] Remove SQLite imports

- [ ] Task 8: Update `main.rs` and `parser.rs` (AC: 9)
  - [ ] Remove SQLite-specific command dispatch
  - [ ] Update any SQLite-specific CLI arguments
  - [ ] Clean up imports

- [ ] Task 9: Run Tests and Validation (AC: 10)
  - [ ] Run `cargo test` - all tests pass
  - [ ] Run `cargo clippy` - no new warnings
  - [ ] Verify CLI commands work with existing DuckDB databases

## Dev Notes

### Reference Implementation

Follow the pattern established in `mount.rs` (STORY-4.3):

```rust
// OLD (SQLite)
use agentfs_sdk::{AgentFSOptions, FileSystem, HostFS, OverlayFS};
use crate::cmd::init::open_agentfs;
let opts = AgentFSOptions::resolve(&args.id_or_path)?;
let agentfs = rt.block_on(open_agentfs(opts))?;

// NEW (DuckDB)
use agentfs_sdk::filesystem::duckagentfs::{DuckAgentFSConfig, DuckConnectionPool};
use agentfs_sdk::filesystem::DuckAgentFS;
let config = DuckAgentFSConfig {
    path: db_path.clone(),
    ..Default::default()
};
let duckfs = rt.block_on(DuckAgentFS::open(config))?;
```

### Path Resolution Helper

Use pattern from `mount.rs`:

```rust
fn resolve_db_path(id_or_path: &str) -> Result<String> {
    if id_or_path == ":memory:" {
        return Ok(":memory:".to_string());
    }
    let path = std::path::Path::new(id_or_path);
    if path.exists() {
        return Ok(id_or_path.to_string());
    }
    // Try as agent ID
    let agentfs_dir = agentfs_sdk::agentfs_dir();
    let db_path = agentfs_dir.join(format!("{}.duckdb", id_or_path));
    if db_path.exists() {
        return Ok(db_path.to_string_lossy().to_string());
    }
    anyhow::bail!("DuckDB database not found: {}", id_or_path)
}
```

### Source Tree Reference

```
cli/src/
├── main.rs              # Command dispatch
├── parser.rs            # CLI argument definitions
└── cmd/
    ├── init.rs          # `agentfs init` - PRIORITY
    ├── fs.rs            # `agentfs fs` subcommands
    ├── sync.rs          # `agentfs sync` - REMOVE/STUB
    ├── timeline.rs      # `agentfs timeline`
    ├── run_darwin.rs    # macOS `agentfs run`
    ├── mcp_server.rs    # `agentfs mcp-server`
    ├── nfs.rs           # `agentfs nfs`
    └── mount.rs         # Already migrated (STORY-4.3)
```

### Testing

- Test file location: `cli/src/cmd/*.rs` (inline tests)
- Test command: `cargo test`
- Lint command: `cargo clippy`
- All 119+ CLI tests must continue to pass

### Key Integration Points

1. **`init.rs`**: Contains `open_agentfs()` helper used by other commands
2. **`main.rs` line 264-266**: DuckAgentFS dispatch logic already exists for graphdocs
3. **DuckDB schema**: `schema/duckagentfs.sql` - already complete

### API Compatibility Notes

| SQLite (AgentFS) | DuckDB (DuckAgentFS) | Notes |
|------------------|---------------------|-------|
| `agentfs.get_pool()` | `duckfs.pool()` | Different method name |
| `AgentFSOptions::resolve()` | `resolve_db_path()` | Use helper from mount.rs |
| `open_agentfs(opts)` | `DuckAgentFS::open(config)` | Async, returns Result |
| `.db` extension | `.duckdb` extension | File naming change |

### Verification Commands

After completing each file, run these to verify:

```bash
# Check for remaining SQLite imports
grep -r "AgentFSOptions\|open_agentfs\|turso::" cli/src/cmd/

# Run tests
cargo test

# Run linter
cargo clippy
```

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Breaking existing tests | Medium | High | Run full test suite after each file |
| Missing functionality | Low | Medium | Audit each command before migration |
| Sync command users | Low | Low | Clear error message with migration path |

## Definition of Done

- [ ] All 9 tasks completed
- [ ] All 10 acceptance criteria verified
- [ ] `cargo test` passes (119+ tests)
- [ ] `cargo clippy` passes
- [ ] No SQLite/Turso imports in any `cli/src/cmd/*.rs` file
- [ ] Story file updated with Dev Agent Record

---

## Dev Agent Record

### Agent Model Used
(To be filled by dev agent)

### Debug Log References
(To be filled by dev agent)

### Completion Notes List
(To be filled by dev agent)

### File List
(To be filled by dev agent)

### Change Log

| Date | Change | Reason |
|------|--------|--------|
| 2026-01-17 | Story created | Part of EPIC-SQLITE-REMOVAL |
| 2026-01-17 | Added ToolCalls dependency note | Validation found cross-story dependency |
| 2026-01-17 | Specified sync command decision (Option B) | Validation required explicit decision |
| 2026-01-17 | Added API compatibility table | Improve dev agent context |
| 2026-01-17 | Added verification commands | Enable self-validation |

---

## QA Results

(To be filled by QA agent)
