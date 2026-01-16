# Story: Rename CLI and SDK Rust from agentfs to markdownfs

## Status

Approved

## Story

**As a** developer using the markdownfs tool,
**I want** the CLI executable, SDK Rust internals, and project identity renamed from `agentfs` to `markdownfs`,
**so that** the tool name reflects its primary purpose of managing markdown documents with GraphDocs.

## Story Context

**Existing System Integration:**
- Integrates with: CLI binary, SDK Rust library, environment variables, data directory conventions
- Technology: Rust (Cargo), shell environment
- Follows pattern: Standard Rust binary naming via Cargo.toml
- Touch points: CLI entry point, SDK Rust lib.rs, environment variable checks, directory path construction

**Rationale:**
This project is forking from `tursodatabase/agentfs` to focus specifically on GraphDocs markdown document management. The rename establishes a distinct identity aligned with the project's specialized purpose.

**Scope Note:**
This story covers CLI + SDK Rust only. SDKs TypeScript and Python will be migrated in a separate future story (STORY-MIGRATE-SDKS).

## Acceptance Criteria

### Functional Requirements

1. CLI binary is named `markdownfs` after compilation (`cargo build` produces `markdownfs` executable)
2. CLI help text and version output show `markdownfs` instead of `agentfs`
3. Environment variable `MARKDOWNFS=1` is set inside sandbox (replacing `AGENTFS=1`)
4. Environment variable `MARKDOWNFS_SANDBOX` replaces `AGENTFS_SANDBOX`
5. Environment variable `MARKDOWNFS_SESSION` replaces `AGENTFS_SESSION`
6. Default data directory is `.markdownfs/` instead of `.agentfs/`
7. FUSE mount prefix is `markdownfs:` instead of `agentfs:` in /proc/mounts

### Integration Requirements

8. SDK Rust `markdownfs_dir()` function returns `.markdownfs` path
9. SDK Rust mount detection uses `markdownfs:` prefix
10. Existing tests pass with renamed identifiers (both CLI and SDK Rust)
11. FUSE mount helper follows new naming convention (`mount.fuse.markdownfs`)
12. Internal library name in `cli/Cargo.toml` updated consistently

### Quality Requirements

13. All references to `agentfs` in CLI source code updated to `markdownfs`
14. All references to `agentfs` in SDK Rust source code updated to `markdownfs`
15. CLAUDE.md updated with new naming conventions
16. No hardcoded `agentfs` strings remain in CLI or SDK Rust crates (except crate name `agentfs-sdk`)

## Tasks / Subtasks

### CLI Tasks

- [ ] **Task 1: Update CLI Cargo.toml** (AC: 1, 12)
  - [ ] Change `[package] name` from `agentfs` to `markdownfs`
  - [ ] Change `[lib] name` from `agentfs` to `markdownfs`
  - [ ] Change `[[bin]] name` from `agentfs` to `markdownfs`
  - [ ] Update repository URL if known

- [ ] **Task 2: Update Environment Variables in CLI** (AC: 3, 4, 5)
  - [ ] Search for `AGENTFS` in CLI source code
  - [ ] Replace `AGENTFS=1` with `MARKDOWNFS=1`
  - [ ] Replace `AGENTFS_SANDBOX` with `MARKDOWNFS_SANDBOX`
  - [ ] Replace `AGENTFS_SESSION` with `MARKDOWNFS_SESSION`

- [ ] **Task 3: Update CLI Help/Branding** (AC: 2)
  - [ ] Update any hardcoded program name strings in help text
  - [ ] Verify `--version` output

- [ ] **Task 4: Update FUSE Mount Helper** (AC: 11)
  - [ ] Update `mount.fuse.agentfs` references to `mount.fuse.markdownfs`

### SDK Rust Tasks

- [ ] **Task 5: Update SDK Rust lib.rs** (AC: 6, 7, 8, 9)
  - [ ] Rename `agentfs_dir()` to `markdownfs_dir()`
  - [ ] Change return value from `.agentfs` to `.markdownfs`
  - [ ] Update `get_mounts()` to use `markdownfs:` prefix instead of `agentfs:`
  - [ ] Update all comments referencing `agentfs` to `markdownfs`

- [ ] **Task 6: Update SDK Rust References** (AC: 14)
  - [ ] Search for `agentfs` in all SDK Rust source files
  - [ ] Update struct/type names if any reference `agentfs`
  - [ ] Update error messages and documentation strings

- [ ] **Task 7: Run SDK Rust Tests** (AC: 10)
  - [ ] Run `cargo test` in `sdk/rust/`
  - [ ] Fix any test failures due to renamed functions/paths

### Documentation & Verification

- [ ] **Task 8: Update Documentation** (AC: 15)
  - [ ] Update CLAUDE.md with new naming
  - [ ] Update any CLI-specific docs

- [ ] **Task 9: Final Verification** (AC: 10, 13, 14, 16)
  - [ ] Run `cargo build` in `cli/` and verify binary name is `markdownfs`
  - [ ] Run `cargo test` in `cli/` and fix any failures
  - [ ] Run `cargo test` in `sdk/rust/` and fix any failures
  - [ ] Grep for remaining `agentfs` references in CLI crate
  - [ ] Grep for remaining `agentfs` references in SDK Rust (excluding crate name)
  - [ ] Manual smoke test of basic commands

## Dev Notes

### Relevant Source Tree

```
cli/
├── Cargo.toml              # Package/binary naming
├── src/
│   ├── main.rs             # Entry point, env var checks
│   ├── lib.rs              # Library exports
│   ├── parser.rs           # CLI argument definitions
│   ├── fuse.rs             # FUSE implementation, mount naming
│   ├── handler.rs          # Request handler
│   └── cmd/
│       ├── init.rs         # May reference directory
│       ├── run.rs          # Sandbox env vars
│       └── mount.rs        # Mount helper naming

sdk/rust/
├── Cargo.toml              # Keep as agentfs-sdk (internal use)
├── src/
│   ├── lib.rs              # agentfs_dir(), get_mounts() - MAIN CHANGES
│   ├── filesystem/         # May have references
│   ├── kvstore.rs
│   ├── toolcalls.rs
│   └── graphdocs/          # Your main focus area
```

### Key SDK Rust Changes (lib.rs)

```rust
// BEFORE (line 29-31)
pub fn agentfs_dir() -> &'static std::path::Path {
    std::path::Path::new(".agentfs")
}

// AFTER
pub fn markdownfs_dir() -> &'static std::path::Path {
    std::path::Path::new(".markdownfs")
}

// BEFORE (line 55-56)
if parts.len() >= 2 && parts[0].starts_with("agentfs:") {
    let agent_id = parts[0].strip_prefix("agentfs:")?;

// AFTER
if parts.len() >= 2 && parts[0].starts_with("markdownfs:") {
    let agent_id = parts[0].strip_prefix("markdownfs:")?;
```

### Environment Variable Locations (Expected)

- Sandbox setup code sets `AGENTFS=1` → `MARKDOWNFS=1`
- Sandbox type stored in `AGENTFS_SANDBOX` → `MARKDOWNFS_SANDBOX`
- Session ID in `AGENTFS_SESSION` → `MARKDOWNFS_SESSION`

### Testing

- **CLI tests:** `cd cli && cargo test`
- **SDK Rust tests:** `cd sdk/rust && cargo test`
- **Framework:** Standard Rust test framework
- **Verify:** All existing tests pass after rename

## Risk Assessment

**Primary Risk:** Breaking existing users/scripts that reference `agentfs` binary name
**Mitigation:** This is intentional divergence from upstream; document the change
**Rollback:** Revert Cargo.toml and source changes via git

**Secondary Risk:** SDK Rust function rename (`agentfs_dir` → `markdownfs_dir`) breaks CLI compilation
**Mitigation:** Update CLI calls to SDK in same PR

## Definition of Done

- [ ] `cargo build` in `cli/` produces `markdownfs` binary
- [ ] `./target/debug/markdownfs --version` shows correct name
- [ ] `cargo test` passes in both `cli/` and `sdk/rust/`
- [ ] No `agentfs` strings in CLI crate
- [ ] No `agentfs` strings in SDK Rust (except crate name in Cargo.toml)
- [ ] CLAUDE.md reflects new naming
- [ ] Data directory is `.markdownfs/`
- [ ] Mount prefix is `markdownfs:`

## Change Log

| Date | Version | Description | Author |
|------|---------|-------------|--------|
| 2026-01-16 | 0.1 | Initial story draft | Sarah (PO Agent) |
| 2026-01-16 | 0.2 | Added SDK Rust scope, updated tasks and ACs | Sarah (PO Agent) |
