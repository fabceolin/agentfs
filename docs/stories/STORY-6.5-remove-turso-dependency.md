# STORY-6.5: Remove Turso Dependency

> **NOTE**: This is the final cleanup story - removes Turso crate from all Cargo.toml files.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-6.5 |
| **Epic** | EPIC-SQLITE-REMOVAL |
| **Status** | Approved |
| **Priority** | Final |
| **Dependencies** | STORY-6.1, STORY-6.2, STORY-6.3, STORY-6.4 |
| **Blocked By** | All previous stories in epic |

## User Story

**As a** developer maintaining AgentFS
**I want** to remove the Turso crate dependency entirely
**So that** the codebase has no SQLite dependencies and reduced binary size

## Story Context

**Gap Identified:** After all SQLite code is removed, the Turso crate will still be listed in Cargo.toml files, adding unnecessary dependencies.

**Affected Files:**

| File | Current State | Action |
|------|---------------|--------|
| `cli/Cargo.toml` | Has `turso` dependency | Remove |
| `sdk/rust/Cargo.toml` | Has `turso` dependency | Remove |
| `sandbox/Cargo.toml` | Has `turso` dependency | Remove |
| `Cargo.lock` files | Has turso entries | Auto-updated |
| `docs/architecture/tech-stack.md` | Lists SQLite/Turso | Update |
| `schema/agentfs.sql` | SQLite schema | Delete if exists |
| `CLAUDE.md` | May reference SQLite | Update |

## Acceptance Criteria

- [ ] AC1: `turso` removed from `cli/Cargo.toml`
- [ ] AC2: `turso` removed from `sdk/rust/Cargo.toml`
- [ ] AC3: `turso` removed from `sandbox/Cargo.toml`
- [ ] AC4: `cargo build` succeeds in all workspaces
- [ ] AC5: Documentation updated (tech-stack.md)
- [ ] AC6: SQLite schema file removed if present
- [ ] AC7: CLAUDE.md updated to reflect DuckDB-only

## Tasks / Subtasks

- [ ] Task 1: Remove Turso from CLI (AC: 1)
  - [ ] Edit `cli/Cargo.toml` to remove `turso` dependency
  - [ ] Remove any turso feature flags
  - [ ] Run `cargo build` to verify

- [ ] Task 2: Remove Turso from SDK (AC: 2)
  - [ ] Edit `sdk/rust/Cargo.toml` to remove `turso` dependency
  - [ ] Remove any turso feature flags
  - [ ] Run `cargo build` to verify

- [ ] Task 3: Remove Turso from Sandbox (AC: 3)
  - [ ] Edit `sandbox/Cargo.toml` to remove `turso` dependency
  - [ ] Remove any turso feature flags
  - [ ] Run `cargo build` to verify

- [ ] Task 4: Update Documentation (AC: 5, 7)
  - [ ] Update `docs/architecture/tech-stack.md`:
    - Remove SQLite/Turso from Storage Backends table
    - Update "Primary storage backend" description
    - Remove Turso-related dependency entries
  - [ ] Update `CLAUDE.md`:
    - Remove SQLite references
    - Update schema descriptions
    - Update sync documentation

- [ ] Task 5: Remove SQLite Schema (AC: 6)
  - [ ] Check if `schema/agentfs.sql` exists
  - [ ] Delete if present (DuckDB schema is `schema/duckagentfs.sql`)

- [ ] Task 6: Final Validation (AC: 4)
  - [ ] Run `cargo build` in workspace root
  - [ ] Run `cargo test` in all workspaces
  - [ ] Verify no turso in any Cargo.toml: `grep -r "turso" */Cargo.toml`
  - [ ] Verify no turso in lock files: `grep "turso" */Cargo.lock`

## Dev Notes

### Cargo.toml Cleanup

```toml
# REMOVE these lines from Cargo.toml files:
turso = { version = "0.4.3-pre.2", ... }

# REMOVE these from [features] if present:
turso-sync = ["turso/sync"]
```

### Documentation Updates

**tech-stack.md changes:**

```markdown
## Storage Backends

| Technology | Version | Purpose |
|------------|---------|---------|
| **DuckDB** | 1.1 | Primary storage backend for AgentFS |

# REMOVE the SQLite row entirely
```

**CLAUDE.md changes:**

- Update "stored in a single SQLite database" → "stored in a single DuckDB database"
- Update file extensions `.db` → `.duckdb`
- Remove Turso sync references

### Verification Commands

```bash
# Check no turso references remain
grep -r "turso" . --include="Cargo.toml"
grep -r "turso" . --include="Cargo.lock"
grep -ri "sqlite" docs/

# Full build test
cargo build --workspace
cargo test --workspace
```

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Missing turso usage | Low | High | Previous stories removed all usage |
| Build failure | Low | Medium | Incremental removal with testing |
| Doc inconsistency | Low | Low | Search and update all docs |

## Definition of Done

- [ ] All 6 tasks completed
- [ ] All 7 acceptance criteria verified
- [ ] Zero `turso` references in any Cargo.toml
- [ ] `cargo build --workspace` succeeds
- [ ] `cargo test --workspace` succeeds
- [ ] Documentation reflects DuckDB-only architecture
- [ ] Epic EPIC-SQLITE-REMOVAL marked complete

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
| 2026-01-17 | Story created | Final story in EPIC-SQLITE-REMOVAL |

---

## QA Results

(To be filled by QA agent)
