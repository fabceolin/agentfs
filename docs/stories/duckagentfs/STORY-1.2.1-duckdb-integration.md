# STORY-1.2.1: DuckDB Rust Crate Integration

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.2.1 |
| **Parent** | STORY-1.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Complete |
| **Priority** | Critical (Blocker) |
| **Estimated Effort** | Small (1-2 days) |
| **File** | `sdk/rust/src/filesystem/duckagentfs.rs` |
| **Dependencies** | STORY-1.1 |

## User Story

**As a** developer
**I want** the DuckDB Rust crate integrated into the SDK
**So that** DuckAgentFS can execute actual database queries

## Technical Description

Replace the placeholder `DuckConnection` and `DuckConnectionPool` types in `duckagentfs.rs` (lines 53-87) with actual DuckDB Rust crate types. This is the foundational blocker for all other STORY-1.2.x sub-stories.

## Acceptance Criteria

- [x] Add `duckdb = "1.1"` to `sdk/rust/Cargo.toml`
- [x] Replace placeholder `DuckConnection` with `duckdb::Connection`
- [x] Replace placeholder `DuckConnectionPool` with working pool implementation
- [x] Verify schema loads from `schema/duckagentfs.sql`
- [x] Single integration test proving DB connectivity and schema initialization

## Technical Specification

### Current Placeholder (Lines 53-87)

```rust
/// Placeholder for DuckDB connection
#[derive(Clone)]
pub struct DuckConnection {
    // In real implementation: duckdb::Connection
    _path: String,
}
```

### Target Implementation

```rust
use duckdb::{Connection, params};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

pub struct DuckConnectionPool {
    conn: Arc<Mutex<Connection>>,
    write_semaphore: Arc<Semaphore>,
}

impl DuckConnectionPool {
    pub async fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            write_semaphore: Arc::new(Semaphore::new(1)),
        })
    }

    pub async fn get_connection(&self) -> Result<impl std::ops::Deref<Target = Connection> + '_> {
        Ok(self.conn.lock().await)
    }

    pub async fn get_write_connection(&self) -> Result<WriteGuard<'_>> {
        let _permit = self.write_semaphore.acquire().await?;
        let conn = self.conn.lock().await;
        Ok(WriteGuard { conn, _permit })
    }
}
```

### Schema Initialization

Update `init_schema()` to execute the actual DDL:

```rust
async fn init_schema(&self) -> Result<()> {
    let conn = self.pool.get_write_connection().await?;
    conn.execute_batch(include_str!("../../../schema/duckagentfs.sql"))?;
    Ok(())
}
```

## Tests

### Test 1: Connection and Schema

```rust
#[tokio::test]
async fn test_duckdb_integration() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_string_lossy().to_string(),
        ..Default::default()
    };

    let fs = DuckAgentFS::open(config).await.unwrap();

    // Verify schema loaded - root directory should exist
    let root_stats = fs.stat("/").await.unwrap();
    assert!(root_stats.is_some());
    assert!(root_stats.unwrap().is_directory());
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/Cargo.toml` | Add duckdb dependency |
| `sdk/rust/src/filesystem/duckagentfs.rs` | Replace placeholder types |
| `schema/duckagentfs.sql` | Schema to load |

## Implementation Notes

1. **Cargo.toml Addition**:
   ```toml
   [dependencies]
   duckdb = { version = "1.1", features = ["bundled"] }
   ```

2. **Feature Flag**: Consider `bundled` feature for easier builds, or system lib for smaller binary.

3. **Error Mapping**: Map `duckdb::Error` to `crate::error::Error` appropriately.

## Dev Agent Record

### Agent Model Used
Claude Opus 4.5 (claude-opus-4-5-20251101)

### Tasks Completed
- [x] Add `duckdb = "1.1"` to `sdk/rust/Cargo.toml`
- [x] Replace placeholder `DuckConnection` with `duckdb::Connection`
- [x] Replace placeholder `DuckConnectionPool` with working pool implementation
- [x] Update `init_schema()` to load schema from `../../../../schema/duckagentfs.sql`
- [x] Add integration tests (`test_duckdb_integration`, `test_connection_pool`, `test_schema_tables_exist`)
- [x] Add `DuckDB` variant to `Error` enum in `error.rs`
- [x] Add module export in `filesystem/mod.rs`
- [x] Resolve async/sync incompatibility with DuckDB Connection using `spawn_blocking`

### Debug Log References
- **Issue**: DuckDB `Connection` is not `Send` or `Sync`
  - The `duckdb::Connection` type uses `RefCell` internally
  - Cannot be held across `.await` points in async code
  - **RESOLVED**: Used `tokio::task::spawn_blocking` pattern for all DB operations

### Completion Notes
**COMPLETED**: All acceptance criteria met. The async/sync incompatibility was resolved using Option B - `tokio::task::spawn_blocking` for all database operations.

**Solution Applied**:
- Converted all internal DB helper methods to synchronous (`_sync` suffix)
- Wrapped all `FileSystem` trait methods with `spawn_blocking`
- Clone pool/config before moving into blocking closures
- All 5 integration tests pass in Docker

### Change Log
| Date | Change |
|------|--------|
| 2026-01-15 | Added duckdb dependency, implemented DuckConnectionPool, updated init_schema |
| 2026-01-15 | Fixed schema include_str! path (4 levels up) |
| 2026-01-15 | Removed .await from sync pool methods |
| 2026-01-15 | Identified async/sync incompatibility blocker |
| 2026-01-15 | Resolved using spawn_blocking pattern for all DB operations |
| 2026-01-15 | Fixed schema FK reference to view (code_symbols table) |
| 2026-01-15 | All 5 tests passing in Docker |

### File List
| File | Status |
|------|--------|
| `sdk/rust/Cargo.toml` | Modified (added duckdb, preserved turso) |
| `sdk/rust/src/error.rs` | Modified (added DuckDB and Custom variants) |
| `sdk/rust/src/filesystem/mod.rs` | Modified (added duckagentfs module) |
| `sdk/rust/src/filesystem/duckagentfs.rs` | Modified (full spawn_blocking implementation) |
| `schema/duckagentfs.sql` | Modified (removed FK to view)

## Created By

Sprint Change Proposal SCP-2026-01-14-STORY-1.2
