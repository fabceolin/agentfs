# STORY-1.4: Time-Travel Queries

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.4 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Ready for Development |
| **Revision Notes** | Validated 2026-01-14: 4 of 6 acceptance criteria incomplete. Remaining: implement list_events() API, diff() API, unit tests for read-only enforcement (3+), integration tests for time-travel (5+). CLI scope moved to STORY-6.1 per SCP-2026-01-14. |
| **Priority** | Medium |
| **File** | `sdk/rust/src/filesystem/duckagentfs.rs` |
| **Dependencies** | STORY-1.1, STORY-1.2 |

## User Story

**As a** developer
**I want** to query the filesystem at any point in time
**So that** I have audit trail and data recovery capabilities

## Technical Description

The append-only journal model allows reconstructing the filesystem state at any point in time by filtering events by `event_id`. This is useful for:

- Audit trail (who changed what, when)
- Recovering deleted data
- Debugging issues
- Compliance and governance

## Acceptance Criteria

- [x] Method `snapshot_at(event_id)` returns read-only filesystem
- [x] View `fs_current` filters by event_id
- [ ] API: `list_events(limit, offset)` returns paginated timeline
- [ ] API: `diff(from_event, to_event)` returns Vec<FileDiff>
- [ ] Unit tests for snapshot read-only enforcement (3+ tests)
- [ ] Integration tests for time-travel scenarios (5+ tests)

> **Note**: CLI commands (`agentfs snapshot`) moved to STORY-6.1 per SCP-2026-01-14

## Technical Specification

### Snapshot API

```rust
impl DuckAgentFS {
    /// Create a read-only snapshot at a specific event
    pub async fn snapshot_at(&self, event_id: i64) -> Result<DuckAgentFSSnapshot> {
        // Verify event_id exists
        let conn = self.pool.get_connection().await?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM fs_journal WHERE event_id = ?)",
            params![event_id],
            |row| row.get(0)
        )?;

        if !exists {
            return Err(Error::Custom(format!("Event {} not found", event_id)));
        }

        Ok(DuckAgentFSSnapshot {
            fs: self.clone(),
            event_id,
        })
    }

    /// Get current (latest) event ID
    pub async fn current_event_id(&self) -> Result<i64> {
        let conn = self.pool.get_connection().await?;
        let event_id: i64 = conn.query_row(
            "SELECT COALESCE(MAX(event_id), 0) FROM fs_journal",
            [],
            |row| row.get(0)
        )?;
        Ok(event_id)
    }

    /// List recent events (timeline)
    pub async fn list_events(
        &self,
        limit: usize,
        offset: usize
    ) -> Result<Vec<JournalEvent>> {
        let conn = self.pool.get_connection().await?;
        let events = conn.query_map(
            r#"
            SELECT event_id, inode, event_type, event_time, name,
                   actor_id, session_id
            FROM fs_journal
            ORDER BY event_id DESC
            LIMIT ? OFFSET ?
            "#,
            params![limit, offset],
            |row| Ok(JournalEvent {
                event_id: row.get(0)?,
                inode: row.get(1)?,
                event_type: row.get(2)?,
                event_time: row.get(3)?,
                name: row.get(4)?,
                actor_id: row.get(5)?,
                session_id: row.get(6)?,
            })
        )?;
        Ok(events)
    }
}
```

### Snapshot Struct

```rust
/// Read-only view of filesystem at a specific point in time
pub struct DuckAgentFSSnapshot {
    fs: DuckAgentFS,
    event_id: i64,
}

impl DuckAgentFSSnapshot {
    pub fn event_id(&self) -> i64 {
        self.event_id
    }

    /// Query fs_current filtered by event_id
    async fn query_current(&self, query: &str, params: impl Params) -> Result<Rows> {
        // Modify query to use historical view
        let historical_query = format!(
            r#"
            WITH ranked AS (
                SELECT *,
                       ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
                FROM fs_journal
                WHERE event_id <= {}
                  AND event_type != 'delete'
            ),
            fs_current_at AS (
                SELECT * FROM ranked WHERE rn = 1
                AND inode NOT IN (
                    SELECT inode FROM fs_journal
                    WHERE event_type = 'delete'
                    AND event_id <= {}
                    AND event_id = (
                        SELECT MAX(event_id) FROM fs_journal j2
                        WHERE j2.inode = fs_journal.inode
                        AND j2.event_id <= {}
                    )
                )
            )
            {}
            "#,
            self.event_id,
            self.event_id,
            self.event_id,
            query.replace("fs_current", "fs_current_at")
        );

        // Execute query...
    }
}
```

### FileSystem Implementation for Snapshot

```rust
#[async_trait]
impl FileSystem for DuckAgentFSSnapshot {
    async fn stat(&self, path: &str) -> Result<Option<Stats>> {
        // Use historical fs_current
        self.stat_at(path).await
    }

    async fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        // Read file data at snapshot time
        self.read_file_at(path).await
    }

    // All write operations return error
    async fn write_file(&self, _: &str, _: &[u8]) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn mkdir(&self, _: &str) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    // ... etc
}
```

### Diff API

```rust
pub struct FileDiff {
    pub path: String,
    pub change_type: ChangeType,
    pub old_stats: Option<Stats>,
    pub new_stats: Option<Stats>,
}

pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Renamed { from: String },
}

impl DuckAgentFS {
    /// Compare two snapshots and return differences
    pub async fn diff(
        &self,
        from_event: i64,
        to_event: i64
    ) -> Result<Vec<FileDiff>> {
        let conn = self.pool.get_connection().await?;

        // Find all events between from and to
        let events = conn.query_map(
            r#"
            SELECT event_id, inode, event_type, name, old_name,
                   parent, old_parent
            FROM fs_journal
            WHERE event_id > ? AND event_id <= ?
            ORDER BY event_id
            "#,
            params![from_event, to_event],
            |row| { /* parse */ }
        )?;

        // Build diff list
        let mut diffs = Vec::new();
        for event in events {
            match event.event_type.as_str() {
                "create" => diffs.push(FileDiff {
                    path: resolve_path(event.inode),
                    change_type: ChangeType::Added,
                    old_stats: None,
                    new_stats: Some(/* stats */),
                }),
                "delete" => diffs.push(FileDiff {
                    path: resolve_path(event.inode),
                    change_type: ChangeType::Deleted,
                    old_stats: Some(/* stats */),
                    new_stats: None,
                }),
                "update" => diffs.push(FileDiff {
                    path: resolve_path(event.inode),
                    change_type: ChangeType::Modified,
                    old_stats: Some(/* old */),
                    new_stats: Some(/* new */),
                }),
                "rename" => diffs.push(FileDiff {
                    path: resolve_path(event.inode),
                    change_type: ChangeType::Renamed {
                        from: event.old_name.unwrap()
                    },
                    old_stats: Some(/* stats */),
                    new_stats: Some(/* stats */),
                }),
                _ => {}
            }
        }

        Ok(diffs)
    }
}
```

## Tests

### Test 1: Snapshot Read
```rust
#[tokio::test]
async fn test_snapshot_read() {
    let fs = DuckAgentFS::open(config).await.unwrap();

    // Create file
    fs.write_file("/test.txt", b"version 1").await.unwrap();
    let event1 = fs.current_event_id().await.unwrap();

    // Update file
    fs.write_file("/test.txt", b"version 2").await.unwrap();

    // Snapshot at event1 should see version 1
    let snapshot = fs.snapshot_at(event1).await.unwrap();
    let content = snapshot.read_file("/test.txt").await.unwrap();
    assert_eq!(content, Some(b"version 1".to_vec()));

    // Current should see version 2
    let content = fs.read_file("/test.txt").await.unwrap();
    assert_eq!(content, Some(b"version 2".to_vec()));
}
```

### Test 2: Snapshot of Deleted File
```rust
#[tokio::test]
async fn test_snapshot_deleted() {
    let fs = DuckAgentFS::open(config).await.unwrap();

    fs.write_file("/test.txt", b"content").await.unwrap();
    let event1 = fs.current_event_id().await.unwrap();

    fs.remove("/test.txt").await.unwrap();

    // Current: file doesn't exist
    let content = fs.read_file("/test.txt").await.unwrap();
    assert!(content.is_none());

    // Snapshot: file exists
    let snapshot = fs.snapshot_at(event1).await.unwrap();
    let content = snapshot.read_file("/test.txt").await.unwrap();
    assert_eq!(content, Some(b"content".to_vec()));
}
```

### Test 3: Diff
```rust
#[tokio::test]
async fn test_diff() {
    let fs = DuckAgentFS::open(config).await.unwrap();

    // Setup: Create files to be modified and deleted
    fs.write_file("/modified.txt", b"v1").await.unwrap();
    fs.write_file("/deleted.txt", b"will be deleted").await.unwrap();

    let start = fs.current_event_id().await.unwrap();

    fs.write_file("/new.txt", b"new").await.unwrap();
    fs.write_file("/modified.txt", b"v2").await.unwrap();
    fs.remove("/deleted.txt").await.unwrap();

    let end = fs.current_event_id().await.unwrap();

    let diffs = fs.diff(start, end).await.unwrap();

    assert!(diffs.iter().any(|d| matches!(d.change_type, ChangeType::Added)));
    assert!(diffs.iter().any(|d| matches!(d.change_type, ChangeType::Modified)));
    assert!(diffs.iter().any(|d| matches!(d.change_type, ChangeType::Deleted)));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/filesystem/duckagentfs.rs` | Snapshot implementation |
| `schema/duckagentfs.sql` | fs_journal table |

## Implementation Notes

1. **Performance**: Historical queries may be slow. Consider:
   - Indexes on (inode, event_id)
   - Materialized views for frequent snapshots
   - Compaction of old events

2. **Storage**: Journal grows indefinitely. Implement:
   - Compaction for old events
   - Archiving to external storage

3. **Concurrency**: Snapshots can be created while writes occur. Ensure consistency via event_id ordering.

## QA Notes

### Test Coverage Summary

| Area | Coverage | Status |
|------|----------|--------|
| `snapshot_at()` API | 3 tests defined | ✅ Covered |
| `current_event_id()` | Implicit in tests | ⚠️ Needs explicit test |
| `list_events()` pagination | Not covered | ❌ Missing |
| `diff()` API | 1 test defined | ⚠️ Partial |
| Read-only enforcement | Not covered | ❌ Missing |
| Error handling (invalid event_id) | Not covered | ❌ Missing |

> **Note:** CLI commands coverage tracked in STORY-6.1

### Risk Areas Identified

1. **HIGH: SQL Injection in Historical Query** - The `query_current()` method uses string formatting with `self.event_id`. While event_id is i64 (not user-controlled string), the pattern is risky. Recommend parameterized queries.

2. **HIGH: Performance Degradation** - Complex CTEs with window functions over large journals may cause timeouts. No pagination or limits in historical queries.

3. **MEDIUM: Race Condition** - Between checking `event_id` exists and executing snapshot query, events could be compacted/archived. Need transactional consistency.

4. **MEDIUM: Memory Exhaustion** - `list_events()` and `diff()` load all results into memory. Large result sets could OOM.

5. **LOW: Edge Case - Event ID 0** - `current_event_id()` returns 0 for empty journal. `snapshot_at(0)` behavior undefined.

### Recommended Test Scenarios

**P0 - Must Have:**
1. Snapshot at non-existent event_id returns appropriate error
2. Read-only snapshot rejects write operations (write_file, mkdir, remove)
3. `list_events()` pagination with limit=0 and large offset
4. Diff between same event_id returns empty list
5. Snapshot of renamed file returns correct path at that time

**P1 - Should Have:**
1. Performance test: snapshot query with 100K+ journal entries
2. Concurrent snapshot reads during active writes
3. Snapshot at event_id=0 (empty filesystem)
4. `diff()` with invalid from_event > to_event
5. Event timeline ordering consistency

**P2 - Nice to Have:**
1. CLI integration tests for `agentfs snapshot` commands
2. Mount snapshot as FUSE filesystem (read-only verification)
3. Recovery workflow: restore deleted file from snapshot

### Concerns and Blockers

| Type | Description | Severity |
|------|-------------|----------|
| **Technical Debt** | Historical query CTE duplicates event_id 3 times | Maintainability |

> **Note:** CLI blocker removed - scope moved to STORY-6.1 per SCP-2026-01-14. Test 3 setup is correct (creates `/deleted.txt` before deletion).

### Recommendations

1. Add explicit test for `current_event_id()` on empty and populated journals
2. Parameterize all SQL queries in `query_current()`
3. Add `LIMIT` clause to diff query or implement streaming
4. Define behavior for `snapshot_at(0)` - should it return empty FS or error?
5. Fix Test 3 setup: create `/deleted.txt` before deleting it
