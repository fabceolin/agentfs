# STORY-1.2.2: Core CRUD Implementation

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.2.2 |
| **Parent** | STORY-1.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Ready for Development |
| **Priority** | Critical |
| **Estimated Effort** | Medium (3-5 days) |
| **File** | `sdk/rust/src/filesystem/duckagentfs.rs` |
| **Dependencies** | STORY-1.2.1 |

## User Story

**As a** developer
**I want** working CRUD operations in DuckAgentFS
**So that** files and directories can be created, read, updated, and deleted

## Technical Description

Implement actual DuckDB queries for core filesystem operations. Replace all placeholder SQL comments with working code. All mutations must append to `fs_journal`; reads query `fs_current` view.

## Acceptance Criteria

- [ ] `stat` / `lstat` query `fs_current` view and return real Stats
- [ ] `read_file` retrieves data from `fs_data` table, assembles chunks
- [ ] `write_file` appends to `fs_journal` and writes to `fs_data`
- [ ] `mkdir` creates directory entries in journal
- [ ] `remove` appends delete events to journal
- [ ] `readdir` / `readdir_plus` list directory contents from `fs_current`
- [ ] `rename` appends rename events with old_parent/old_name
- [ ] `chmod` appends chmod events preserving file type
- [ ] TC-P0-001 (Basic CRUD E2E) test passing
- [ ] TC-P0-005 (Root Directory Protection) test passing

## Methods to Implement

| Method | Current State | Action Required |
|--------|---------------|-----------------|
| `stat_inode` | Returns `Ok(None)` | Query `fs_current` by inode |
| `lookup_child` | Returns `Ok(None)` | Query `fs_current` by parent+name |
| `read_file` | Returns `Ok(Some(vec![]))` | Query `fs_data`, assemble chunks |
| `write_file` | No-op data write | Implement `write_data_chunks` |
| `append_journal_event` | Returns `Ok(1)` | Execute INSERT RETURNING |
| `allocate_inode` | Returns `Ok(2)` | Use `nextval('fs_inode_seq')` |
| `readdir` | Returns `[., ..]` only | Query children from `fs_current` |

## Technical Specification

### stat_inode Implementation

```rust
async fn stat_inode(&self, ino: i64) -> Result<Option<Stats>> {
    let conn = self.pool.get_connection().await?;

    let result: Option<Stats> = conn.query_row(
        r#"
        SELECT inode, mode, nlink, uid, gid, size,
               EXTRACT(EPOCH FROM mtime)::BIGINT as mtime
        FROM fs_current
        WHERE inode = ?
        "#,
        params![ino],
        |row| Ok(Stats {
            ino: row.get(0)?,
            mode: row.get(1)?,
            nlink: row.get(2)?,
            uid: row.get(3)?,
            gid: row.get(4)?,
            size: row.get(5)?,
            atime: row.get::<_, i64>(6)?,
            mtime: row.get::<_, i64>(6)?,
            ctime: row.get::<_, i64>(6)?,
        })
    ).optional()?;

    Ok(result)
}
```

### write_data_chunks Implementation

```rust
async fn write_data_chunks(
    &self,
    conn: &DuckConnection,
    ino: i64,
    data: &[u8],
) -> Result<()> {
    // Use transaction for atomicity
    conn.execute("BEGIN TRANSACTION", [])?;

    // Delete existing chunks
    conn.execute("DELETE FROM fs_data WHERE inode = ?", params![ino])?;

    // Write new chunks
    for (idx, chunk) in data.chunks(self.config.chunk_size).enumerate() {
        conn.execute(
            "INSERT INTO fs_data (inode, chunk_idx, data) VALUES (?, ?, ?)",
            params![ino, idx as u32, chunk]
        )?;
    }

    conn.execute("COMMIT", [])?;
    Ok(())
}
```

## Tests

### Test 1: Basic CRUD E2E (TC-P0-001)

```rust
#[tokio::test]
async fn test_basic_crud() {
    let (fs, _temp) = create_test_fs().await;

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

### Test 2: Root Directory Protection (TC-P0-005)

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
}
```

## Test Reference

See: `docs/qa/STORY-1.2-filesystem-trait-test-design.md` Section 3.1

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/filesystem/duckagentfs.rs` | Implementation |
| `schema/duckagentfs.sql` | Schema reference |
| `docs/qa/STORY-1.2-filesystem-trait-test-design.md` | Test scenarios |

## Implementation Notes

1. **Transaction Boundaries**: Wrap multi-statement operations in transactions
2. **Error Mapping**: Map DuckDB constraint violations to `FsError::AlreadyExists`
3. **Chunk Assembly**: Truncate final result to exact file size from stats

## Created By

Sprint Change Proposal SCP-2026-01-14-STORY-1.2
