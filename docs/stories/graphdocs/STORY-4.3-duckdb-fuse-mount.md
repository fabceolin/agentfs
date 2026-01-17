# STORY-4.3: DuckDB FUSE Mount with GraphDocs

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-4.3 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 4 - FUSE Handler |
| **Status** | Draft |
| **Priority** | High |
| **File** | `cli/src/cmd/mount.rs` |
| **Dependencies** | STORY-4.1, STORY-4.2 |

## User Story

**As a** user
**I want** to mount a DuckDB-based AgentFS with GraphDocs support
**So that** I can access documents via `/.graphdocs/` virtual directory in the filesystem

## Story Context

**Gap Identified:** The current mount command uses SQLite-based AgentFS and does not:
1. Support DuckDB databases (DuckAgentFS)
2. Register `GraphDocsHandler` with the FUSE handler registry
3. Inject `/.graphdocs/` virtual directory into directory listings

**Migration Decision:** Remove SQLite (AgentFS) support from mount command entirely. DuckDB (DuckAgentFS) is the sole database backend going forward.

**Existing System Integration:**
- Integrates with: `cli/src/cmd/mount.rs`, `cli/src/fuse.rs`, `cli/src/handler.rs`
- Technology: Rust, DuckDB, FUSE
- Follows pattern: Existing mount command structure
- Touch points: `mount()` function, `HandlerRegistry`, `GraphDocsHandler`

## Acceptance Criteria

- [ ] Mount command uses DuckDB/DuckAgentFS exclusively (remove SQLite/AgentFS support)
- [ ] `GraphDocsHandler` is registered when DuckDB database contains GraphDocs tables
- [ ] `GraphDocsDirInjector` injects `/.graphdocs/` into root directory listings
- [ ] `ls /.graphdocs/` lists all documents from `gd_documents` table
- [ ] `cat /.graphdocs/{doc_id}.gd.md` renders document via `GraphDocsEngine`
- [ ] Remove unused SQLite imports and `AgentFSOptions` from mount.rs

## Tasks / Subtasks

- [ ] Task 1: Replace SQLite with DuckDB in mount command (AC: 1, 6)
  - [ ] Remove `AgentFSOptions::resolve` and SQLite-based `open_agentfs` usage
  - [ ] Add `DuckAgentFS::open()` to open DuckDB databases directly
  - [ ] Remove unused imports: `agentfs_sdk::{AgentFSOptions, FileSystem as SqliteFS}`
  - [ ] Update error messages to reference DuckDB

- [ ] Task 2: Register GraphDocsHandler (AC: 2, 3)
  - [ ] Check if `gd_documents` table exists in DuckDB database
  - [ ] Create `HandlerRegistry` with `GraphDocsHandler`
  - [ ] Wrap default handler with `GraphDocsDirInjector` for root directory injection
  - [ ] Pass registry to `fuse::mount()` instead of `None`

- [ ] Task 3: Integration testing (AC: 4, 5)
  - [ ] Test `ls /.graphdocs/` returns document list
  - [ ] Test `cat /.graphdocs/readme.gd.md` renders document
  - [ ] Test mount works for DuckDB database without GraphDocs tables (no handler)

## Technical Specification

### Handler Registration

```rust
// cli/src/cmd/mount.rs

use crate::handler::{DefaultHandler, GraphDocsHandler, GraphDocsDirInjector, HandlerRegistry};
use agentfs_sdk::DuckAgentFS;

fn create_handler_registry(
    fs: Arc<dyn FileSystem>,
    pool: &DuckConnectionPool,
) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    // Check if GraphDocs tables exist
    let has_graphdocs = pool.get_connection()
        .and_then(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'gd_documents'",
                [],
                |r| r.get::<_, i64>(0)
            ).ok()
        })
        .unwrap_or(0) > 0;

    if has_graphdocs {
        // Register GraphDocsHandler for /.graphdocs/ virtual directory
        let graphdocs_handler = Arc::new(GraphDocsHandler::new(pool.clone()));
        registry.register(graphdocs_handler);
    }

    // Wrap default handler with GraphDocsDirInjector to add /.graphdocs/ to ls /
    let default_handler = Arc::new(DefaultHandler::new(fs));
    let injector = Arc::new(GraphDocsDirInjector::new(default_handler));
    registry.register(injector);

    registry
}
```

### Updated Mount Function (DuckDB-only)

```rust
// cli/src/cmd/mount.rs

use agentfs_sdk::DuckAgentFS;

pub fn mount(args: MountArgs) -> Result<()> {
    // Open DuckDB database directly (SQLite support removed)
    let pool = DuckConnectionPool::open(&args.id_or_path)
        .context("Failed to open DuckDB database")?;

    let fs: Arc<dyn FileSystem> = Arc::new(DuckAgentFS::new(pool.clone()));

    // Create handler registry with GraphDocs support if tables exist
    let handler_registry = create_handler_registry(fs.clone(), &pool);

    // ... existing FUSE options setup ...

    crate::fuse::mount(fs, fuse_opts, rt, Some(handler_registry))
}
```

### Removed Code

```rust
// REMOVE these from cli/src/cmd/mount.rs:
use agentfs_sdk::{AgentFSOptions, FileSystem as SqliteFS};  // Remove
use crate::cmd::init::open_agentfs;  // Remove

// REMOVE SQLite-specific logic:
let opts = AgentFSOptions::resolve(&args.id_or_path)?;  // Remove
let agentfs = rt.block_on(open_agentfs(opts))?;  // Remove
```

## Dev Notes

### Source Tree Reference

```
cli/src/
├── cmd/
│   └── mount.rs          # Mount command - UPDATE THIS
├── fuse.rs               # FUSE filesystem - accepts HandlerRegistry
└── handler.rs            # GraphDocsHandler, GraphDocsDirInjector - ALREADY DONE
```

### Key Integration Points

1. **`mount.rs` line 117**: Currently passes `None` for handler_registry
2. **`fuse.rs` line 1936**: `mount()` accepts `Option<HandlerRegistry>`
3. **`handler.rs`**: `GraphDocsHandler::new(pool)` and `GraphDocsDirInjector::new(handler)`

### Testing

- Test file location: `cli/src/cmd/mount.rs` (integration tests)
- Test standards: Tokio async tests
- Framework: cargo test with FUSE mocking or real mount tests

## Risk Assessment

**Primary Risk:** SQLite databases will no longer be mountable
**Mitigation:** This is intentional - DuckAgentFS is the new standard; SQLite/AgentFS is deprecated
**Rollback:** Revert changes to mount.rs if critical issues found

**Breaking Change:** Existing SQLite-based `.agentfs/*.db` files cannot be mounted after this change. Users must migrate to DuckDB format.

## Definition of Done

- [ ] DuckDB databases can be mounted with `agentfs mount <path.duckdb> <mountpoint>`
- [ ] `ls /.graphdocs/` shows documents from `gd_documents` table
- [ ] `cat /.graphdocs/{id}.gd.md` renders document via GraphDocsEngine
- [ ] SQLite imports and code paths removed from mount.rs
- [ ] Tests pass (existing SQLite tests removed/updated, new DuckDB tests added)
- [ ] Code follows existing patterns and standards

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
| 2026-01-17 | Story created | Fill gap identified during STORY-2.3 demo |
| 2026-01-17 | Updated to remove SQLite support | Migration to DuckDB-only per user direction |

---

## QA Results

(To be filled by QA agent)
