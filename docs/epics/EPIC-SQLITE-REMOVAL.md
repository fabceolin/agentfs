# EPIC-SQLITE-REMOVAL: Complete SQLite/Turso Removal

## Metadata

| Field | Value |
|-------|-------|
| **ID** | EPIC-SQLITE-REMOVAL |
| **Type** | Brownfield Enhancement |
| **Status** | Draft |
| **Priority** | High |
| **Created** | 2026-01-17 |

## Epic Goal

Remove all SQLite/Turso code from the AgentFS codebase, establishing DuckDB (DuckAgentFS) as the sole database backend. This simplifies the codebase, reduces dependencies, and unifies the storage layer.

## Epic Description

### Existing System Context

- **Current functionality**: Dual-backend support (SQLite via Turso, DuckDB)
- **Technology stack**: Rust CLI, Rust/TypeScript/Python SDKs, FUSE/NFS mounts
- **Integration points**: FileSystem trait implementations, CLI commands, sandbox VFS

### Enhancement Details

- **What's being changed**: Removing SQLite/Turso backend entirely
- **How it integrates**: DuckAgentFS replaces AgentFS everywhere
- **Success criteria**:
  - Zero `turso` crate references
  - Zero `AgentFS` (SQLite) type usage
  - All 35 affected files migrated or removed
  - All tests pass with DuckDB-only

### Affected Files Summary

| Area | File Count | Key Files |
|------|------------|-----------|
| CLI Commands | 11 | init.rs, fs.rs, sync.rs, timeline.rs, run_darwin.rs, mcp_server.rs, nfs.rs |
| Rust SDK | 8 | agentfs.rs, overlayfs.rs, connection_pool.rs, kvstore.rs, toolcalls.rs |
| Sandbox | 6 | sqlite.rs, vfs/mod.rs, vfs/file.rs, vfs/mount.rs |
| Benchmarks | 2 | overlayfs.rs, workload.rs |
| Other | 8 | Various integration points |

## Stories

### STORY-6.1: Remove SQLite from CLI Commands
**Priority:** High | **Complexity:** Medium

Remove SQLite/AgentFS usage from all CLI command implementations:
- `init.rs` - Switch to DuckAgentFS initialization
- `fs.rs` - Update file operations to DuckDB
- `sync.rs` - Remove Turso sync (DuckDB has different sync model)
- `timeline.rs` - Update timeline queries for DuckDB
- `run_darwin.rs` - Update macOS run command
- `mcp_server.rs` - Update MCP protocol handler
- `nfs.rs` - Update NFS server backend

**Acceptance Criteria:**
1. All CLI commands use DuckAgentFS exclusively
2. `agentfs init` creates DuckDB databases only
3. Remove `open_agentfs` helper function
4. Update error messages to reference DuckDB
5. All CLI tests pass

---

### STORY-6.2: Remove SQLite from Rust SDK
**Priority:** High | **Complexity:** High

Remove SQLite-based filesystem implementation from SDK:
- Remove `agentfs.rs` (SQLite FileSystem)
- Remove or adapt `overlayfs.rs` (uses AgentFS as delta layer)
- Update `connection_pool.rs` to DuckDB-only
- Update `kvstore.rs` if it has SQLite dependencies
- Update `toolcalls.rs` if it has SQLite dependencies
- Update `lib.rs` exports

**Acceptance Criteria:**
1. `AgentFS` type removed from SDK
2. `OverlayFS` either removed or uses DuckAgentFS
3. Connection pool is DuckDB-only
4. All SDK public exports updated
5. SDK tests pass with DuckDB-only

---

### STORY-6.3: Remove SQLite from Sandbox Module
**Priority:** Medium | **Complexity:** Medium

Remove SQLite VFS from sandbox syscall interception:
- Remove `sandbox/src/vfs/sqlite.rs`
- Update VFS mount handling for DuckDB
- Update syscall handlers that reference SQLite
- Update sandbox lib.rs exports

**Acceptance Criteria:**
1. `sqlite.rs` VFS module removed
2. Sandbox uses DuckDB for intercepted operations
3. Syscall handlers updated
4. Sandbox tests pass

---

### STORY-6.4: Update Benchmarks to DuckDB-only
**Priority:** Low | **Complexity:** Low

Update benchmark suite to use DuckDB exclusively:
- `benches/overlayfs.rs` - Update or remove if OverlayFS removed
- `benches/workload.rs` - Update workload benchmarks

**Acceptance Criteria:**
1. All benchmarks use DuckAgentFS
2. Benchmark results comparable or better
3. No SQLite references in bench code

---

### STORY-6.5: Remove Turso Dependency
**Priority:** Final | **Complexity:** Low

Final cleanup - remove Turso crate from dependencies:
- Remove `turso` from `cli/Cargo.toml`
- Remove `turso` from `sdk/rust/Cargo.toml`
- Remove `turso` from `sandbox/Cargo.toml`
- Remove `schema/agentfs.sql` if present
- Update `docs/architecture/tech-stack.md`

**Acceptance Criteria:**
1. Zero `turso` references in any Cargo.toml
2. `cargo build` succeeds without Turso
3. Documentation updated to reflect DuckDB-only
4. Tech stack document updated

---

## Compatibility Requirements

- [x] Existing DuckDB APIs remain unchanged
- [x] Database schema (DuckDB) unchanged
- [ ] CLI interface unchanged (same commands, DuckDB backend)
- [ ] Performance maintained or improved

**Breaking Changes:**
- SQLite `.db` files will no longer be mountable
- Users must migrate existing SQLite databases to DuckDB format
- `agentfs sync` behavior will change (no Turso remote sync)

## Risk Mitigation

| Risk | Mitigation | Rollback |
|------|------------|----------|
| **Breaking existing SQLite users** | Document migration path | Git revert to pre-removal state |
| **Missing functionality in DuckDB** | Audit feature parity before removal | Keep SQLite code in separate branch |
| **Performance regression** | Run benchmarks before/after | Optimize DuckDB queries |

## Definition of Done

- [ ] All 5 stories completed with acceptance criteria met
- [ ] Zero SQLite/Turso references in codebase
- [ ] All tests pass (CLI, SDK, Sandbox)
- [ ] Benchmarks run successfully
- [ ] Documentation updated
- [ ] No regression in DuckDB functionality

## Dependencies

| Story | Depends On |
|-------|------------|
| STORY-6.1 | None (can start immediately) |
| STORY-6.2 | STORY-6.1 (CLI patterns established) |
| STORY-6.3 | STORY-6.2 (SDK patterns established) |
| STORY-6.4 | STORY-6.2, STORY-6.3 |
| STORY-6.5 | All previous stories |

## Story Sequence

```
STORY-6.1 (CLI) ──┬──► STORY-6.2 (SDK) ──┬──► STORY-6.4 (Benchmarks)
                  │                       │
                  │                       └──► STORY-6.3 (Sandbox) ──► STORY-6.5 (Cleanup)
```

---

## Story Files

| Story | File | Status |
|-------|------|--------|
| STORY-6.1 | `docs/stories/STORY-6.1-remove-sqlite-cli.md` | **Approved** |
| STORY-6.2 | `docs/stories/STORY-6.2-remove-sqlite-sdk.md` | **Approved** |
| STORY-6.3 | `docs/stories/STORY-6.3-remove-sqlite-sandbox.md` | **Approved** |
| STORY-6.4 | `docs/stories/STORY-6.4-update-benchmarks.md` | **Approved** |
| STORY-6.5 | `docs/stories/STORY-6.5-remove-turso-dependency.md` | **Approved** |

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-17 | Epic created | Sarah (PO Agent) |
| 2026-01-17 | All 5 stories created | Sarah (PO Agent) |
| 2026-01-17 | STORY-6.1 validated and approved | Sarah (PO Agent) |
| 2026-01-17 | STORY-6.2-6.5 validated | Sarah (PO Agent) |
| 2026-01-17 | OverlayFS decision: DELETE (tightly coupled to AgentFS) | Sarah (PO Agent) |
| 2026-01-17 | All stories approved - ready for implementation | Sarah (PO Agent) |
