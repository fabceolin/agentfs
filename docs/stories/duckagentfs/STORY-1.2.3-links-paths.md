# STORY-1.2.3: Links and Path Resolution

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.2.3 |
| **Parent** | STORY-1.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Ready for Development |
| **Priority** | High |
| **Estimated Effort** | Medium (2-3 days) |
| **File** | `sdk/rust/src/filesystem/duckagentfs.rs` |
| **Dependencies** | STORY-1.2.2 |

## User Story

**As a** developer
**I want** symlinks and hardlinks working in DuckAgentFS
**So that** the filesystem supports standard Unix link semantics

## Technical Description

Implement symlink/hardlink operations and ensure path resolution correctly follows symlinks. Symlink targets are stored in `fs_data` as file content. Hardlinks share the same inode with incremented nlink.

## Acceptance Criteria

- [ ] `symlink` creates symbolic links (target stored in `fs_data`)
- [ ] `link` creates hardlinks (increments nlink in journal event)
- [ ] `readlink` returns symlink target from `fs_data`
- [ ] `read_symlink_target` implemented with actual DB query
- [ ] `resolve_path` follows symlinks correctly (uses `read_symlink_target`)
- [ ] Symlink loop detection works (MAX_SYMLINK_DEPTH=40)
- [ ] DentryCache invalidation on rename works correctly
- [ ] TC-P0-003 (Path Resolution with Cache) passing
- [ ] TC-P0-004 (Symlink Resolution Limits) passing
- [ ] TC-P1-002 (Hardlink nlink Tracking) passing
- [ ] TC-P1-003 (Rename Across Directories) passing
- [ ] TC-P1-004 (DentryCache Invalidation) passing
- [ ] TC-P1-005 (Directory Non-Empty Check) passing
- [ ] TC-P1-006 (Symlink Target Storage) passing

## Methods to Implement

| Method | Current State | Action Required |
|--------|---------------|-----------------|
| `read_symlink_target` | Returns `Ok(None)` | Query `fs_data` chunk 0 as string |
| `symlink` | Placeholder data write | Ensure target stored in `fs_data` |
| `link` | Conceptual only | Verify nlink increment in journal |

## Technical Specification

### read_symlink_target Implementation

```rust
async fn read_symlink_target(&self, ino: i64) -> Result<Option<String>> {
    let conn = self.pool.get_connection().await?;

    let target: Option<String> = conn.query_row(
        "SELECT CAST(data AS VARCHAR) FROM fs_data WHERE inode = ? AND chunk_idx = 0",
        params![ino],
        |row| row.get(0)
    ).optional()?;

    Ok(target)
}
```

### Hardlink nlink Tracking

When creating a hardlink, the journal event must update nlink:

```rust
// In link() method
self.append_journal_event(
    &conn,
    old_ino,
    "link",  // New event type for hardlinks
    Some(parent_ino),
    Some(name),
    Some(old_stats.mode),
    Some(old_stats.size),
    Some(old_stats.nlink + 1),  // Increment nlink
    None,
    None,
).await?;
```

### DentryCache Invalidation Pattern

```rust
// In rename() - invalidate old entry and any children
self.dentry_cache.remove(from_parent_ino, from_name);

// If renaming a directory, clear all cached children
// (simplified approach - full implementation would walk cache)
if is_directory {
    self.dentry_cache.clear();  // Conservative approach
}

self.dentry_cache.insert(to_parent_ino, to_name, ino);
```

## Tests

### Test 1: Symlink Loop Detection (TC-P0-004)

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
```

### Test 2: Hardlink nlink Tracking (TC-P1-002)

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
```

### Test 3: DentryCache Invalidation (TC-P1-004)

```rust
#[tokio::test]
async fn test_dentry_cache_invalidation() {
    let (fs, _temp) = create_test_fs().await;

    // Setup and populate cache
    fs.mkdir("/a").await.unwrap();
    fs.mkdir("/a/b").await.unwrap();
    fs.write_file("/a/b/file.txt", b"data").await.unwrap();

    // Populate cache with multiple accesses
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
```

## Test Reference

See: `docs/qa/STORY-1.2-filesystem-trait-test-design.md` Sections 3.1-3.2

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/filesystem/duckagentfs.rs` | Implementation |
| `docs/qa/STORY-1.2-filesystem-trait-test-design.md` | Test scenarios |

## Implementation Notes

1. **Symlink Storage**: Symlink targets stored as UTF-8 in `fs_data` chunk 0
2. **Relative Symlinks**: Resolution must handle relative paths from symlink's parent
3. **nlink Decrement**: On `remove()`, if nlink > 1, append update event decrementing nlink instead of delete

## Created By

Sprint Change Proposal SCP-2026-01-14-STORY-1.2
