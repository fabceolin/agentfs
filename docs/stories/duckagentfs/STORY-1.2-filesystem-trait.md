# STORY-1.2: DuckAgentFS FileSystem Trait

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | In Progress |
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

- [x] Implement all methods of the `FileSystem` trait
- [x] Use journal model for all mutations
- [x] Maintain DentryCache for performance
- [x] Support symlinks and hardlinks
- [ ] Unit and integration tests

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
