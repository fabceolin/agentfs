# STORY-1.2: DuckAgentFS FileSystem Trait

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Needs Revision |
| **Status Notes** | QA validation failed: All filesystem operations return placeholder values; no integration tests exist; DuckDB SQL implementation not complete. Story split into sub-stories 1.2.1-1.2.4 per SCP-2026-01-14. Complete sub-stories before parent story can proceed. |
| **Priority** | Critical |
| **File** | `sdk/rust/src/filesystem/duckagentfs.rs` |
| **Dependencies** | STORY-1.1 |

## User Story

**As a** developer
**I want** an implementation of the `FileSystem` trait for DuckDB
**So that** I can use DuckAgentFS as a transparent backend

## Technical Description

Implement all methods of the `FileSystem` trait defined in `sdk/rust/src/filesystem/mod.rs`, using DuckDB as the backend and the append-only journal model for all mutations.

## Acceptance Criteria

- [ ] Integrate DuckDB Rust crate (`duckdb >= 1.1`) — see STORY-1.2.1
- [ ] Implement all methods of the `FileSystem` trait with actual DB queries — see STORY-1.2.2
- [ ] Use journal model for all mutations (verified via tests)
- [ ] Maintain DentryCache for performance (unit tested) — see STORY-1.2.4
- [ ] Support symlinks and hardlinks (integration tested) — see STORY-1.2.3
- [ ] Unit tests for DentryCache (6+ test cases)
- [ ] Integration tests for P0/P1 scenarios per test design doc

## Methods to Implement

### Read Operations

| Method | Description | Status |
|--------|-------------|--------|
| `stat(path)` | Stats following symlinks | [x] Implemented |
| `lstat(path)` | Stats without following symlinks | [x] Implemented |
| `read_file(path)` | Read complete file | [x] Implemented |
| `readdir(path)` | List directory | [x] Implemented |
| `readdir_plus(path)` | List with stats | [x] Implemented |
| `readlink(path)` | Read symlink target | [x] Implemented |
| `statfs()` | Filesystem stats | [x] Implemented |
| `open(path)` | Open file | [x] Implemented |

### Write Operations

| Method | Description | Status |
|--------|-------------|--------|
| `write_file(path, data)` | Write file | [x] Implemented |
| `mkdir(path)` | Create directory | [x] Implemented |
| `remove(path)` | Remove file/dir | [x] Implemented |
| `chmod(path, mode)` | Change permissions | [x] Implemented |
| `rename(from, to)` | Rename/move | [x] Implemented |
| `symlink(target, link)` | Create symlink | [x] Implemented |
| `link(old, new)` | Create hardlink | [x] Implemented |
| `create_file(path, mode)` | Create new file | [x] Implemented |

## Technical Specification

### Main Structure

```rust
pub struct DuckAgentFS {
    pool: DuckConnectionPool,
    config: DuckAgentFSConfig,
    dentry_cache: Arc<DentryCache>,
    embedding_generator: Arc<dyn EmbeddingGenerator>,
}

pub struct DuckAgentFSConfig {
    pub path: String,
    pub chunk_size: usize,
    pub dentry_cache_size: usize,
    pub enable_vss: bool,
    pub enable_pgq: bool,
    pub actor_id: Option<String>,
    pub session_id: Option<String>,
}
```

### Journal Event Append

All mutations must use `append_journal_event`:

```rust
async fn append_journal_event(
    &self,
    conn: &DuckConnection,
    inode: i64,
    event_type: &str,  // 'create', 'update', 'delete', 'rename', 'chmod'
    parent: Option<i64>,
    name: Option<&str>,
    mode: Option<u32>,
    size: Option<i64>,
    nlink: Option<u32>,
    old_parent: Option<i64>,
    old_name: Option<&str>,
) -> Result<i64>;
```

### Path Resolution

```rust
async fn resolve_path(&self, path: &str, follow_symlinks: bool) -> Result<Option<i64>> {
    // 1. Normalize path
    // 2. For each component:
    //    a. Check cache (dentry_cache)
    //    b. If not in cache, query fs_current
    //    c. If symlink and follow_symlinks, resolve recursively
    //    d. Update cache
    // 3. Return final inode or None
}
```

### DentryCache

```rust
struct DentryCache {
    entries: Mutex<LruCache<(i64, String), i64>>,  // (parent_ino, name) -> child_ino
}

impl DentryCache {
    fn get(&self, parent_ino: i64, name: &str) -> Option<i64>;
    fn insert(&self, parent_ino: i64, name: &str, child_ino: i64);
    fn remove(&self, parent_ino: i64, name: &str);
    fn clear(&self);
}
```

## Differences from AgentFS (SQLite)

| Aspect | AgentFS | DuckAgentFS |
|--------|---------|-------------|
| Mutation | `UPDATE fs_inode SET ...` | `INSERT INTO fs_journal ...` |
| State | Table `fs_inode` | View `fs_current` |
| Delete | `DELETE FROM fs_inode` | `INSERT ... event_type='delete'` |
| History | Lost | Preserved |

## Tests

### Test 1: Basic CRUD
```rust
#[tokio::test]
async fn test_basic_crud() {
    let fs = DuckAgentFS::open(config).await.unwrap();

    // Create
    fs.write_file("/test.txt", b"hello").await.unwrap();

    // Read
    let content = fs.read_file("/test.txt").await.unwrap();
    assert_eq!(content, Some(b"hello".to_vec()));

    // Update
    fs.write_file("/test.txt", b"world").await.unwrap();
    let content = fs.read_file("/test.txt").await.unwrap();
    assert_eq!(content, Some(b"world".to_vec()));

    // Delete
    fs.remove("/test.txt").await.unwrap();
    let content = fs.read_file("/test.txt").await.unwrap();
    assert!(content.is_none());
}
```

### Test 2: Directories
```rust
#[tokio::test]
async fn test_directories() {
    let fs = DuckAgentFS::open(config).await.unwrap();

    // Create nested directories
    fs.mkdir("/a").await.unwrap();
    fs.mkdir("/a/b").await.unwrap();
    fs.mkdir("/a/b/c").await.unwrap();

    // Write file in nested dir
    fs.write_file("/a/b/c/file.txt", b"content").await.unwrap();

    // Readdir
    let entries = fs.readdir("/a/b").await.unwrap().unwrap();
    assert!(entries.contains(&"c".to_string()));

    // Remove non-empty dir should fail
    let result = fs.remove("/a/b").await;
    assert!(result.is_err());
}
```

### Test 3: Symlinks
```rust
#[tokio::test]
async fn test_symlinks() {
    let fs = DuckAgentFS::open(config).await.unwrap();

    fs.write_file("/original.txt", b"content").await.unwrap();
    fs.symlink("/original.txt", "/link.txt").await.unwrap();

    // Read through symlink
    let content = fs.read_file("/link.txt").await.unwrap();
    assert_eq!(content, Some(b"content".to_vec()));

    // lstat vs stat
    let lstat = fs.lstat("/link.txt").await.unwrap().unwrap();
    assert!(lstat.is_symlink());

    let stat = fs.stat("/link.txt").await.unwrap().unwrap();
    assert!(stat.is_file());
}
```

### Test 4: Hardlinks
```rust
#[tokio::test]
async fn test_hardlinks() {
    let fs = DuckAgentFS::open(config).await.unwrap();

    fs.write_file("/file1.txt", b"content").await.unwrap();
    fs.link("/file1.txt", "/file2.txt").await.unwrap();

    // Both point to same inode
    let stat1 = fs.stat("/file1.txt").await.unwrap().unwrap();
    let stat2 = fs.stat("/file2.txt").await.unwrap().unwrap();
    assert_eq!(stat1.ino, stat2.ino);
    assert_eq!(stat1.nlink, 2);
}
```

### Test 5: Rename
```rust
#[tokio::test]
async fn test_rename() {
    let fs = DuckAgentFS::open(config).await.unwrap();

    fs.write_file("/old.txt", b"content").await.unwrap();
    fs.rename("/old.txt", "/new.txt").await.unwrap();

    // Old path should not exist
    let old = fs.stat("/old.txt").await.unwrap();
    assert!(old.is_none());

    // New path should exist with same content
    let content = fs.read_file("/new.txt").await.unwrap();
    assert_eq!(content, Some(b"content".to_vec()));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/filesystem/mod.rs` | FileSystem trait |
| `sdk/rust/src/filesystem/duckagentfs.rs` | Implementation |
| `sdk/rust/src/filesystem/agentfs.rs` | Reference (SQLite) |
| `schema/duckagentfs.sql` | Schema used |

## Implementation Notes

1. **Connection Pool**: Use pool with semaphore for DuckDB single-writer semantics

2. **Error Handling**: Map DuckDB errors to `FsError`:
   ```rust
   match duckdb_error {
       DuckDBError::NotFound => FsError::NotFound,
       DuckDBError::Constraint => FsError::AlreadyExists,
       // ...
   }
   ```

3. **Chunk Size**: Use 4KB chunks for file data, similar to AgentFS

4. **Embedding Integration**: Call `update_embedding()` after `write_file()` if VSS is enabled

## QA Notes

**Review Date:** 2026-01-14
**Updated:** 2026-01-14
**Reviewer:** Quinn (QA Agent)
**Story Status:** Needs Revision (Split into sub-stories)
**Test Design Doc:** [STORY-1.2-filesystem-trait-test-design.md](../../qa/STORY-1.2-filesystem-trait-test-design.md)

### Test Coverage Summary

| Category | Coverage | Status | Sub-Story |
|----------|----------|--------|-----------|
| Infrastructure (DuckDB Integration) | Not implemented | ❌ Blocked | 1.2.1 |
| Unit Tests (Path Utilities) | 2 tests present | ⚠️ Minimal | - |
| DentryCache Unit Tests | Logic implemented | ❌ Not Tested | 1.2.4 |
| Core CRUD Operations | Placeholders only | ❌ Not Tested | 1.2.2 |
| Path Resolution | Placeholders only | ❌ Not Tested | 1.2.3 |
| Symlink/Hardlink | Placeholders only | ❌ Not Tested | 1.2.3 |
| Time-Travel (Snapshots) | Placeholders only | ❌ Future | - |
| VSS Integration | Placeholders only | ❌ Future | - |

**Current State:** The implementation file (`duckagentfs.rs`) contains a well-structured conceptual implementation with placeholder/stub methods. Only 2 unit tests exist (`test_normalize_path`, `test_split_path`) covering path utility functions. The 5 integration test scenarios defined in the story (Basic CRUD, Directories, Symlinks, Hardlinks, Rename) are documented but **not implemented as actual test code**.

### Risk Areas Identified

| Risk | Severity | Probability | Impact | Mitigation | Sub-Story |
|------|----------|-------------|--------|------------|-----------|
| **Placeholder Methods** | HIGH | 100% | All operations return empty/default values | Complete DuckDB integration before testing | 1.2.1, 1.2.2 |
| **No DuckDB Integration** | HIGH | 100% | Cannot validate actual filesystem behavior | Implement actual SQL queries with duckdb crate | 1.2.1 |
| **DentryCache Concurrency** | MEDIUM | 40% | Mutex contention under load | Benchmark with concurrent operations | 1.2.4 |
| **Symlink Loop Detection** | MEDIUM | 30% | MAX_SYMLINK_DEPTH=40 may be excessive | Unit test boundary conditions | 1.2.3 |
| **Journal Event Integrity** | HIGH | 60% | Data loss if journal append fails mid-operation | Add transaction boundaries, test rollback | 1.2.2 |
| **Connection Pool Semantics** | MEDIUM | 50% | Write semaphore not actually implemented | Test single-writer guarantee under concurrency | 1.2.1 |
| **Snapshot Read Consistency** | MEDIUM | 40% | Snapshot delegates to current fs methods | Implement event_id filtering in queries | Future |

### Recommended Test Scenarios

Test scenarios mapped to sub-stories per test design document:

#### P0 - Critical (Must Have Before Release)

| Test ID | Scenario | Sub-Story |
|---------|----------|-----------|
| TC-INFRA-001 | DuckDB Connection and Schema Initialization | 1.2.1 |
| TC-P0-001 | Basic CRUD E2E | 1.2.2 |
| TC-P0-002 | Journal Append Atomicity | 1.2.2 |
| TC-P0-003 | Path Resolution with DentryCache | 1.2.3 |
| TC-P0-004 | Symlink Resolution Limits | 1.2.3 |
| TC-P0-005 | Root Directory Protection | 1.2.2 |

#### P1 - High (Required for Production)

| Test ID | Scenario | Sub-Story |
|---------|----------|-----------|
| TC-P1-001 | Concurrent Write Serialization | 1.2.2 |
| TC-P1-002 | Hardlink nlink Tracking | 1.2.3 |
| TC-P1-003 | Rename Across Directories | 1.2.3 |
| TC-P1-004 | DentryCache Invalidation on Rename | 1.2.3, 1.2.4 |
| TC-P1-005 | Directory Non-Empty Check | 1.2.2 |
| TC-P1-006 | Symlink Target Storage and Retrieval | 1.2.3 |

#### P2 - Medium (Quality Enhancement)

| Test ID | Scenario | Sub-Story |
|---------|----------|-----------|
| TC-P2-001 | Time-Travel Snapshot Isolation | Future |
| TC-P2-002 | VSS Embedding Update on Write | Future |
| TC-P2-003 | Large File Chunking | 1.2.2 |
| TC-P2-004 | mkdir_recursive Idempotence | 1.2.2 |
| TC-P2-005 | Snapshot Read-Only Enforcement | Future |

### Concerns and Blockers

| Type | Description | Severity | Action Required |
|------|-------------|----------|-----------------|
| **BLOCKER** | All filesystem operations return placeholder values | Critical | Complete DuckDB SQL implementation (1.2.1, 1.2.2) |
| **BLOCKER** | No integration tests exist | Critical | Implement test suite before marking story complete |
| **BLOCKER** | `DuckConnectionPool` is placeholder | Critical | Integrate `duckdb` crate (1.2.1) |
| **BLOCKER** | `lookup_child` always returns None | Critical | Implement actual SQL query (1.2.2) |
| **BLOCKER** | `stat_inode` always returns None | Critical | Implement fs_current query (1.2.2) |
| **BLOCKER** | `write_data_chunks` is no-op | Critical | Implement chunk storage (1.2.2) |
| **BLOCKER** | `read_symlink_target` returns None | High | Implement fs_data query (1.2.3) |
| **CONCERN** | Snapshot `read_file` delegates to current fs (no event_id filtering) | Medium | Implement proper time-travel in snapshot methods |
| **CONCERN** | `write_data_chunks` deletes before insert (not atomic) | Medium | Wrap in transaction |
| **NOTE** | Schema file `schema/duckagentfs.sql` referenced but not verified | Low | Confirm schema compatibility |

### Test Infrastructure Requirements

- [ ] DuckDB test fixture with in-memory database
- [ ] Mock `EmbeddingGenerator` for VSS tests
- [ ] Concurrent operation test harness (tokio test runtime)
- [ ] Journal event verification helper functions
- [ ] Snapshot comparison utilities

### Test Execution Gating

| Gate | Milestone | Sub-Stories Required | Status |
|------|-----------|---------------------|--------|
| Gate 1 | Pre-Alpha | 1.2.1 + 1.2.4 | ❌ Not Started |
| Gate 2 | Alpha | 1.2.2 complete | ❌ Blocked |
| Gate 3 | Beta | 1.2.3 complete | ❌ Blocked |
| Gate 4 | Release Candidate | All P0+P1+P2 | ❌ Blocked |

### Recommendation

**Gate Status:** 🚫 **BLOCKED** - Story cannot proceed until sub-stories completed:
1. ✅ Story split into sub-stories (SCP-2026-01-14)
2. ⏳ STORY-1.2.1: DuckDB Rust Crate Integration (Ready for Development)
3. ⏳ STORY-1.2.4: DentryCache Unit Tests (Ready for Development - parallel track)
4. ⏳ STORY-1.2.2: Core CRUD Implementation (Blocked by 1.2.1)
5. ⏳ STORY-1.2.3: Links and Path Resolution (Blocked by 1.2.2)

---

## Remediation Plan (SCP-2026-01-14)

This story has been split into focused sub-stories to address the identified blockers:

| Sub-Story | Description | Status | Blocks |
|-----------|-------------|--------|--------|
| [STORY-1.2.1](STORY-1.2.1-duckdb-integration.md) | DuckDB Rust Crate Integration | Ready for Development | 1.2.2, 1.2.3 |
| [STORY-1.2.2](STORY-1.2.2-core-crud.md) | Core CRUD Implementation | Blocked by 1.2.1 | 1.2.3 |
| [STORY-1.2.3](STORY-1.2.3-links-paths.md) | Links and Path Resolution | Blocked by 1.2.2 | - |
| [STORY-1.2.4](STORY-1.2.4-dentry-tests.md) | DentryCache Unit Tests | Ready for Development | - |

### Execution Order

```
STORY-1.2.1 ──────────────────────────► STORY-1.2.2 ──► STORY-1.2.3
    │                                        │
    └─ STORY-1.2.4 (parallel) ◄──────────────┘
```

### Completion Criteria

STORY-1.2 will be marked **Complete** when:
1. All four sub-stories pass their acceptance criteria
2. QA gates from `docs/qa/STORY-1.2-filesystem-trait-test-design.md` are satisfied:
   - Gate 1 (Pre-Alpha): All P0 tests passing
   - Gate 2 (Alpha): All P0 + P1 tests passing

### Reference Documents

- [Test Design Document](../../qa/STORY-1.2-filesystem-trait-test-design.md)
- [DuckDB Rust Client Documentation](https://duckdb.org/docs/stable/clients/rust)

### Created By

Sprint Change Proposal SCP-2026-01-14-STORY-1.2
Date: 2026-01-14
