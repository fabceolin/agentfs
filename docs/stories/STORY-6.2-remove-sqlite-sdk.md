# STORY-6.2: Remove SQLite from Rust SDK

> **NOTE**: This story removes the SQLite-based AgentFS implementation from the Rust SDK.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-6.2 |
| **Epic** | EPIC-SQLITE-REMOVAL |
| **Status** | Approved |
| **Priority** | High |
| **Dependencies** | STORY-6.1 (CLI patterns established) |
| **Blocked By** | STORY-6.1 |

## User Story

**As a** developer maintaining the AgentFS SDK
**I want** to remove the SQLite-based AgentFS implementation
**So that** the SDK has a single, unified DuckDB backend

## Story Context

**Gap Identified:** The Rust SDK contains dual filesystem implementations:
- `agentfs.rs` - SQLite-based (to be removed)
- `duckagentfs.rs` - DuckDB-based (to be kept)

**Affected Files:**

| File | Current State | Action |
|------|---------------|--------|
| `sdk/rust/src/filesystem/agentfs.rs` | SQLite FileSystem impl | **DELETE** |
| `sdk/rust/src/filesystem/overlayfs.rs` | Uses AgentFS as delta | Update or delete |
| `sdk/rust/src/filesystem/mod.rs` | Exports both | Remove AgentFS exports |
| `sdk/rust/src/connection_pool.rs` | Turso connection pool | Remove or update |
| `sdk/rust/src/kvstore.rs` | May use Turso | Check and update |
| `sdk/rust/src/toolcalls.rs` | Uses Turso Builder/Value | Migrate to DuckDB |
| `sdk/rust/src/lib.rs` | Public exports | Remove AgentFS exports |

## Acceptance Criteria

- [ ] AC1: `AgentFS` type removed from SDK (agentfs.rs deleted)
- [ ] AC2: `OverlayFS` either removed or updated to use DuckAgentFS
- [ ] AC3: `ConnectionPool` (Turso-based) removed
- [ ] AC4: `KvStore` works with DuckDB only
- [ ] AC5: `ToolCalls` migrated from Turso to DuckDB
- [ ] AC6: All public exports updated in `lib.rs`
- [ ] AC7: All SDK tests pass
- [ ] AC8: No `turso::` imports remain in SDK

## Tasks / Subtasks

- [ ] Task 1: Remove `agentfs.rs` (AC: 1)
  - [ ] Delete `sdk/rust/src/filesystem/agentfs.rs`
  - [ ] Update `sdk/rust/src/filesystem/mod.rs` to remove AgentFS export
  - [ ] Update `sdk/rust/src/lib.rs` to remove AgentFS re-export

- [ ] Task 2: Delete `overlayfs.rs` (AC: 2)
  - [ ] **Decision: Option A - DELETE** (OverlayFS requires AgentFS as delta layer)
  - [ ] Delete `sdk/rust/src/filesystem/overlayfs.rs`
  - [ ] Update `sdk/rust/src/filesystem/mod.rs` to remove OverlayFS export
  - [ ] Update `sdk/rust/src/lib.rs` to remove OverlayFS re-export
  - [ ] Remove OverlayFS from CLI if referenced (check mount.rs, sandbox)

- [ ] Task 3: Remove `connection_pool.rs` Turso code (AC: 3)
  - [ ] Identify Turso-specific connection pool code
  - [ ] Remove or consolidate with DuckDB pool
  - [ ] Update imports in dependent files

- [ ] Task 4: Update `kvstore.rs` (AC: 4)
  - [ ] Audit for Turso dependencies
  - [ ] Migrate to DuckDB queries if needed
  - [ ] Update tests

- [ ] Task 5: Migrate `toolcalls.rs` to DuckDB (AC: 5)
  - [ ] Replace `turso::{Builder, Value}` with DuckDB equivalents
  - [ ] Update query syntax for DuckDB
  - [ ] Update `ToolCalls::from_pool()` to use DuckConnectionPool
  - [ ] Update all tests

- [ ] Task 6: Update `lib.rs` exports (AC: 6)
  - [ ] Remove `AgentFS` from public exports
  - [ ] Remove `AgentFSOptions` from public exports
  - [ ] Remove Turso re-exports if any
  - [ ] Verify all public API still works

- [ ] Task 7: Run Tests and Validation (AC: 7, 8)
  - [ ] Run `cargo test` in sdk/rust
  - [ ] Run `cargo clippy`
  - [ ] Verify no `turso::` imports remain: `grep -r "turso::" sdk/rust/src/`

## Dev Notes

### Files to Delete

```
sdk/rust/src/filesystem/agentfs.rs  # DELETE - SQLite implementation
```

### Files to Delete

```
sdk/rust/src/filesystem/overlayfs.rs  # DELETE - requires AgentFS delta layer
sdk/rust/src/connection_pool.rs       # DELETE or merge - Turso-specific pool
```

### OverlayFS Removal Rationale

OverlayFS (`overlayfs.rs` line 258) declares:
```rust
delta: AgentFS,  // Writable delta layer (must be AgentFS for whiteout storage)
```

This is tightly coupled to SQLite AgentFS. A DuckDB-based overlay would require new implementation and is out of scope for this migration.

### ToolCalls Migration Pattern

```rust
// OLD (Turso/SQLite)
use turso::{Builder, Value};
let mut stmt = conn.prepare("SELECT ...").await?;
let rows = stmt.query(()).await?;

// NEW (DuckDB)
use duckdb::params;
let conn = pool.get_connection()?;
let mut stmt = conn.prepare("SELECT ...")?;
let rows = stmt.query_map(params![], |row| { ... })?;
```

### Source Tree Reference

```
sdk/rust/src/
├── lib.rs              # Public exports - UPDATE
├── error.rs            # Keep as-is
├── connection_pool.rs  # Remove Turso pool
├── kvstore.rs          # Check for Turso deps
├── toolcalls.rs        # Migrate to DuckDB
├── embedding.rs        # Keep as-is
└── filesystem/
    ├── mod.rs          # Remove AgentFS export
    ├── agentfs.rs      # DELETE
    ├── duckagentfs.rs  # Keep as-is
    ├── hostfs.rs       # Keep as-is
    └── overlayfs.rs    # Check/Update/Delete
```

### Testing

- Test location: `sdk/rust/src/*.rs` (inline tests)
- Test command: `cd sdk/rust && cargo test`
- Lint command: `cargo clippy`

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Breaking SDK consumers | Medium | High | Update CLI first (STORY-6.1) |
| OverlayFS removal breaks features | Low | Medium | Audit usage before deletion |
| ToolCalls migration errors | Medium | Medium | Comprehensive test coverage |

## Definition of Done

- [ ] All 7 tasks completed
- [ ] All 8 acceptance criteria verified
- [ ] `cargo test` passes in sdk/rust
- [ ] `cargo clippy` passes
- [ ] Zero `turso::` imports in sdk/rust/src/
- [ ] `agentfs.rs` file deleted

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
| 2026-01-17 | Specified OverlayFS decision (Option A: Delete) | Validation required explicit decision |
| 2026-01-17 | Added OverlayFS removal rationale | Document architectural reasoning |

---

## QA Results

(To be filled by QA agent)
