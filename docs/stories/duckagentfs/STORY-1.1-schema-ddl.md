# STORY-1.1: Schema DDL DuckDB

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Ready for Development ✅ |
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

---

## Dev Agent Record

### Agent Model Used
- Claude Opus 4.5

### File List
| File | Status | Description |
|------|--------|-------------|
| `schema/duckagentfs.sql` | Modified | Fixed sequence ordering (sequences must be created before tables that reference them) |
| `schema/test_schema.sql` | Created | SQL test suite for schema validation |

### Change Log
- Fixed `fs_event_seq` and `fs_inode_seq` sequences to be created before `fs_journal` table
- Fixed `audit_seq` sequence to be created before `audit_log` table
- Created `test_schema.sql` test suite validating all acceptance criteria

### Completion Notes
- All 8 acceptance criteria verified and passing
- Schema validates correctly in DuckDB 1.1.3
- Functional tests pass: file creation, update, delete, and time-travel queries
- 4 indexes created for common query patterns
- Root directory (inode 1) auto-initialized on schema load

---

## QA Notes

**Reviewer**: Quinn (Test Architect)
**Review Date**: 2026-01-14
**Story Status**: Ready for Review → **PASS with Recommendations**

### Test Coverage Summary

| Area | Coverage | Notes |
|------|----------|-------|
| Core CRUD Operations | ✅ Good | Create, update, delete tests present |
| Time-Travel Queries | ✅ Good | Test 4 covers point-in-time recovery |
| View Logic (fs_current) | ⚠️ Partial | Happy path only, edge cases missing |
| Data Integrity | ⚠️ Partial | No constraint violation tests |
| Performance | ❌ Missing | No load/stress tests for append-only scaling |

**Overall Coverage**: ~60% of critical paths tested

### Risk Areas Identified

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| **fs_current view performance degradation** | High | High | Add materialized view or caching strategy; include performance regression test |
| **Concurrent event insertion race conditions** | Medium | High | Test multi-session concurrent inserts; verify sequence atomicity |
| **Delete logic in fs_current view complexity** | Medium | Medium | The nested subquery for delete exclusion is complex; add edge case tests |
| **Journal table unbounded growth** | High | Medium | Compaction strategy documented but not tested; add compaction validation |
| **Orphaned fs_data chunks** | Medium | Low | No cascade/cleanup when inodes deleted; document expected behavior |

### Recommended Test Scenarios

#### High Priority (Must Have)

1. **Concurrent Operations Test**
   ```
   Given: Multiple sessions inserting events simultaneously
   When: 100 concurrent file creates execute
   Then: All inodes receive unique IDs with no sequence gaps or duplicates
   ```

2. **Delete Edge Cases**
   ```
   Given: A file created, updated 5 times, then deleted
   When: Querying fs_current
   Then: File should NOT appear (verify delete exclusion logic)

   Given: A deleted file with same name recreated
   When: Querying fs_current
   Then: Only the new file appears with new inode
   ```

3. **fs_current View Performance Baseline**
   ```
   Given: 100,000 events in fs_journal
   When: SELECT * FROM fs_current
   Then: Query completes in < 500ms (establish baseline)
   ```

4. **Rename Operation Integrity**
   ```
   Given: File at /parent1/file.txt
   When: Renamed to /parent2/newname.txt
   Then: old_parent and old_name populated; fs_current shows new location only
   ```

#### Medium Priority (Should Have)

5. **Sequence Restart Resilience**
   ```
   Given: Database restarted after events inserted
   When: New event inserted
   Then: event_id continues from max(event_id)+1, not sequence default
   ```

6. **fs_data Chunk Boundary Test**
   ```
   Given: File with data spanning multiple chunks
   When: Chunks retrieved by inode
   Then: chunk_idx ordering produces correct byte sequence
   ```

7. **Time-Travel Consistency**
   ```
   Given: Directory with files created, modified, deleted over time
   When: Snapshot at event_id=N retrieved
   Then: Directory listing matches exact state at that event
   ```

### Concerns and Blockers

#### Concerns (Non-Blocking)

1. **View Complexity**: The `fs_current` view uses a correlated subquery for delete exclusion that may cause query planner issues at scale. Consider rewriting with a LEFT ANTI JOIN pattern.

2. **Missing Constraints**: No CHECK constraints on `event_type` enum values. Typos like 'delet' would silently create invalid events.

3. **Checksum Usage Unclear**: `fs_data.checksum` field exists but no tests validate checksum computation or verification.

4. **Audit Fields Optional**: `actor_id` and `session_id` are nullable but critical for audit trails. Consider documenting when these should be populated.

#### Blockers (None)

No blocking issues identified. Schema is functional and acceptance criteria are met.

### QA Gate Decision

**Decision**: ✅ **PASS**

**Rationale**:
- All 8 acceptance criteria verified and passing
- Core functionality (CRUD + time-travel) tested and working
- Schema design follows sound append-only principles
- Implementation notes acknowledge known limitations (performance, compaction)

**Conditions for Production**:
1. Add concurrent operation tests before high-load deployment
2. Establish performance baseline for fs_current view
3. Implement compaction job before journal exceeds 1M events