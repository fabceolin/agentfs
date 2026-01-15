# Test Design: STORY-1.2 - DuckAgentFS FileSystem Trait

> **Generated:** 2026-01-14
> **Updated:** 2026-01-14
> **Story:** STORY-1.2-filesystem-trait.md
> **Implementation:** `sdk/rust/src/filesystem/duckagentfs.rs`
> **Status:** Ready for Implementation

---

## 1. Executive Summary

This document provides a comprehensive test design for the DuckAgentFS FileSystem trait implementation. The implementation uses DuckDB as the storage backend with an append-only journal model, supporting time-travel, vector similarity search (VSS), and property graphs (DuckPGQ).

### 1.1 Sub-Story Mapping

| Sub-Story | Description | Test Coverage |
|-----------|-------------|---------------|
| STORY-1.2.1 | DuckDB Rust Crate Integration | TC-INFRA-001 |
| STORY-1.2.2 | Core CRUD Implementation | TC-P0-001, TC-P0-002, TC-P0-005, TC-P1-005 |
| STORY-1.2.3 | Links and Path Resolution | TC-P0-003, TC-P0-004, TC-P1-001 through TC-P1-006 |
| STORY-1.2.4 | DentryCache Unit Tests | Section 4 (6 unit tests) |

### 1.2 Current State Analysis

| Aspect | Status | Notes |
|--------|--------|-------|
| **Implementation** | Conceptual/Placeholder | All methods return placeholder values |
| **Unit Tests** | 2 tests present | `test_normalize_path`, `test_split_path` only |
| **Integration Tests** | None | 5 scenarios documented but not implemented |
| **DuckDB Integration** | Not implemented | Placeholder connection pool |

### 1.3 Test Coverage Goals

| Priority | Category | Target Coverage | Sub-Story |
|----------|----------|-----------------|-----------|
| P0 | Core CRUD operations | 100% | 1.2.2 |
| P0 | Path resolution | 100% | 1.2.3 |
| P0 | Journal atomicity | 100% | 1.2.2 |
| P1 | Symlinks/Hardlinks | 100% | 1.2.3 |
| P1 | DentryCache | 90% | 1.2.3, 1.2.4 |
| P1 | Concurrency | 80% | 1.2.2 |
| P2 | Time-Travel/Snapshots | 80% | Future |
| P2 | VSS Integration | 70% | Future |
| P2 | Edge cases | 70% | 1.2.2, 1.2.3 |

---

## 2. Test Infrastructure Requirements

### 2.1 Test Fixtures

```rust
// tests/common/mod.rs

use std::sync::Arc;
use tempfile::TempDir;

/// Creates an in-memory DuckAgentFS instance for testing
pub async fn create_test_fs() -> (DuckAgentFS, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_string_lossy().to_string(),
        chunk_size: 4096,
        dentry_cache_size: 1000,
        enable_vss: false,
        enable_pgq: false,
        actor_id: Some("test-actor".to_string()),
        session_id: Some("test-session".to_string()),
    };

    let fs = DuckAgentFS::open(config).await.expect("Failed to open FS");
    (fs, temp_dir)
}

/// Creates a DuckAgentFS with VSS enabled for embedding tests
pub async fn create_test_fs_with_vss(
    embedding_gen: Arc<dyn EmbeddingGenerator>
) -> (DuckAgentFS, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_string_lossy().to_string(),
        chunk_size: 4096,
        dentry_cache_size: 1000,
        enable_vss: true,
        enable_pgq: false,
        actor_id: None,
        session_id: None,
    };

    let fs = DuckAgentFS::open_with_embeddings(config, embedding_gen)
        .await
        .expect("Failed to open FS with VSS");
    (fs, temp_dir)
}

/// Creates a test FS with custom chunk size for chunking tests
pub async fn create_test_fs_with_chunk_size(chunk_size: usize) -> (DuckAgentFS, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_string_lossy().to_string(),
        chunk_size,
        dentry_cache_size: 1000,
        enable_vss: false,
        enable_pgq: false,
        actor_id: None,
        session_id: None,
    };

    let fs = DuckAgentFS::open(config).await.expect("Failed to open FS");
    (fs, temp_dir)
}
```

### 2.2 Mock Embedding Generator

```rust
// tests/common/mock_embeddings.rs

use async_trait::async_trait;
use crate::error::Result;

/// Mock embedding generator for VSS tests
pub struct MockEmbeddingGenerator {
    pub dimension: usize,
    pub call_count: std::sync::atomic::AtomicUsize,
}

impl MockEmbeddingGenerator {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn get_call_count(&self) -> usize {
        self.call_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl EmbeddingGenerator for MockEmbeddingGenerator {
    async fn generate(&self, content: &str) -> Result<Vec<f32>> {
        self.call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // Generate deterministic embedding based on content hash
        let hash = content.bytes().fold(0u64, |acc, b| acc.wrapping_add(b as u64));
        let mut embedding = Vec::with_capacity(self.dimension);
        for i in 0..self.dimension {
            embedding.push(((hash + i as u64) % 1000) as f32 / 1000.0);
        }
        Ok(embedding)
    }

    fn model_name(&self) -> &str {
        "mock-embedding-v1"
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
```

### 2.3 Journal Verification Helpers

```rust
// tests/common/journal_helpers.rs

use duckdb::Connection;

/// Verify journal contains expected event
pub fn verify_journal_event(
    conn: &Connection,
    inode: i64,
    event_type: &str,
) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM fs_journal WHERE inode = ? AND event_type = ?",
        params![inode, event_type],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Get the latest event_id for an inode
pub fn get_latest_event_id(conn: &Connection, inode: i64) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT MAX(event_id) FROM fs_journal WHERE inode = ?",
        params![inode],
        |row| row.get(0),
    ).optional()
}

/// Count journal entries for an inode
pub fn count_journal_entries(conn: &Connection, inode: i64) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM fs_journal WHERE inode = ?",
        params![inode],
        |row| row.get(0),
    )
}

/// Count total journal entries
pub fn count_total_journal_entries(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM fs_journal",
        [],
        |row| row.get(0),
    )
}

/// Verify no partial journal entries exist (atomicity check)
pub fn verify_journal_integrity(conn: &Connection) -> Result<bool> {
    // Check that every inode has a complete event chain
    let orphaned: i64 = conn.query_row(
        r#"
        SELECT COUNT(*) FROM fs_journal j1
        WHERE event_type = 'update'
        AND NOT EXISTS (
            SELECT 1 FROM fs_journal j2
            WHERE j2.inode = j1.inode AND j2.event_type = 'create'
        )
        "#,
        [],
        |row| row.get(0),
    )?;
    Ok(orphaned == 0)
}

/// Get journal event types for an inode in order
pub fn get_event_types_for_inode(conn: &Connection, inode: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT event_type FROM fs_journal WHERE inode = ? ORDER BY event_id"
    )?;
    let rows = stmt.query_map(params![inode], |row| row.get(0))?;
    rows.collect()
}
```

### 2.4 Snapshot Comparison Utilities

```rust
// tests/common/snapshot_helpers.rs

/// Compare filesystem state at two different event IDs
pub async fn compare_snapshots(
    fs: &DuckAgentFS,
    event_id_a: i64,
    event_id_b: i64,
    path: &str,
) -> Result<SnapshotDiff> {
    let snap_a = fs.snapshot_at(event_id_a).await?;
    let snap_b = fs.snapshot_at(event_id_b).await?;

    let stats_a = snap_a.stat(path).await?;
    let stats_b = snap_b.stat(path).await?;

    Ok(SnapshotDiff {
        path: path.to_string(),
        exists_in_a: stats_a.is_some(),
        exists_in_b: stats_b.is_some(),
        size_a: stats_a.map(|s| s.size),
        size_b: stats_b.map(|s| s.size),
    })
}

pub struct SnapshotDiff {
    pub path: String,
    pub exists_in_a: bool,
    pub exists_in_b: bool,
    pub size_a: Option<i64>,
    pub size_b: Option<i64>,
}
```

---

## 3. Test Scenarios by Priority

### 3.1 Infrastructure Tests (STORY-1.2.1)

#### TC-INFRA-001: DuckDB Connection and Schema Initialization

**Objective:** Verify DuckDB crate integration and schema loading.

**Sub-Story:** STORY-1.2.1

**Preconditions:**
- DuckDB crate added to Cargo.toml

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `DuckAgentFSConfig` with temp path | Config created |
| 2 | Call `DuckAgentFS::open(config)` | Returns Ok |
| 3 | Query `fs_current` view | View exists and queryable |
| 4 | Query `fs_journal` table | Table exists |
| 5 | `stat("/")` | Returns root directory stats |

**Verification:**
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

---

### 3.2 P0 - Critical (Must Have Before Release)

#### TC-P0-001: Basic CRUD E2E

**Objective:** Validate create, read, update, delete cycle with real DuckDB backend.

**Sub-Story:** STORY-1.2.2

**Preconditions:**
- Empty DuckAgentFS instance with real DuckDB

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/test.txt", b"hello")` | Returns `Ok(())` |
| 2 | `read_file("/test.txt")` | Returns `Ok(Some(b"hello"))` |
| 3 | Query `fs_journal` | Contains `create` event for new inode |
| 4 | Query `fs_current` | File visible with correct size (5 bytes) |
| 5 | `write_file("/test.txt", b"world")` | Returns `Ok(())` |
| 6 | `read_file("/test.txt")` | Returns `Ok(Some(b"world"))` |
| 7 | Query `fs_journal` | Contains `update` event for same inode |
| 8 | `remove("/test.txt")` | Returns `Ok(())` |
| 9 | `read_file("/test.txt")` | Returns `Ok(None)` |
| 10 | Query `fs_journal` | Contains `delete` event |
| 11 | Query `fs_current` | File no longer visible |

**Verification:**
```rust
#[tokio::test]
async fn test_basic_crud() {
    let (fs, _temp) = create_test_fs().await;

    // Create
    fs.write_file("/test.txt", b"hello").await.unwrap();

    // Read
    let content = fs.read_file("/test.txt").await.unwrap();
    assert_eq!(content, Some(b"hello".to_vec()));

    // Verify stats
    let stats = fs.stat("/test.txt").await.unwrap().unwrap();
    assert!(stats.is_file());
    assert_eq!(stats.size, 5);

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

---

#### TC-P0-002: Journal Append Atomicity

**Objective:** Verify no partial journal entries exist after failed operations.

**Sub-Story:** STORY-1.2.2

**Preconditions:**
- DuckAgentFS instance with injectable error conditions

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create file successfully | Journal entry exists |
| 2 | Inject DB error before journal commit | Operation fails |
| 3 | Verify no partial entry | Journal integrity check passes |
| 4 | Retry operation | Succeeds without conflicts |

**Verification:**
```rust
#[tokio::test]
async fn test_journal_atomicity_on_failure() {
    let (fs, _temp) = create_test_fs().await;

    // Setup: create initial file
    fs.write_file("/file.txt", b"initial").await.unwrap();

    // Verify journal integrity after successful operation
    // Note: Full atomicity testing requires error injection hooks
    let integrity = verify_journal_integrity(&conn).await.unwrap();
    assert!(integrity, "Journal should have no orphaned entries");
}

#[tokio::test]
async fn test_journal_event_chain() {
    let (fs, _temp) = create_test_fs().await;

    // Create file
    fs.write_file("/file.txt", b"v1").await.unwrap();

    // Update file
    fs.write_file("/file.txt", b"v2").await.unwrap();

    // Delete file
    fs.remove("/file.txt").await.unwrap();

    // Verify event chain: create -> update -> delete
    let events = get_event_types_for_inode(&conn, inode).await.unwrap();
    assert!(events.contains(&"create".to_string()));
    assert!(events.contains(&"update".to_string()));
    assert!(events.contains(&"delete".to_string()));
}
```

---

#### TC-P0-003: Path Resolution with DentryCache

**Objective:** Verify cache hit/miss behavior during path resolution.

**Sub-Story:** STORY-1.2.3

**Preconditions:**
- Nested directory structure `/a/b/c/file.txt`

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/a/b/c/file.txt` | Directories and file created |
| 2 | First `stat("/a/b/c/file.txt")` | DB queries for each component |
| 3 | Second `stat("/a/b/c/file.txt")` | Cache hits for all components |
| 4 | `rename("/a/b/c", "/a/b/d")` | Cache invalidated for `/a/b/c` |
| 5 | `stat("/a/b/d/file.txt")` | DB query for `d`, cache for `a`, `b` |

**Verification:**
```rust
#[tokio::test]
async fn test_path_resolution_caching() {
    let (fs, _temp) = create_test_fs().await;

    // Create nested structure
    fs.mkdir("/a").await.unwrap();
    fs.mkdir("/a/b").await.unwrap();
    fs.mkdir("/a/b/c").await.unwrap();
    fs.write_file("/a/b/c/file.txt", b"content").await.unwrap();

    // First resolution - populates cache
    let stats1 = fs.stat("/a/b/c/file.txt").await.unwrap();
    assert!(stats1.is_some());

    // Second resolution - should use cache
    let stats2 = fs.stat("/a/b/c/file.txt").await.unwrap();
    assert!(stats2.is_some());

    // Rename invalidates cache
    fs.rename("/a/b/c", "/a/b/d").await.unwrap();

    // Old path should not exist
    let old_stats = fs.stat("/a/b/c/file.txt").await.unwrap();
    assert!(old_stats.is_none());

    // New path should work
    let new_stats = fs.stat("/a/b/d/file.txt").await.unwrap();
    assert!(new_stats.is_some());
}
```

---

#### TC-P0-004: Symlink Resolution Limits

**Objective:** Detect and prevent infinite symlink loops.

**Sub-Story:** STORY-1.2.3

**Preconditions:**
- Circular symlink chain (a→b→c→a)

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/a` → `/b` symlink | Succeeds |
| 2 | Create `/b` → `/c` symlink | Succeeds |
| 3 | Create `/c` → `/a` symlink | Succeeds |
| 4 | `stat("/a")` with follow | Returns `FsError::SymlinkLoop` |
| 5 | Verify depth counter | Stopped at `MAX_SYMLINK_DEPTH` (40) |

**Verification:**
```rust
#[tokio::test]
async fn test_symlink_loop_detection() {
    let (fs, _temp) = create_test_fs().await;

    // Create circular symlink chain
    fs.symlink("/b", "/a").await.unwrap();
    fs.symlink("/c", "/b").await.unwrap();
    fs.symlink("/a", "/c").await.unwrap();

    // Following symlinks should detect loop
    let result = fs.stat("/a").await;
    assert!(matches!(result, Err(Error::Fs(FsError::SymlinkLoop))));

    // lstat should work (no follow)
    let lstat = fs.lstat("/a").await.unwrap();
    assert!(lstat.is_some());
    assert!(lstat.unwrap().is_symlink());
}

#[tokio::test]
async fn test_deep_symlink_chain() {
    let (fs, _temp) = create_test_fs().await;

    // Create a long but non-circular chain
    fs.write_file("/target.txt", b"content").await.unwrap();

    let mut prev = "/target.txt".to_string();
    for i in 0..35 {
        let link_name = format!("/link_{}", i);
        fs.symlink(&prev, &link_name).await.unwrap();
        prev = link_name;
    }

    // Should still resolve (under MAX_SYMLINK_DEPTH)
    let content = fs.read_file(&prev).await.unwrap();
    assert_eq!(content, Some(b"content".to_vec()));
}
```

---

#### TC-P0-005: Root Directory Protection

**Objective:** Ensure root directory cannot be deleted or renamed.

**Sub-Story:** STORY-1.2.2

**Preconditions:**
- Fresh DuckAgentFS instance

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `remove("/")` | Returns `FsError::RootOperation` |
| 2 | `rename("/", "/new")` | Returns `FsError::RootOperation` |
| 3 | `stat("/")` | Returns valid Stats with mode containing S_IFDIR |

**Verification:**
```rust
#[tokio::test]
async fn test_root_directory_protection() {
    let (fs, _temp) = create_test_fs().await;

    // Cannot remove root
    let result = fs.remove("/").await;
    assert!(matches!(result, Err(Error::Fs(FsError::RootOperation))));

    // Cannot rename root
    let result = fs.rename("/", "/new").await;
    assert!(matches!(result, Err(Error::Fs(FsError::RootOperation))));

    // Root should exist
    let stats = fs.stat("/").await.unwrap().unwrap();
    assert!(stats.is_directory());
    assert_eq!(stats.ino, 1); // ROOT_INO
}
```

---

### 3.3 P1 - High (Required for Production)

#### TC-P1-001: Concurrent Write Serialization

**Objective:** Validate single-writer semantics under concurrent load.

**Sub-Story:** STORY-1.2.2

**Preconditions:**
- DuckAgentFS with connection pool

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Spawn 10 concurrent `write_file` to different paths | All succeed |
| 2 | Verify all files exist | 10 files with correct content |
| 3 | Verify no data corruption | Journal consistent |

**Verification:**
```rust
#[tokio::test]
async fn test_concurrent_writes() {
    let (fs, _temp) = create_test_fs().await;
    let fs = Arc::new(fs);

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                let path = format!("/file_{}.txt", i);
                let data = format!("content_{}", i);
                fs.write_file(&path, data.as_bytes()).await
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // Verify all files exist with correct content
    for i in 0..10 {
        let path = format!("/file_{}.txt", i);
        let content = fs.read_file(&path).await.unwrap().unwrap();
        assert_eq!(content, format!("content_{}", i).as_bytes());
    }
}

#[tokio::test]
async fn test_concurrent_writes_same_file() {
    let (fs, _temp) = create_test_fs().await;
    let fs = Arc::new(fs);

    // Write initial content
    fs.write_file("/shared.txt", b"initial").await.unwrap();

    // Concurrent updates to same file
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                let data = format!("update_{}", i);
                fs.write_file("/shared.txt", data.as_bytes()).await
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // File should exist with one of the values (last write wins)
    let content = fs.read_file("/shared.txt").await.unwrap().unwrap();
    assert!(String::from_utf8(content).unwrap().starts_with("update_"));
}
```

---

#### TC-P1-002: Hardlink nlink Tracking

**Objective:** Verify nlink is correctly maintained for hardlinks.

**Sub-Story:** STORY-1.2.3

**Preconditions:**
- File with initial nlink=1

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/file.txt` | nlink=1 |
| 2 | `link("/file.txt", "/link.txt")` | Both paths have nlink=2 |
| 3 | `remove("/file.txt")` | `/link.txt` remains, nlink=1 |
| 4 | `read_file("/link.txt")` | Returns original content |

**Verification:**
```rust
#[tokio::test]
async fn test_hardlink_nlink() {
    let (fs, _temp) = create_test_fs().await;

    // Create original file
    fs.write_file("/file.txt", b"content").await.unwrap();
    let stats1 = fs.stat("/file.txt").await.unwrap().unwrap();
    assert_eq!(stats1.nlink, 1);

    // Create hardlink
    fs.link("/file.txt", "/link.txt").await.unwrap();

    // Both should have nlink=2
    let stats1 = fs.stat("/file.txt").await.unwrap().unwrap();
    let stats2 = fs.stat("/link.txt").await.unwrap().unwrap();
    assert_eq!(stats1.nlink, 2);
    assert_eq!(stats2.nlink, 2);
    assert_eq!(stats1.ino, stats2.ino);

    // Remove original
    fs.remove("/file.txt").await.unwrap();

    // Hardlink should still work
    let stats = fs.stat("/link.txt").await.unwrap().unwrap();
    assert_eq!(stats.nlink, 1);
    let content = fs.read_file("/link.txt").await.unwrap();
    assert_eq!(content, Some(b"content".to_vec()));
}

#[tokio::test]
async fn test_multiple_hardlinks() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/original.txt", b"data").await.unwrap();

    // Create multiple hardlinks
    fs.link("/original.txt", "/link1.txt").await.unwrap();
    fs.link("/original.txt", "/link2.txt").await.unwrap();
    fs.link("/original.txt", "/link3.txt").await.unwrap();

    // All should have nlink=4
    let stats = fs.stat("/original.txt").await.unwrap().unwrap();
    assert_eq!(stats.nlink, 4);

    // Remove all but one
    fs.remove("/original.txt").await.unwrap();
    fs.remove("/link1.txt").await.unwrap();
    fs.remove("/link2.txt").await.unwrap();

    // Last link should have nlink=1 and data intact
    let stats = fs.stat("/link3.txt").await.unwrap().unwrap();
    assert_eq!(stats.nlink, 1);
    let content = fs.read_file("/link3.txt").await.unwrap();
    assert_eq!(content, Some(b"data".to_vec()));
}
```

---

#### TC-P1-003: Rename Across Directories

**Objective:** Validate rename with parent change and journal event.

**Sub-Story:** STORY-1.2.3

**Preconditions:**
- Source and destination directories exist

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/src/file.txt` | File exists |
| 2 | Create `/dst/` | Directory exists |
| 3 | `rename("/src/file.txt", "/dst/file.txt")` | Succeeds |
| 4 | Query journal | Contains rename event with `old_parent`, `old_name` |
| 5 | `stat("/src/file.txt")` | Returns None |
| 6 | `stat("/dst/file.txt")` | Returns Stats |

**Verification:**
```rust
#[tokio::test]
async fn test_rename_across_directories() {
    let (fs, _temp) = create_test_fs().await;

    // Setup
    fs.mkdir("/src").await.unwrap();
    fs.mkdir("/dst").await.unwrap();
    fs.write_file("/src/file.txt", b"content").await.unwrap();

    // Capture inode before rename
    let old_stats = fs.stat("/src/file.txt").await.unwrap().unwrap();
    let inode = old_stats.ino;

    // Rename
    fs.rename("/src/file.txt", "/dst/file.txt").await.unwrap();

    // Verify old path gone
    let old = fs.stat("/src/file.txt").await.unwrap();
    assert!(old.is_none());

    // Verify new path exists with same inode
    let new = fs.stat("/dst/file.txt").await.unwrap().unwrap();
    assert_eq!(new.ino, inode);

    // Content preserved
    let content = fs.read_file("/dst/file.txt").await.unwrap();
    assert_eq!(content, Some(b"content".to_vec()));
}

#[tokio::test]
async fn test_rename_directory() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/old_dir").await.unwrap();
    fs.write_file("/old_dir/file.txt", b"data").await.unwrap();

    fs.rename("/old_dir", "/new_dir").await.unwrap();

    // Old path gone
    let old = fs.stat("/old_dir").await.unwrap();
    assert!(old.is_none());

    // New path works, contents intact
    let new = fs.stat("/new_dir").await.unwrap();
    assert!(new.is_some());
    assert!(new.unwrap().is_directory());

    let content = fs.read_file("/new_dir/file.txt").await.unwrap();
    assert_eq!(content, Some(b"data".to_vec()));
}
```

---

#### TC-P1-004: DentryCache Invalidation on Rename

**Objective:** Ensure cache is properly invalidated during renames.

**Sub-Story:** STORY-1.2.3

**Preconditions:**
- Cached path resolution for `/a/b`

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/a/b/file.txt` | Path cached |
| 2 | Access `/a/b/file.txt` multiple times | Cache hits |
| 3 | `rename("/a/b", "/a/c")` | Cache invalidated |
| 4 | Access `/a/b/file.txt` | Returns None (not stale cached result) |
| 5 | Access `/a/c/file.txt` | Returns Stats |

**Verification:**
```rust
#[tokio::test]
async fn test_dentry_cache_invalidation() {
    let (fs, _temp) = create_test_fs().await;

    // Setup and populate cache
    fs.mkdir("/a").await.unwrap();
    fs.mkdir("/a/b").await.unwrap();
    fs.write_file("/a/b/file.txt", b"data").await.unwrap();

    // Populate cache
    for _ in 0..3 {
        fs.stat("/a/b/file.txt").await.unwrap();
    }

    // Rename parent directory
    fs.rename("/a/b", "/a/c").await.unwrap();

    // Old path must not resolve (no stale cache)
    let old = fs.stat("/a/b/file.txt").await.unwrap();
    assert!(old.is_none());

    // New path works
    let new = fs.stat("/a/c/file.txt").await.unwrap();
    assert!(new.is_some());
}

#[tokio::test]
async fn test_cache_invalidation_on_remove() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/dir").await.unwrap();
    fs.write_file("/dir/file.txt", b"data").await.unwrap();

    // Populate cache
    fs.stat("/dir/file.txt").await.unwrap();

    // Remove file
    fs.remove("/dir/file.txt").await.unwrap();

    // Cache should be invalidated
    let stats = fs.stat("/dir/file.txt").await.unwrap();
    assert!(stats.is_none());
}
```

---

#### TC-P1-005: Directory Non-Empty Check

**Objective:** Prevent removal of non-empty directories.

**Sub-Story:** STORY-1.2.2

**Preconditions:**
- Directory with contents

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/dir/file.txt` | File in directory |
| 2 | `remove("/dir")` | Returns `FsError::NotEmpty` |
| 3 | `remove("/dir/file.txt")` | Succeeds |
| 4 | `remove("/dir")` | Succeeds |

**Verification:**
```rust
#[tokio::test]
async fn test_remove_non_empty_directory() {
    let (fs, _temp) = create_test_fs().await;

    // Create directory with file
    fs.mkdir("/dir").await.unwrap();
    fs.write_file("/dir/file.txt", b"data").await.unwrap();

    // Cannot remove non-empty
    let result = fs.remove("/dir").await;
    assert!(matches!(result, Err(Error::Fs(FsError::NotEmpty))));

    // Remove file first
    fs.remove("/dir/file.txt").await.unwrap();

    // Now can remove directory
    fs.remove("/dir").await.unwrap();

    let stats = fs.stat("/dir").await.unwrap();
    assert!(stats.is_none());
}

#[tokio::test]
async fn test_remove_directory_with_subdirectory() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/parent").await.unwrap();
    fs.mkdir("/parent/child").await.unwrap();

    // Cannot remove parent with child
    let result = fs.remove("/parent").await;
    assert!(matches!(result, Err(Error::Fs(FsError::NotEmpty))));

    // Remove child first
    fs.remove("/parent/child").await.unwrap();
    fs.remove("/parent").await.unwrap();

    let stats = fs.stat("/parent").await.unwrap();
    assert!(stats.is_none());
}
```

---

#### TC-P1-006: Symlink Target Storage and Retrieval

**Objective:** Verify symlink targets are stored and retrieved correctly.

**Sub-Story:** STORY-1.2.3

**Preconditions:**
- Target file exists

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/target.txt` | File exists |
| 2 | `symlink("/target.txt", "/link.txt")` | Symlink created |
| 3 | `lstat("/link.txt")` | Returns symlink stats |
| 4 | `readlink("/link.txt")` | Returns `/target.txt` |
| 5 | `read_file("/link.txt")` | Returns target content |

**Verification:**
```rust
#[tokio::test]
async fn test_symlink_operations() {
    let (fs, _temp) = create_test_fs().await;

    // Create target
    fs.write_file("/target.txt", b"target content").await.unwrap();

    // Create symlink
    fs.symlink("/target.txt", "/link.txt").await.unwrap();

    // lstat returns symlink info
    let lstat = fs.lstat("/link.txt").await.unwrap().unwrap();
    assert!(lstat.is_symlink());
    assert_eq!(lstat.size, 11); // "/target.txt".len()

    // stat follows symlink
    let stat = fs.stat("/link.txt").await.unwrap().unwrap();
    assert!(stat.is_file());

    // readlink returns target
    let target = fs.readlink("/link.txt").await.unwrap().unwrap();
    assert_eq!(target, "/target.txt");

    // read follows symlink
    let content = fs.read_file("/link.txt").await.unwrap();
    assert_eq!(content, Some(b"target content".to_vec()));
}

#[tokio::test]
async fn test_relative_symlink() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/a").await.unwrap();
    fs.write_file("/a/target.txt", b"content").await.unwrap();

    // Create relative symlink
    fs.symlink("target.txt", "/a/link.txt").await.unwrap();

    // readlink returns relative path
    let target = fs.readlink("/a/link.txt").await.unwrap().unwrap();
    assert_eq!(target, "target.txt");

    // Should resolve relative to link's directory
    let content = fs.read_file("/a/link.txt").await.unwrap();
    assert_eq!(content, Some(b"content".to_vec()));
}

#[tokio::test]
async fn test_symlink_to_directory() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/real_dir").await.unwrap();
    fs.write_file("/real_dir/file.txt", b"data").await.unwrap();

    fs.symlink("/real_dir", "/link_dir").await.unwrap();

    // Can read through symlinked directory
    let content = fs.read_file("/link_dir/file.txt").await.unwrap();
    assert_eq!(content, Some(b"data".to_vec()));

    // Can list through symlinked directory
    let entries = fs.readdir("/link_dir").await.unwrap().unwrap();
    assert!(entries.contains(&"file.txt".to_string()));
}
```

---

### 3.4 P2 - Medium (Quality Enhancement)

#### TC-P2-001: Time-Travel Snapshot Isolation

**Objective:** Verify snapshot returns historical state, not current.

**Preconditions:**
- DuckAgentFS with time-travel support

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/file.txt` with "v1" | event_id = E1 |
| 2 | Update `/file.txt` with "v2" | event_id = E2 |
| 3 | `snapshot_at(E1).read_file("/file.txt")` | Returns "v1" |
| 4 | `snapshot_at(E2).read_file("/file.txt")` | Returns "v2" |
| 5 | Current `read_file("/file.txt")` | Returns "v2" |

**Verification:**
```rust
#[tokio::test]
async fn test_time_travel_snapshot() {
    let (fs, _temp) = create_test_fs().await;

    // Version 1
    fs.write_file("/file.txt", b"version1").await.unwrap();
    let event_id_v1 = fs.current_event_id().await.unwrap();

    // Version 2
    fs.write_file("/file.txt", b"version2").await.unwrap();
    let event_id_v2 = fs.current_event_id().await.unwrap();

    // Snapshot at v1
    let snap_v1 = fs.snapshot_at(event_id_v1).await.unwrap();
    let content_v1 = snap_v1.read_file("/file.txt").await.unwrap();
    assert_eq!(content_v1, Some(b"version1".to_vec()));

    // Snapshot at v2
    let snap_v2 = fs.snapshot_at(event_id_v2).await.unwrap();
    let content_v2 = snap_v2.read_file("/file.txt").await.unwrap();
    assert_eq!(content_v2, Some(b"version2".to_vec()));

    // Current
    let content_current = fs.read_file("/file.txt").await.unwrap();
    assert_eq!(content_current, Some(b"version2".to_vec()));
}
```

---

#### TC-P2-002: VSS Embedding Update on Write

**Objective:** Verify embeddings are generated for text files.

**Preconditions:**
- DuckAgentFS with VSS enabled and mock embedding generator

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/doc.txt", "Hello world")` | File created |
| 2 | Query `fs_embeddings` | Entry exists for inode |
| 3 | Verify embedding dimension | Matches mock generator dimension |
| 4 | Verify embedding generator called | Call count = 1 |

**Verification:**
```rust
#[tokio::test]
async fn test_vss_embedding_on_write() {
    let mock_gen = Arc::new(MockEmbeddingGenerator::new(1536));
    let (fs, _temp) = create_test_fs_with_vss(mock_gen.clone()).await;

    // Write text file
    fs.write_file("/doc.txt", b"Hello world").await.unwrap();

    // Verify embedding generator was called
    assert_eq!(mock_gen.get_call_count(), 1);

    // Write another file
    fs.write_file("/doc2.txt", b"Another document").await.unwrap();
    assert_eq!(mock_gen.get_call_count(), 2);
}

#[tokio::test]
async fn test_vss_not_called_when_disabled() {
    let (fs, _temp) = create_test_fs().await; // VSS disabled by default

    fs.write_file("/doc.txt", b"Hello world").await.unwrap();
    // No embedding generator configured, should not fail
}
```

---

#### TC-P2-003: Large File Chunking

**Objective:** Verify files larger than chunk_size are handled correctly.

**Preconditions:**
- DuckAgentFS with chunk_size = 4096

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create 10KB file (random data) | Succeeds |
| 2 | Query `fs_data` | 3 chunks exist (4096 + 4096 + remaining) |
| 3 | `read_file` | Returns exact original content |
| 4 | Verify checksum | Matches original data |

**Verification:**
```rust
#[tokio::test]
async fn test_large_file_chunking() {
    let (fs, _temp) = create_test_fs().await;

    // Create 10KB of data
    let data: Vec<u8> = (0..10240).map(|i| (i % 256) as u8).collect();

    fs.write_file("/large.bin", &data).await.unwrap();

    // Read back
    let content = fs.read_file("/large.bin").await.unwrap().unwrap();

    // Verify exact match
    assert_eq!(content.len(), data.len());
    assert_eq!(content, data);
}

#[tokio::test]
async fn test_exact_chunk_boundary() {
    let (fs, _temp) = create_test_fs_with_chunk_size(1024).await;

    // Exactly 3 chunks (3072 bytes)
    let data: Vec<u8> = (0..3072).map(|i| (i % 256) as u8).collect();

    fs.write_file("/exact.bin", &data).await.unwrap();
    let content = fs.read_file("/exact.bin").await.unwrap().unwrap();
    assert_eq!(content, data);
}

#[tokio::test]
async fn test_file_smaller_than_chunk() {
    let (fs, _temp) = create_test_fs().await;

    let data = b"small file";
    fs.write_file("/small.txt", data).await.unwrap();

    let content = fs.read_file("/small.txt").await.unwrap().unwrap();
    assert_eq!(content, data.to_vec());
}
```

---

#### TC-P2-004: mkdir_recursive Idempotence

**Objective:** Verify recursive mkdir handles existing directories.

**Preconditions:**
- Partial directory structure exists

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create `/a/b` | Exists |
| 2 | Call internal `mkdir_recursive("/a/b/c/d")` | Succeeds |
| 3 | Only `/a/b/c` and `/a/b/c/d` created | No duplicate events |
| 4 | No errors for existing `/a` and `/a/b` | Silent success |

**Verification:**
```rust
#[tokio::test]
async fn test_mkdir_recursive_idempotent() {
    let (fs, _temp) = create_test_fs().await;

    // Create partial structure
    fs.mkdir("/a").await.unwrap();
    fs.mkdir("/a/b").await.unwrap();

    // Write file that triggers mkdir_recursive
    fs.write_file("/a/b/c/d/file.txt", b"data").await.unwrap();

    // Verify all directories exist
    assert!(fs.stat("/a").await.unwrap().unwrap().is_directory());
    assert!(fs.stat("/a/b").await.unwrap().unwrap().is_directory());
    assert!(fs.stat("/a/b/c").await.unwrap().unwrap().is_directory());
    assert!(fs.stat("/a/b/c/d").await.unwrap().unwrap().is_directory());

    // Verify file exists
    let content = fs.read_file("/a/b/c/d/file.txt").await.unwrap();
    assert_eq!(content, Some(b"data".to_vec()));
}
```

---

#### TC-P2-005: Snapshot Read-Only Enforcement

**Objective:** Verify snapshot rejects all write operations.

**Preconditions:**
- Snapshot of filesystem

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Get snapshot | Success |
| 2 | `snapshot.write_file(...)` | Error: "Snapshots are read-only" |
| 3 | `snapshot.mkdir(...)` | Error: "Snapshots are read-only" |
| 4 | `snapshot.remove(...)` | Error: "Snapshots are read-only" |
| 5 | `snapshot.rename(...)` | Error: "Snapshots are read-only" |
| 6 | `snapshot.chmod(...)` | Error: "Snapshots are read-only" |
| 7 | `snapshot.symlink(...)` | Error: "Snapshots are read-only" |
| 8 | `snapshot.link(...)` | Error: "Snapshots are read-only" |
| 9 | `snapshot.create_file(...)` | Error: "Snapshots are read-only" |

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_read_only() {
    let (fs, _temp) = create_test_fs().await;

    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    // All write operations should fail
    let write_err = snapshot.write_file("/new.txt", b"data").await;
    assert!(matches!(write_err, Err(Error::Custom(msg)) if msg.contains("read-only")));

    let mkdir_err = snapshot.mkdir("/newdir").await;
    assert!(matches!(mkdir_err, Err(Error::Custom(msg)) if msg.contains("read-only")));

    let remove_err = snapshot.remove("/anything").await;
    assert!(matches!(remove_err, Err(Error::Custom(msg)) if msg.contains("read-only")));

    let rename_err = snapshot.rename("/a", "/b").await;
    assert!(matches!(rename_err, Err(Error::Custom(msg)) if msg.contains("read-only")));

    let chmod_err = snapshot.chmod("/file", 0o755).await;
    assert!(matches!(chmod_err, Err(Error::Custom(msg)) if msg.contains("read-only")));

    let symlink_err = snapshot.symlink("/target", "/link").await;
    assert!(matches!(symlink_err, Err(Error::Custom(msg)) if msg.contains("read-only")));

    let link_err = snapshot.link("/old", "/new").await;
    assert!(matches!(link_err, Err(Error::Custom(msg)) if msg.contains("read-only")));

    let create_err = snapshot.create_file("/new.txt", 0o644).await;
    assert!(matches!(create_err, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-P2-006: File Handle Operations (pread/pwrite)

**Objective:** Verify file handle I/O operations work correctly.

**Preconditions:**
- Open file handle

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create file with known content | Success |
| 2 | `open("/file.txt")` | Returns BoxedFile |
| 3 | `file.pread(0, 5)` | Returns first 5 bytes |
| 4 | `file.pread(5, 5)` | Returns next 5 bytes |
| 5 | `file.pwrite(10, b"append")` | Writes at offset |
| 6 | `file.pread(10, 6)` | Returns "append" |
| 7 | `file.fstat()` | Returns updated size |

**Verification:**
```rust
#[tokio::test]
async fn test_file_handle_operations() {
    let (fs, _temp) = create_test_fs().await;

    // Create file
    fs.write_file("/test.txt", b"0123456789").await.unwrap();

    // Open file
    let file = fs.open("/test.txt").await.unwrap();

    // pread at different offsets
    let first = file.pread(0, 5).await.unwrap();
    assert_eq!(first, b"01234");

    let second = file.pread(5, 5).await.unwrap();
    assert_eq!(second, b"56789");

    // pwrite
    file.pwrite(10, b"ABCDE").await.unwrap();

    // Verify written data
    let written = file.pread(10, 5).await.unwrap();
    assert_eq!(written, b"ABCDE");

    // fstat shows correct size
    let stats = file.fstat().await.unwrap();
    assert_eq!(stats.size, 15);
}

#[tokio::test]
async fn test_file_handle_truncate() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"0123456789").await.unwrap();
    let file = fs.open("/test.txt").await.unwrap();

    // Truncate to 5 bytes
    file.truncate(5).await.unwrap();

    let stats = file.fstat().await.unwrap();
    assert_eq!(stats.size, 5);

    let content = file.pread(0, 10).await.unwrap();
    assert_eq!(content, b"01234");
}
```

---

#### TC-P2-007: chmod Permission Preservation

**Objective:** Verify chmod only modifies permission bits, not file type.

**Preconditions:**
- Regular file and directory exist

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create file with mode 0644 | Success |
| 2 | `chmod("/file.txt", 0755)` | Succeeds |
| 3 | `stat("/file.txt")` | mode = S_IFREG \| 0755 |
| 4 | Verify still a regular file | is_file() = true |

**Verification:**
```rust
#[tokio::test]
async fn test_chmod_preserves_file_type() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/file.txt", b"data").await.unwrap();

    // Change permissions
    fs.chmod("/file.txt", 0o755).await.unwrap();

    let stats = fs.stat("/file.txt").await.unwrap().unwrap();

    // File type preserved
    assert!(stats.is_file());
    assert!(!stats.is_directory());

    // Permissions changed
    assert_eq!(stats.mode & 0o7777, 0o755);
}

#[tokio::test]
async fn test_chmod_directory() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/dir").await.unwrap();
    fs.chmod("/dir", 0o700).await.unwrap();

    let stats = fs.stat("/dir").await.unwrap().unwrap();
    assert!(stats.is_directory());
    assert_eq!(stats.mode & 0o7777, 0o700);
}
```

---

### 3.5 Edge Cases and Error Conditions

#### TC-EDGE-001: Empty Path Handling

```rust
#[test]
fn test_empty_path_normalization() {
    assert_eq!(DuckAgentFS::normalize_path(""), "/");
}
```

---

#### TC-EDGE-002: Path with Trailing Slashes

```rust
#[tokio::test]
async fn test_trailing_slash_normalization() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/dir").await.unwrap();

    // Both should work
    let stats1 = fs.stat("/dir").await.unwrap();
    let stats2 = fs.stat("/dir/").await.unwrap();

    assert!(stats1.is_some());
    assert!(stats2.is_some());
    assert_eq!(stats1.unwrap().ino, stats2.unwrap().ino);
}
```

---

#### TC-EDGE-003: Relative Symlink Resolution

```rust
#[tokio::test]
async fn test_relative_symlink() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/a").await.unwrap();
    fs.write_file("/a/target.txt", b"content").await.unwrap();

    // Create relative symlink
    fs.symlink("target.txt", "/a/link.txt").await.unwrap();

    // Should resolve relative to link's directory
    let content = fs.read_file("/a/link.txt").await.unwrap();
    assert_eq!(content, Some(b"content".to_vec()));
}
```

---

#### TC-EDGE-004: Cannot Hardlink Directories

```rust
#[tokio::test]
async fn test_cannot_hardlink_directory() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/dir").await.unwrap();

    let result = fs.link("/dir", "/dir_link").await;
    assert!(matches!(result, Err(Error::Fs(FsError::IsADirectory))));
}
```

---

#### TC-EDGE-005: Readlink on Non-Symlink

```rust
#[tokio::test]
async fn test_readlink_on_regular_file() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/file.txt", b"data").await.unwrap();

    let result = fs.readlink("/file.txt").await;
    assert!(matches!(result, Err(Error::Fs(FsError::NotASymlink))));
}
```

---

#### TC-EDGE-006: Create File with AlreadyExists

```rust
#[tokio::test]
async fn test_create_file_already_exists() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/file.txt", b"existing").await.unwrap();

    let result = fs.create_file("/file.txt", 0o644).await;
    assert!(matches!(result, Err(Error::Fs(FsError::AlreadyExists))));
}
```

---

#### TC-EDGE-007: readdir on Non-Directory

```rust
#[tokio::test]
async fn test_readdir_on_file() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/file.txt", b"data").await.unwrap();

    let result = fs.readdir("/file.txt").await;
    assert!(matches!(result, Err(Error::Fs(FsError::NotADirectory))));
}
```

---

#### TC-EDGE-008: read_file on Directory

```rust
#[tokio::test]
async fn test_read_file_on_directory() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/dir").await.unwrap();

    let result = fs.read_file("/dir").await;
    assert!(matches!(result, Err(Error::Fs(FsError::IsADirectory))));
}
```

---

#### TC-EDGE-009: Rename to Subdirectory of Self

```rust
#[tokio::test]
async fn test_rename_to_subdirectory_of_self() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/a").await.unwrap();
    fs.mkdir("/a/b").await.unwrap();

    // Cannot rename directory into its own subdirectory
    let result = fs.rename("/a", "/a/b/a").await;
    assert!(matches!(result, Err(Error::Fs(FsError::InvalidRename))));
}
```

---

#### TC-EDGE-010: statfs Returns Correct Counts

```rust
#[tokio::test]
async fn test_statfs() {
    let (fs, _temp) = create_test_fs().await;

    // Create some files
    fs.write_file("/file1.txt", b"hello").await.unwrap();
    fs.write_file("/file2.txt", b"world").await.unwrap();
    fs.mkdir("/dir").await.unwrap();

    let stats = fs.statfs().await.unwrap();

    // At least 4 inodes: root + file1 + file2 + dir
    assert!(stats.inodes >= 4);
    // At least 10 bytes
    assert!(stats.bytes_used >= 10);
}
```

---

#### TC-EDGE-011: mkdir Parent Not Found

```rust
#[tokio::test]
async fn test_mkdir_parent_not_found() {
    let (fs, _temp) = create_test_fs().await;

    let result = fs.mkdir("/nonexistent/child").await;
    assert!(matches!(result, Err(Error::Fs(FsError::NotFound))));
}
```

---

#### TC-EDGE-012: mkdir Already Exists

```rust
#[tokio::test]
async fn test_mkdir_already_exists() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/dir").await.unwrap();

    let result = fs.mkdir("/dir").await;
    assert!(matches!(result, Err(Error::Fs(FsError::AlreadyExists))));
}
```

---

#### TC-EDGE-013: Rename Overwrites Target

```rust
#[tokio::test]
async fn test_rename_overwrites_target() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/source.txt", b"source content").await.unwrap();
    fs.write_file("/target.txt", b"target content").await.unwrap();

    fs.rename("/source.txt", "/target.txt").await.unwrap();

    // Source gone
    let source = fs.stat("/source.txt").await.unwrap();
    assert!(source.is_none());

    // Target has source content
    let content = fs.read_file("/target.txt").await.unwrap();
    assert_eq!(content, Some(b"source content".to_vec()));
}
```

---

#### TC-EDGE-014: Rename Type Mismatch

```rust
#[tokio::test]
async fn test_rename_type_mismatch() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/file.txt", b"data").await.unwrap();
    fs.mkdir("/dir").await.unwrap();

    // Cannot rename file over directory
    let result = fs.rename("/file.txt", "/dir").await;
    assert!(matches!(result, Err(Error::Fs(FsError::InvalidRename))));

    // Cannot rename directory over file
    let result = fs.rename("/dir", "/file.txt").await;
    assert!(matches!(result, Err(Error::Fs(FsError::InvalidRename))));
}
```

---

## 4. DentryCache Unit Tests (STORY-1.2.4)

The DentryCache is a critical internal component that needs direct unit testing.

```rust
#[cfg(test)]
mod dentry_cache_tests {
    use super::*;

    #[test]
    fn test_cache_get_miss() {
        let cache = DentryCache::new(100);
        assert_eq!(cache.get(1, "nonexistent"), None);
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = DentryCache::new(100);
        cache.insert(1, "child", 42);
        assert_eq!(cache.get(1, "child"), Some(42));
    }

    #[test]
    fn test_cache_remove() {
        let cache = DentryCache::new(100);
        cache.insert(1, "child", 42);
        cache.remove(1, "child");
        assert_eq!(cache.get(1, "child"), None);
    }

    #[test]
    fn test_cache_clear() {
        let cache = DentryCache::new(100);
        cache.insert(1, "a", 2);
        cache.insert(1, "b", 3);
        cache.clear();
        assert_eq!(cache.get(1, "a"), None);
        assert_eq!(cache.get(1, "b"), None);
    }

    #[test]
    fn test_cache_lru_eviction() {
        let cache = DentryCache::new(2); // Max 2 entries

        cache.insert(1, "a", 10);
        cache.insert(1, "b", 20);

        // Access 'a' to make it recently used
        cache.get(1, "a");

        // Insert 'c' - should evict 'b' (least recently used)
        cache.insert(1, "c", 30);

        assert_eq!(cache.get(1, "a"), Some(10)); // Still there
        assert_eq!(cache.get(1, "b"), None);     // Evicted
        assert_eq!(cache.get(1, "c"), Some(30)); // Newly added
    }

    #[test]
    fn test_cache_different_parents() {
        let cache = DentryCache::new(100);

        // Same name, different parents
        cache.insert(1, "file.txt", 10);
        cache.insert(2, "file.txt", 20);

        assert_eq!(cache.get(1, "file.txt"), Some(10));
        assert_eq!(cache.get(2, "file.txt"), Some(20));
    }

    #[test]
    fn test_cache_update_existing() {
        let cache = DentryCache::new(100);

        cache.insert(1, "file.txt", 10);
        cache.insert(1, "file.txt", 20); // Update

        assert_eq!(cache.get(1, "file.txt"), Some(20));
    }

    #[test]
    fn test_cache_empty_name() {
        let cache = DentryCache::new(100);

        cache.insert(1, "", 10);
        assert_eq!(cache.get(1, ""), Some(10));
    }
}
```

---

## 5. Performance Tests

#### TC-PERF-001: Path Resolution Cache Efficiency

**Objective:** Measure cache hit rate under realistic workload.

```rust
#[tokio::test]
async fn test_path_resolution_cache_performance() {
    let (fs, _temp) = create_test_fs().await;

    // Create deep structure
    let mut path = String::new();
    for i in 0..10 {
        path.push_str(&format!("/level{}", i));
        fs.mkdir(&path).await.unwrap();
    }
    fs.write_file(&format!("{}/file.txt", path), b"x").await.unwrap();

    // Warm up cache
    fs.stat(&format!("{}/file.txt", path)).await.unwrap();

    // Measure repeated access
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        fs.stat(&format!("{}/file.txt", path)).await.unwrap();
    }
    let elapsed = start.elapsed();

    // Should be very fast with caching (< 1ms per access average)
    let avg_us = elapsed.as_micros() / 1000;
    assert!(avg_us < 1000, "Avg access {}us should be < 1000us", avg_us);
}
```

#### TC-PERF-002: Bulk Write Performance

```rust
#[tokio::test]
async fn test_bulk_write_performance() {
    let (fs, _temp) = create_test_fs().await;

    let start = std::time::Instant::now();
    for i in 0..100 {
        let path = format!("/file_{}.txt", i);
        fs.write_file(&path, b"test content").await.unwrap();
    }
    let elapsed = start.elapsed();

    // 100 files should complete in reasonable time
    assert!(elapsed.as_secs() < 10, "Bulk write took too long: {:?}", elapsed);
}
```

---

## 6. Test Organization

### Recommended File Structure

```
sdk/rust/
├── src/
│   └── filesystem/
│       └── duckagentfs.rs     # Implementation + unit tests
├── tests/
│   ├── common/
│   │   ├── mod.rs             # Test utilities
│   │   ├── fixtures.rs        # Test fixtures
│   │   ├── mock_embeddings.rs # Mock embedding generator
│   │   ├── journal_helpers.rs # Journal verification
│   │   └── snapshot_helpers.rs # Snapshot utilities
│   ├── duckagentfs_infra.rs   # Infrastructure tests (1.2.1)
│   ├── duckagentfs_crud.rs    # P0 CRUD tests (1.2.2)
│   ├── duckagentfs_paths.rs   # Path resolution tests (1.2.3)
│   ├── duckagentfs_links.rs   # Symlink/hardlink tests (1.2.3)
│   ├── duckagentfs_concurrent.rs # Concurrency tests (1.2.2)
│   ├── duckagentfs_snapshot.rs # Time-travel tests
│   ├── duckagentfs_vss.rs     # VSS integration tests
│   └── duckagentfs_edge.rs    # Edge cases
```

### Test Execution Commands

```bash
# Run all DuckAgentFS tests
cargo test duckagentfs

# Run only P0 critical tests
cargo test duckagentfs_crud

# Run DentryCache unit tests
cargo test dentry_cache

# Run with verbose output
cargo test duckagentfs -- --nocapture

# Run specific test
cargo test test_basic_crud -- --exact

# Run tests matching pattern
cargo test test_symlink
```

---

## 7. Blockers and Prerequisites

### Implementation Blockers

| Blocker | Impact | Resolution | Sub-Story |
|---------|--------|------------|-----------|
| All methods return placeholder values | Cannot run any tests | Complete DuckDB SQL implementation | 1.2.2 |
| `DuckConnectionPool` is placeholder | No actual DB connections | Integrate `duckdb` crate | 1.2.1 |
| `lookup_child` always returns None | Path resolution fails | Implement actual SQL query | 1.2.2 |
| `stat_inode` always returns None | All stats fail | Implement fs_current query | 1.2.2 |
| `write_data_chunks` is no-op | No file data stored | Implement chunk storage | 1.2.2 |
| `read_symlink_target` returns None | Symlinks don't work | Implement fs_data query | 1.2.3 |

### Dependencies

- `duckdb` crate (Rust bindings for DuckDB)
- `tempfile` crate for test isolation
- `tokio` with `test` feature for async tests

### Pre-Test Checklist

- [ ] DuckDB crate integrated and compiling (STORY-1.2.1)
- [ ] Schema initialization working (`init_schema`)
- [ ] Connection pool functional
- [ ] At least `write_file` and `read_file` returning real data (STORY-1.2.2)
- [ ] Journal events being written
- [ ] DentryCache unit tests passing (STORY-1.2.4)
- [ ] Symlink operations working (STORY-1.2.3)

---

## 8. Test Execution Gating

### Gate 1: Pre-Alpha (Internal) - STORY-1.2.1 + 1.2.4 Complete

All infrastructure and cache tests passing:
- TC-INFRA-001: DuckDB Connection and Schema
- All DentryCache unit tests (Section 4)

### Gate 2: Alpha - STORY-1.2.2 Complete

All P0 tests passing:
- TC-P0-001: Basic CRUD E2E
- TC-P0-002: Journal Append Atomicity
- TC-P0-005: Root Directory Protection
- TC-P1-005: Directory Non-Empty Check

### Gate 3: Beta - STORY-1.2.3 Complete

All P0 + P1 tests passing:
- All P0 tests
- TC-P0-003: Path Resolution with DentryCache
- TC-P0-004: Symlink Resolution Limits
- TC-P1-001: Concurrent Write Serialization
- TC-P1-002: Hardlink nlink Tracking
- TC-P1-003: Rename Across Directories
- TC-P1-004: DentryCache Invalidation on Rename
- TC-P1-006: Symlink Target Storage and Retrieval

### Gate 4: Release Candidate - All Tests Complete

All P0 + P1 + P2 tests passing:
- All P0 and P1 tests
- TC-P2-001: Time-Travel Snapshot Isolation
- TC-P2-002: VSS Embedding Update on Write
- TC-P2-003: Large File Chunking
- TC-P2-004: mkdir_recursive Idempotence
- TC-P2-005: Snapshot Read-Only Enforcement
- TC-P2-006: File Handle Operations
- TC-P2-007: chmod Permission Preservation
- All edge case tests
- Performance tests meet SLOs

---

## 9. Appendix: Error Type Reference

| Error | Condition | Errno |
|-------|-----------|-------|
| `FsError::NotFound` | Path does not exist | ENOENT |
| `FsError::AlreadyExists` | Path already exists | EEXIST |
| `FsError::NotEmpty` | Directory not empty | ENOTEMPTY |
| `FsError::NotADirectory` | Expected directory | ENOTDIR |
| `FsError::IsADirectory` | Expected file | EISDIR |
| `FsError::NotASymlink` | Expected symlink | EINVAL |
| `FsError::InvalidPath` | Malformed path | EINVAL |
| `FsError::RootOperation` | Cannot modify root | EPERM |
| `FsError::SymlinkLoop` | Symlink depth exceeded | ELOOP |
| `FsError::InvalidRename` | Invalid rename target | EINVAL |

---

## 10. Sub-Story Test Matrix

| Test ID | Sub-Story | Priority | Status |
|---------|-----------|----------|--------|
| TC-INFRA-001 | 1.2.1 | P0 | Ready |
| TC-P0-001 | 1.2.2 | P0 | Ready |
| TC-P0-002 | 1.2.2 | P0 | Ready |
| TC-P0-003 | 1.2.3 | P0 | Blocked by 1.2.2 |
| TC-P0-004 | 1.2.3 | P0 | Blocked by 1.2.2 |
| TC-P0-005 | 1.2.2 | P0 | Ready |
| TC-P1-001 | 1.2.2 | P1 | Ready |
| TC-P1-002 | 1.2.3 | P1 | Blocked by 1.2.2 |
| TC-P1-003 | 1.2.3 | P1 | Blocked by 1.2.2 |
| TC-P1-004 | 1.2.3, 1.2.4 | P1 | Blocked by 1.2.2 |
| TC-P1-005 | 1.2.2 | P1 | Ready |
| TC-P1-006 | 1.2.3 | P1 | Blocked by 1.2.2 |
| DentryCache Tests | 1.2.4 | P1 | Ready (no blockers) |

---

## 11. Sign-Off

| Role | Name | Date | Status |
|------|------|------|--------|
| QA Engineer | Quinn (QA Agent) | 2026-01-14 | Designed |
| Updated By | QA Agent | 2026-01-14 | Updated with sub-story mapping |
| Developer | - | - | Pending |
| Tech Lead | - | - | Pending |
