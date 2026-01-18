# STORY-6.6: DuckDB CLI Namespace and Auto-Initialize

## Status

Ready for Review

## Story

**As a** CLI user,
**I want** a dedicated `agentfs duckdb` subcommand namespace with `init` command and auto-create behavior on mount/graphdocs,
**So that** I can easily create and manage DuckDB databases, with a clean namespace ready for additional backends in the future.

## Acceptance Criteria

### AC1: `agentfs duckdb init` Command
1. Command `agentfs duckdb init <agent-id>` creates `.agentfs/<id>.duckdb`
2. Optional flags: `--vss` (enable VSS extension), `--pgq` (enable PGQ extension)
3. Creates `.agentfs/` directory if it doesn't exist
4. Error if database already exists (unless `--force` flag)
5. Success message shows database path

### AC2: Auto-Create on Mount
6. `agentfs mount <agent-id>` auto-creates `.agentfs/<id>.duckdb` if not found
7. Info message shown when database is auto-created
8. Uses default config (no VSS/PGQ unless specified via new flags)

### AC3: Auto-Create on GraphDocs
9. `agentfs graphdocs <agent-id> <cmd>` auto-creates database if not found
10. Info message shown when database is auto-created

### AC4: CLI Help and Consistency
11. `agentfs duckdb --help` shows available subcommands
12. Help text includes examples
13. Consistent error messages across all commands

## Tasks / Subtasks

- [x] **Task 1: Add `duckdb` subcommand namespace** (AC: 1, 11, 12)
  - [x] Add `DuckdbCommand` enum in `cli/src/parser.rs`
  - [x] Add `duckdb` variant to main `Command` enum
  - [x] Create `cli/src/cmd/duckdb.rs` module
  - [x] Wire up in `cli/src/main.rs`

- [x] **Task 2: Implement `duckdb init` handler** (AC: 1, 2, 3, 4, 5)
  - [x] Implement `handle_duckdb_init()` in `cli/src/cmd/duckdb.rs`
  - [x] Call `DuckAgentFS::open()` with appropriate config
  - [x] Handle existing database (error without `--force`)
  - [x] Print success message with path

- [x] **Task 3: Add auto-create to mount command** (AC: 6, 7, 8)
  - [x] Modify `resolve_db_path()` in `cli/src/cmd/mount.rs`
  - [x] Create database if not found (call `DuckAgentFS::open()`)
  - [x] Print info message on auto-create

- [x] **Task 4: Add auto-create to graphdocs command** (AC: 9, 10)
  - [x] Modify `open_duckagentfs()` in `cli/src/cmd/graphdocs.rs`
  - [x] Create database if not found
  - [x] Print info message on auto-create

- [x] **Task 5: Add integration tests** (AC: all)
  - [x] Test `duckdb init` creates database
  - [x] Test `duckdb init --force` overwrites
  - [x] Test mount auto-creates
  - [x] Test graphdocs auto-creates

## Dev Notes

### Existing Pattern Reference

The `graphdocs` command provides a good pattern for subcommand namespaces:

```rust
// cli/src/parser.rs - existing pattern
#[derive(Subcommand)]
pub enum GraphDocsCommand {
    List { ... },
    Create { ... },
    // ...
}
```

### Key Files to Modify

| File | Change |
|------|--------|
| `cli/src/parser.rs` | Add `DuckdbCommand` enum |
| `cli/src/main.rs` | Add command dispatch |
| `cli/src/cmd/duckdb.rs` | **New file** - init handler |
| `cli/src/cmd/mount.rs` | Auto-create logic |
| `cli/src/cmd/graphdocs.rs` | Auto-create logic |
| `cli/src/cmd/mod.rs` | Export new module |

### SDK Integration

The SDK already provides everything needed:

```rust
// sdk/rust/src/filesystem/duckagentfs.rs:248-278
pub async fn open(config: DuckAgentFSConfig) -> Result<Self> {
    // Creates database and initializes schema automatically
}
```

### Testing

- Test file location: `cli/tests/` or inline tests in `cli/src/cmd/duckdb.rs`
- Test pattern: Create temp directory, run command, verify `.duckdb` file exists
- Use `tempfile` crate for test isolation

## Definition of Done

- [x] All acceptance criteria met
- [x] `cargo build` succeeds
- [x] `cargo test` passes (131 tests)
- [x] `cargo clippy` has no new warnings (7 pre-existing CLI warnings, 2 SDK warnings)
- [x] Manual testing: `agentfs duckdb init test && agentfs mount test /tmp/mnt`
- [x] Help text is clear and includes examples

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| Breaking existing workflows | Auto-create uses same code path as explicit init |
| Extension loading failures | VSS/PGQ are optional, errors are warnings only |

## Change Log

| Date | Version | Description | Author |
|------|---------|-------------|--------|
| 2026-01-17 | 1.0 | Initial draft | PO Agent (Sarah) |
| 2026-01-17 | 1.1 | Implementation complete | Dev Agent (James) |

---

## Dev Agent Record

### Agent Model Used

Claude Opus 4.5 (claude-opus-4-5-20251101)

### Debug Log References

No blocking issues encountered. Pre-existing bug in graphdocs.rs (`CheckArgs` undefined) was fixed as a dependency to allow compilation.

### Completion Notes

1. **Task 1-2**: Created `cli/src/cmd/duckdb.rs` with `DuckdbCommand::Init` subcommand, wired up in parser.rs and main.rs
2. **Task 3**: Modified `resolve_db_path()` in mount.rs to auto-create database and return `(path, auto_created)` tuple
3. **Task 4**: Modified `open_duckagentfs()` in graphdocs.rs to auto-create database with info message
4. **Task 5**: Added 7 integration tests in duckdb.rs covering init, force overwrite, and path resolution
5. **Pre-existing bug fix**: Added missing `CheckArgs` struct and `handle_check` stub in graphdocs.rs to fix compilation

### File List

| File | Action | Description |
|------|--------|-------------|
| `cli/src/cmd/duckdb.rs` | Created | New duckdb subcommand module with init handler |
| `cli/src/cmd/mod.rs` | Modified | Export duckdb module |
| `cli/src/parser.rs` | Modified | Add DuckdbCommand enum and Duckdb variant |
| `cli/src/main.rs` | Modified | Add command dispatch for Duckdb |
| `cli/src/cmd/mount.rs` | Modified | Auto-create logic in resolve_db_path |
| `cli/src/cmd/graphdocs.rs` | Modified | Auto-create logic in open_duckagentfs, added CheckArgs struct and handle_check stub |

### Test Results

- All 131 tests pass
- 7 new tests added for duckdb module
- Manual testing verified: `agentfs duckdb init`, `agentfs mount` auto-create, `agentfs graph-docs` auto-create
