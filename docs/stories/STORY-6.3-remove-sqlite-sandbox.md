# STORY-6.3: Remove SQLite from Sandbox Module

> **NOTE**: This story removes SQLite VFS from the Linux sandbox syscall interception module.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-6.3 |
| **Epic** | EPIC-SQLITE-REMOVAL |
| **Status** | Approved |
| **Priority** | Medium |
| **Dependencies** | STORY-6.2 (SDK patterns established) |
| **Blocked By** | STORY-6.2 |

## User Story

**As a** developer maintaining the AgentFS sandbox
**I want** to remove SQLite VFS from syscall interception
**So that** the sandbox uses DuckDB exclusively for virtualized filesystem operations

## Story Context

**Gap Identified:** The sandbox module has SQLite-specific VFS handling for intercepting filesystem syscalls within sandboxed processes.

**Affected Files:**

| File | Current State | Action |
|------|---------------|--------|
| `sandbox/src/vfs/sqlite.rs` | SQLite VFS impl | **DELETE** |
| `sandbox/src/vfs/mod.rs` | Exports sqlite module | Remove export |
| `sandbox/src/vfs/file.rs` | May reference SQLite | Check and update |
| `sandbox/src/vfs/mount.rs` | May reference SQLite | Check and update |
| `sandbox/src/lib.rs` | Module exports | Update |
| `sandbox/src/syscall/file.rs` | Syscall handlers | Check for SQLite refs |
| `sandbox/src/syscall/stat.rs` | Stat handlers | Check for SQLite refs |

## Acceptance Criteria

- [ ] AC1: `sqlite.rs` VFS module deleted
- [ ] AC2: VFS mod.rs updated to remove sqlite export
- [ ] AC3: All syscall handlers use DuckDB patterns
- [ ] AC4: Sandbox lib.rs exports updated
- [ ] AC5: Sandbox compiles without SQLite deps
- [ ] AC6: Sandbox tests pass

## Tasks / Subtasks

- [ ] Task 1: Audit Sandbox SQLite Usage
  - [ ] Identify all SQLite references in sandbox/src/
  - [ ] Document which are deletable vs need migration
  - [ ] Check if sandbox even needs database VFS (may be filesystem-only)

- [ ] Task 2: Remove `sqlite.rs` (AC: 1, 2)
  - [ ] Delete `sandbox/src/vfs/sqlite.rs`
  - [ ] Update `sandbox/src/vfs/mod.rs` to remove sqlite module

- [ ] Task 3: Update VFS Modules (AC: 3)
  - [ ] Check `vfs/file.rs` for SQLite references
  - [ ] Check `vfs/mount.rs` for SQLite references
  - [ ] Update or remove SQLite-specific code paths

- [ ] Task 4: Update Syscall Handlers (AC: 3)
  - [ ] Check `syscall/file.rs` for SQLite VFS calls
  - [ ] Check `syscall/stat.rs` for SQLite VFS calls
  - [ ] Remove or update SQLite-specific handling

- [ ] Task 5: Update Sandbox Exports (AC: 4)
  - [ ] Update `sandbox/src/lib.rs`
  - [ ] Remove SQLite-related public exports

- [ ] Task 6: Run Tests and Validation (AC: 5, 6)
  - [ ] Run `cd sandbox && cargo build`
  - [ ] Run `cargo test`
  - [ ] Run `cargo clippy`
  - [ ] Verify no SQLite imports: `grep -r "sqlite\|turso" sandbox/src/`

## Dev Notes

### Source Tree Reference

```
sandbox/src/
├── lib.rs              # Module exports - UPDATE
├── sandbox/
│   └── mod.rs          # Sandbox orchestration
├── syscall/
│   ├── mod.rs
│   ├── file.rs         # File syscalls - CHECK
│   ├── stat.rs         # Stat syscalls - CHECK
│   ├── xattr.rs
│   └── process.rs
└── vfs/
    ├── mod.rs          # VFS exports - UPDATE
    ├── file.rs         # File abstraction - CHECK
    ├── fdtable.rs
    ├── mount.rs        # Mount management - CHECK
    ├── bind.rs
    └── sqlite.rs       # DELETE
```

### Key Question

**Does the sandbox need database VFS at all?**

The sandbox intercepts syscalls to virtualize filesystem access. It may only need:
- HostFS passthrough
- Bind mounts
- File descriptor table

If SQLite VFS was only for AgentFS database access (not sandbox virtualization), deletion may be straightforward.

### Testing

- Test location: `sandbox/src/*.rs` (inline tests)
- Test command: `cd sandbox && cargo test`
- Note: Sandbox only builds on Linux

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Breaking sandbox functionality | Medium | High | Audit before deletion |
| Missing DuckDB VFS | Low | Medium | May not be needed |

## Definition of Done

- [ ] All 6 tasks completed
- [ ] All 6 acceptance criteria verified
- [ ] `cargo build` passes in sandbox/
- [ ] `cargo test` passes
- [ ] Zero SQLite/Turso imports in sandbox/src/
- [ ] `sqlite.rs` file deleted

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

---

## QA Results

(To be filled by QA agent)
