# STORY-1.1: Schema DDL DuckDB

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Done |
| **Priority** | Critical |
| **File** | `schema/duckagentfs.sql` |

## User Story

**As a** developer
**I want** a complete DDL schema for DuckDB
**So that** I have the database structure for DuckAgentFS

## Technical Description

The schema implements an append-only journal model for the filesystem, where all operations are recorded as immutable events. The current state is derived through a view that applies ROW_NUMBER() over the most recent events for each inode.

### Append-Only Model vs Direct Mutation

```
SQLite (current AgentFS):
UPDATE fs_inode SET size = 100 WHERE ino = 5;
-- Previous history is lost

DuckDB (DuckAgentFS):
INSERT INTO fs_journal (inode, event_type, size, ...) VALUES (5, 'update', 100, ...);
-- History preserved, state derived via fs_current
```

## Acceptance Criteria

- [x] Table `fs_journal` with append-only model
- [x] View `fs_current` deriving current state via ROW_NUMBER()
- [x] Table `fs_data` for binary data chunks
- [x] Sequences `fs_event_seq` and `fs_inode_seq`
- [x] Table `kv_store` for key-value store
- [x] Table `tool_calls` for tool tracking
- [x] Indexes for frequent queries
- [x] Root directory initialized (inode 1)

## Technical Specification

### Table fs_journal

```sql
CREATE TABLE fs_journal (
    event_id    UBIGINT PRIMARY KEY DEFAULT nextval('fs_event_seq'),
    inode       UBIGINT NOT NULL,
    event_type  VARCHAR NOT NULL,  -- 'create', 'update', 'delete', 'rename', 'chmod'
    event_time  TIMESTAMP DEFAULT current_timestamp,

    -- Inode state snapshot
    parent      UBIGINT,
    name        VARCHAR,
    mode        UINTEGER,
    uid         UINTEGER DEFAULT 0,
    gid         UINTEGER DEFAULT 0,
    size        UBIGINT DEFAULT 0,
    nlink       UINTEGER DEFAULT 1,

    -- For renames
    old_parent  UBIGINT,
    old_name    VARCHAR,

    -- Audit
    actor_id    VARCHAR,
    session_id  VARCHAR,
    metadata    JSON
);
```

### View fs_current

```sql
CREATE VIEW fs_current AS
WITH ranked AS (
    SELECT *,
           ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_type != 'delete'
)
SELECT inode, parent, name, mode, uid, gid, size, nlink,
       event_time as mtime, event_id as last_event_id
FROM ranked
WHERE rn = 1
  AND inode NOT IN (
      SELECT inode FROM fs_journal
      WHERE event_type = 'delete'
      AND event_id = (SELECT MAX(event_id) FROM fs_journal j2 WHERE j2.inode = fs_journal.inode)
  );
```

### Table fs_data

```sql
CREATE TABLE fs_data (
    inode       UBIGINT NOT NULL,
    chunk_idx   UINTEGER NOT NULL,
    data        BLOB NOT NULL,
    checksum    VARCHAR,
    created_at  TIMESTAMP DEFAULT current_timestamp,
    PRIMARY KEY (inode, chunk_idx)
);
```

## Tests

### Test 1: File Creation
```sql
-- Create file
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (2, 'create', 1, 'test.txt', 33188, 0);

-- Verify in fs_current
SELECT * FROM fs_current WHERE name = 'test.txt';
-- Expected: 1 row with inode=2
```

### Test 2: File Update
```sql
-- Update
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (2, 'update', 1, 'test.txt', 33188, 100);

-- Verify that fs_current returns most recent version
SELECT size FROM fs_current WHERE inode = 2;
-- Expected: size = 100
```

### Test 3: File Delete
```sql
-- Delete
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (2, 'delete', 1, 'test.txt', 33188);

-- Verify it doesn't appear in fs_current
SELECT * FROM fs_current WHERE inode = 2;
-- Expected: 0 rows
```

### Test 4: Time-Travel
```sql
-- Get state at specific event_id
WITH ranked AS (
    SELECT *,
           ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_id <= 10  -- Snapshot at event 10
      AND event_type != 'delete'
)
SELECT * FROM ranked WHERE rn = 1;
```

## Dependencies

- DuckDB >= 0.9.0
- VSS extension (for future stories)
- DuckPGQ extension (for future stories)

## Related Files

| File | Description |
|------|-------------|
| `schema/duckagentfs.sql` | Complete DDL |
| `sdk/rust/src/filesystem/duckagentfs.rs` | Implementation using this schema |

## Implementation Notes

1. **Performance**: The `fs_current` view may be slow for many events. Consider materialized view or cache table for production.

2. **Compaction**: Implement periodic job to compact old events:
   ```sql
   -- Move old events to archive
   CREATE TABLE fs_journal_archive AS
   SELECT * FROM fs_journal WHERE event_time < current_date - INTERVAL '90 days';
   ```

3. **Indexes**: Created indexes are optimized for:
   - Lookup by inode
   - Lookup by parent (directory listing)
   - Lookup by name
   - Ordering by event_time (time-travel)
