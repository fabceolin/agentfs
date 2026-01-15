# Test Design: STORY-1.1 Schema DDL DuckDB

| Field | Value |
|-------|-------|
| **Story ID** | STORY-1.1 |
| **Test Design Version** | 1.0 |
| **QA Engineer** | QA Agent |
| **Date** | 2026-01-14 |
| **Status** | Ready for Implementation |

---

## 1. Overview

### 1.1 Purpose

This test design document provides comprehensive test coverage for the DuckAgentFS Schema DDL implementation. The schema implements an append-only journal model for filesystem operations, where all operations are recorded as immutable events and the current state is derived through views.

### 1.2 Scope

| In Scope | Out of Scope |
|----------|--------------|
| DDL schema creation and validation | Application-layer integration |
| fs_journal table operations | SDK/Rust implementation |
| fs_current view correctness | VSS/DuckPGQ extensions |
| fs_data chunk storage | Production deployment |
| Sequence behavior | Compaction job implementation |
| Index effectiveness | Full performance benchmarking |
| kv_store and tool_calls tables | |
| Time-travel queries | |

### 1.3 Test Strategy

```
┌─────────────────────────────────────────────────────────────────┐
│                        Test Pyramid                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│                    ┌─────────────┐                              │
│                    │  E2E Tests  │  (5%)                        │
│                    │  Time-travel│                              │
│                    └─────────────┘                              │
│               ┌───────────────────────┐                         │
│               │   Integration Tests   │  (25%)                  │
│               │   View correctness    │                         │
│               │   Concurrent ops      │                         │
│               └───────────────────────┘                         │
│          ┌───────────────────────────────────┐                  │
│          │         Unit Tests                │  (70%)           │
│          │   Table structure, constraints,   │                  │
│          │   sequences, indexes, CRUD        │                  │
│          └───────────────────────────────────┘                  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. Test Categories

### 2.1 Test Category Matrix

| Category | Priority | Test Count | Automation |
|----------|----------|------------|------------|
| Schema Structure | Critical | 12 | Automated |
| fs_journal CRUD | Critical | 15 | Automated |
| fs_current View | Critical | 18 | Automated |
| fs_data Operations | High | 8 | Automated |
| Sequence Behavior | High | 6 | Automated |
| Index Effectiveness | Medium | 5 | Automated |
| Time-Travel | High | 8 | Automated |
| Concurrent Operations | High | 6 | Automated |
| Edge Cases | Medium | 12 | Automated |
| Performance Baseline | Medium | 5 | Semi-Auto |

**Total Test Count: 95**

---

## 3. Schema Structure Tests

### 3.1 Table Existence Tests

#### TC-1.1.001: fs_journal Table Exists

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.001 |
| **Priority** | Critical |
| **Type** | Schema Validation |

**Preconditions:**
- DuckDB database initialized
- Schema DDL script executed

**Test Steps:**
1. Execute schema DDL
2. Query system catalog for fs_journal table

**Test Data:**
```sql
SELECT table_name FROM information_schema.tables
WHERE table_name = 'fs_journal';
```

**Expected Result:**
- Query returns exactly 1 row
- table_name = 'fs_journal'

---

#### TC-1.1.002: fs_journal Column Schema Validation

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.002 |
| **Priority** | Critical |
| **Type** | Schema Validation |

**Preconditions:**
- fs_journal table exists

**Test Steps:**
1. Query column metadata for fs_journal
2. Validate each column name, type, and nullability

**Expected Result:**

| Column | Type | Nullable | Default |
|--------|------|----------|---------|
| event_id | UBIGINT | NO | nextval('fs_event_seq') |
| inode | UBIGINT | NO | - |
| event_type | VARCHAR | NO | - |
| event_time | TIMESTAMP | YES | current_timestamp |
| parent | UBIGINT | YES | - |
| name | VARCHAR | YES | - |
| mode | UINTEGER | YES | - |
| uid | UINTEGER | YES | 0 |
| gid | UINTEGER | YES | 0 |
| size | UBIGINT | YES | 0 |
| nlink | UINTEGER | YES | 1 |
| old_parent | UBIGINT | YES | - |
| old_name | VARCHAR | YES | - |
| actor_id | VARCHAR | YES | - |
| session_id | VARCHAR | YES | - |
| metadata | JSON | YES | - |

---

#### TC-1.1.003: fs_current View Exists

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.003 |
| **Priority** | Critical |
| **Type** | Schema Validation |

**Preconditions:**
- Schema DDL executed

**Test Steps:**
1. Query system catalog for fs_current view

**Test Data:**
```sql
SELECT table_name FROM information_schema.tables
WHERE table_name = 'fs_current' AND table_type = 'VIEW';
```

**Expected Result:**
- Query returns exactly 1 row

---

#### TC-1.1.004: fs_data Table Exists and Schema Valid

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.004 |
| **Priority** | Critical |
| **Type** | Schema Validation |

**Expected Result:**

| Column | Type | Nullable | Notes |
|--------|------|----------|-------|
| inode | UBIGINT | NO | Part of PK |
| chunk_idx | UINTEGER | NO | Part of PK |
| data | BLOB | NO | - |
| checksum | VARCHAR | YES | - |
| created_at | TIMESTAMP | YES | default current_timestamp |

---

#### TC-1.1.005: Sequences Created

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.005 |
| **Priority** | Critical |
| **Type** | Schema Validation |

**Test Steps:**
1. Verify fs_event_seq exists
2. Verify fs_inode_seq exists

**Test Data:**
```sql
SELECT sequence_name FROM information_schema.sequences
WHERE sequence_name IN ('fs_event_seq', 'fs_inode_seq');
```

**Expected Result:**
- 2 rows returned
- Both sequences present

---

#### TC-1.1.006: kv_store Table Schema

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.006 |
| **Priority** | High |
| **Type** | Schema Validation |

**Expected Result:**
- Table exists
- Columns: key (VARCHAR, PK), value (JSON), created_at, updated_at (TIMESTAMP)

---

#### TC-1.1.007: tool_calls Table Schema

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.007 |
| **Priority** | High |
| **Type** | Schema Validation |

**Expected Result:**
- Table exists with appropriate columns for tool tracking

---

#### TC-1.1.008: Index Existence Validation

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.008 |
| **Priority** | High |
| **Type** | Schema Validation |

**Test Steps:**
1. Query system catalog for indexes on fs_journal

**Expected Result:**
- Index on inode column
- Index on parent column
- Index on name column
- Index on event_time column

---

#### TC-1.1.009: Root Directory Initialized

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.009 |
| **Priority** | Critical |
| **Type** | Data Validation |

**Test Steps:**
1. Execute schema DDL
2. Query fs_current for inode 1

**Test Data:**
```sql
SELECT inode, parent, name, mode FROM fs_current WHERE inode = 1;
```

**Expected Result:**
- 1 row returned
- inode = 1
- parent = NULL or 0
- name = '' or '/'
- mode indicates directory (S_IFDIR | 0755 = 16877)

---

### 3.2 Primary Key Constraints

#### TC-1.1.010: fs_journal Primary Key

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.010 |
| **Priority** | Critical |
| **Type** | Constraint Validation |

**Test Steps:**
1. Attempt to insert two events with same event_id

**Test Data:**
```sql
-- Should fail due to PK violation
INSERT INTO fs_journal (event_id, inode, event_type) VALUES (999999, 2, 'create');
INSERT INTO fs_journal (event_id, inode, event_type) VALUES (999999, 3, 'create');
```

**Expected Result:**
- First INSERT succeeds
- Second INSERT fails with primary key violation error

---

#### TC-1.1.011: fs_data Composite Primary Key

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.011 |
| **Priority** | Critical |
| **Type** | Constraint Validation |

**Test Steps:**
1. Insert chunk (inode=5, chunk_idx=0)
2. Attempt to insert duplicate (inode=5, chunk_idx=0)

**Expected Result:**
- First INSERT succeeds
- Second INSERT fails with primary key violation

---

#### TC-1.1.012: Default Values Applied

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.012 |
| **Priority** | High |
| **Type** | Data Validation |

**Test Steps:**
1. Insert minimal fs_journal record
2. Verify default values

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (100, 'create', 1, 'defaults_test.txt', 33188);

SELECT uid, gid, size, nlink, event_time
FROM fs_journal WHERE inode = 100;
```

**Expected Result:**
- uid = 0
- gid = 0
- size = 0
- nlink = 1
- event_time IS NOT NULL (auto-populated)

---

## 4. fs_journal CRUD Tests

### 4.1 Create Operations

#### TC-1.1.013: File Creation Event

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.013 |
| **Priority** | Critical |
| **Type** | Functional |

**Preconditions:**
- Empty database with schema loaded

**Test Steps:**
1. Insert 'create' event for new file
2. Query fs_journal to verify event
3. Query fs_current to verify file appears

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (nextval('fs_inode_seq'), 'create', 1, 'newfile.txt', 33188, 0);
```

**Expected Result:**
- Event recorded in fs_journal with auto-generated event_id
- File appears in fs_current with correct attributes

---

#### TC-1.1.014: Directory Creation Event

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.014 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (nextval('fs_inode_seq'), 'create', 1, 'subdir', 16877);
-- mode 16877 = S_IFDIR | 0755
```

**Expected Result:**
- Directory appears in fs_current
- mode indicates directory type

---

#### TC-1.1.015: Symlink Creation Event

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.015 |
| **Priority** | High |
| **Type** | Functional |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (nextval('fs_inode_seq'), 'create', 1, 'link', 41471);
-- mode 41471 = S_IFLNK | 0777
```

**Expected Result:**
- Symlink appears in fs_current with correct mode

---

### 4.2 Update Operations

#### TC-1.1.016: File Size Update

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.016 |
| **Priority** | Critical |
| **Type** | Functional |

**Preconditions:**
- File with inode 2 exists

**Test Steps:**
1. Insert 'update' event with new size
2. Query fs_current to verify size changed

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (2, 'update', 1, 'test.txt', 33188, 1024);

SELECT size FROM fs_current WHERE inode = 2;
```

**Expected Result:**
- size = 1024

---

#### TC-1.1.017: Multiple Sequential Updates

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.017 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Create file
2. Update size to 100
3. Update size to 200
4. Update size to 300
5. Query fs_current

**Expected Result:**
- fs_current shows size = 300 (most recent)
- fs_journal contains 4 events for this inode
- Only one row in fs_current for this inode

---

#### TC-1.1.018: chmod Event

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.018 |
| **Priority** | High |
| **Type** | Functional |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (2, 'chmod', 1, 'test.txt', 33261);
-- 33261 = regular file + 0755

SELECT mode FROM fs_current WHERE inode = 2;
```

**Expected Result:**
- mode = 33261

---

### 4.3 Delete Operations

#### TC-1.1.019: File Deletion

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.019 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Create file
2. Insert 'delete' event
3. Query fs_current

**Test Data:**
```sql
-- Create file
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (10, 'create', 1, 'todelete.txt', 33188);

-- Delete
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (10, 'delete', 1, 'todelete.txt', 33188);

-- Verify gone from fs_current
SELECT COUNT(*) FROM fs_current WHERE inode = 10;
```

**Expected Result:**
- COUNT = 0 (file not in fs_current)
- Both events still in fs_journal (append-only)

---

#### TC-1.1.020: Delete After Multiple Updates

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.020 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Create file
2. Update 5 times
3. Delete
4. Query fs_current

**Expected Result:**
- File does NOT appear in fs_current
- All 7 events preserved in fs_journal

---

#### TC-1.1.021: Directory Deletion

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.021 |
| **Priority** | High |
| **Type** | Functional |

**Expected Result:**
- Directory removed from fs_current after delete event

---

### 4.4 Rename Operations

#### TC-1.1.022: File Rename Same Directory

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.022 |
| **Priority** | High |
| **Type** | Functional |

**Test Data:**
```sql
-- Create original
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (20, 'create', 1, 'oldname.txt', 33188);

-- Rename
INSERT INTO fs_journal (inode, event_type, parent, name, mode, old_parent, old_name)
VALUES (20, 'rename', 1, 'newname.txt', 33188, 1, 'oldname.txt');

SELECT name, old_name FROM fs_journal WHERE inode = 20 ORDER BY event_id DESC LIMIT 1;
SELECT name FROM fs_current WHERE inode = 20;
```

**Expected Result:**
- fs_current shows name = 'newname.txt'
- old_name = 'oldname.txt' in journal event

---

#### TC-1.1.023: File Move to Different Directory

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.023 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Create directory (inode 30)
2. Create file in root (inode 31)
3. Move file to directory

**Test Data:**
```sql
-- Move file from root (parent=1) to subdir (parent=30)
INSERT INTO fs_journal (inode, event_type, parent, name, mode, old_parent, old_name)
VALUES (31, 'rename', 30, 'moved.txt', 33188, 1, 'original.txt');
```

**Expected Result:**
- fs_current shows parent = 30
- old_parent = 1 preserved in journal

---

### 4.5 Audit Fields

#### TC-1.1.024: Actor and Session Tracking

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.024 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode, actor_id, session_id)
VALUES (40, 'create', 1, 'audited.txt', 33188, 'user-123', 'sess-abc-456');

SELECT actor_id, session_id FROM fs_journal WHERE inode = 40;
```

**Expected Result:**
- actor_id = 'user-123'
- session_id = 'sess-abc-456'

---

#### TC-1.1.025: Metadata JSON Storage

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.025 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode, metadata)
VALUES (41, 'create', 1, 'meta.txt', 33188, '{"source": "api", "version": "1.0"}');

SELECT metadata->>'source' as src FROM fs_journal WHERE inode = 41;
```

**Expected Result:**
- src = 'api'

---

## 5. fs_current View Tests

### 5.1 View Correctness

#### TC-1.1.026: View Returns Most Recent State

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.026 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Create file
2. Update 10 times with incrementing sizes
3. Query fs_current

**Expected Result:**
- Only 1 row returned for this inode
- size = value from 10th update

---

#### TC-1.1.027: View Excludes Deleted Files

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.027 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Create 5 files
2. Delete 2 of them
3. Query fs_current

**Expected Result:**
- Only 3 files returned (5 - 2 deleted)

---

#### TC-1.1.028: View mtime Equals Last Event Time

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.028 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Create file
2. Wait 1 second
3. Update file
4. Query fs_current.mtime

**Expected Result:**
- mtime matches event_time of most recent event

---

#### TC-1.1.029: View last_event_id Correct

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.029 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Create file
2. Update file
3. Query fs_journal max event_id for inode
4. Query fs_current.last_event_id

**Expected Result:**
- last_event_id matches max(event_id) from fs_journal

---

### 5.2 Delete Exclusion Logic

#### TC-1.1.030: Delete After Create Hides File

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.030 |
| **Priority** | Critical |
| **Type** | Functional |

**Scenario:** Verify the complex delete exclusion subquery works

**Test Steps:**
1. Create file (event 1)
2. Delete file (event 2)
3. Query fs_current

**Expected Result:**
- File NOT in fs_current

---

#### TC-1.1.031: Create After Delete Shows New File

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.031 |
| **Priority** | Critical |
| **Type** | Edge Case |

**Scenario:** Same inode recreated after deletion

**Test Steps:**
1. Create file inode=50
2. Delete file inode=50
3. Create NEW file reusing inode=50 (in practice, would be new inode)
4. Query fs_current

**NOTE:** In practice, deleted inodes should not be reused. Test with different inode.

**Alternative Test:**
1. Create file1 with name 'reuse.txt'
2. Delete file1
3. Create file2 with name 'reuse.txt' (new inode)
4. Query fs_current

**Expected Result:**
- Only file2 appears (new inode)

---

#### TC-1.1.032: Delete Is Final State

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.032 |
| **Priority** | Critical |
| **Type** | Edge Case |

**Scenario:** Verify update after delete doesn't resurrect file (edge case that shouldn't happen in practice but tests view logic)

**Test Steps:**
1. Create file
2. Update file
3. Delete file
4. (Artificially) Insert another 'update' event

**Expected Result:**
- File should NOT appear in fs_current (delete was most recent non-update event)
- OR: The view logic correctly identifies delete as final

**NOTE:** This tests the fs_current view's delete detection logic robustness.

---

### 5.3 Directory Listing

#### TC-1.1.033: List Directory Contents

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.033 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Create directory (inode 60)
2. Create 5 files with parent=60
3. Query fs_current WHERE parent = 60

**Expected Result:**
- 5 rows returned

---

#### TC-1.1.034: Root Directory Listing

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.034 |
| **Priority** | High |
| **Type** | Functional |

**Test Data:**
```sql
SELECT * FROM fs_current WHERE parent = 1 ORDER BY name;
```

**Expected Result:**
- All files/directories created in root returned
- Ordered by name

---

## 6. fs_data Chunk Storage Tests

### 6.1 Chunk Operations

#### TC-1.1.035: Single Chunk Write

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.035 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Data:**
```sql
INSERT INTO fs_data (inode, chunk_idx, data, checksum)
VALUES (70, 0, '\x48454C4C4F'::BLOB, 'sha256:abc123');

SELECT data FROM fs_data WHERE inode = 70 AND chunk_idx = 0;
```

**Expected Result:**
- Data retrieved matches inserted data
- checksum stored correctly

---

#### TC-1.1.036: Multi-Chunk Write

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.036 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Insert chunks 0, 1, 2 for inode 71
2. Query all chunks ordered by chunk_idx

**Expected Result:**
- 3 chunks returned
- Correctly ordered by chunk_idx

---

#### TC-1.1.037: Chunk Overwrite

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.037 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Insert chunk (inode=72, chunk_idx=0) with data 'OLD'
2. Attempt UPDATE (if supported) or DELETE + INSERT
3. Verify new data

**NOTE:** Depending on schema design, chunks may be append-only or updateable.

---

#### TC-1.1.038: Chunk Deletion

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.038 |
| **Priority** | High |
| **Type** | Functional |

**Test Data:**
```sql
DELETE FROM fs_data WHERE inode = 70;
SELECT COUNT(*) FROM fs_data WHERE inode = 70;
```

**Expected Result:**
- COUNT = 0

---

#### TC-1.1.039: Large Blob Storage

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.039 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Steps:**
1. Generate 1MB blob
2. Insert as single chunk
3. Retrieve and verify integrity

**Expected Result:**
- Data integrity maintained for large blobs

---

### 6.2 Chunk Boundary Tests

#### TC-1.1.040: Chunk Reassembly Order

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.040 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Insert chunks out of order: 2, 0, 1
2. Query with ORDER BY chunk_idx
3. Reassemble data

**Test Data:**
```sql
INSERT INTO fs_data VALUES (80, 2, '\x33'::BLOB, NULL, current_timestamp);
INSERT INTO fs_data VALUES (80, 0, '\x31'::BLOB, NULL, current_timestamp);
INSERT INTO fs_data VALUES (80, 1, '\x32'::BLOB, NULL, current_timestamp);

SELECT data FROM fs_data WHERE inode = 80 ORDER BY chunk_idx;
```

**Expected Result:**
- Data returned in correct order: 0x31, 0x32, 0x33

---

#### TC-1.1.041: Gap in Chunk Indexes

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.041 |
| **Priority** | Medium |
| **Type** | Edge Case |

**Scenario:** Chunks 0, 2 exist but not 1

**Expected Result:**
- Query returns chunks 0 and 2
- Application layer responsible for detecting gaps

---

#### TC-1.1.042: Checksum Verification

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.042 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Steps:**
1. Insert chunk with known checksum
2. Query and verify checksum matches expected

---

## 7. Sequence Behavior Tests

### 7.1 fs_event_seq

#### TC-1.1.043: Event ID Auto-Increment

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.043 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Insert 3 events without specifying event_id
2. Query event_ids

**Expected Result:**
- event_ids are sequential (e.g., N, N+1, N+2)

---

#### TC-1.1.044: Event Sequence No Gaps Under Normal Operation

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.044 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Note current sequence value
2. Insert 100 events
3. Verify no gaps in event_ids

**Expected Result:**
- All 100 event_ids are consecutive

---

#### TC-1.1.045: Sequence Survives Restart

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.045 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Insert events
2. Note max(event_id)
3. Reconnect to database
4. Insert new event
5. Verify event_id > previous max

**Expected Result:**
- No event_id collision after restart

---

### 7.2 fs_inode_seq

#### TC-1.1.046: Inode Sequence Uniqueness

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.046 |
| **Priority** | Critical |
| **Type** | Functional |

**Test Steps:**
1. Generate 100 inodes via nextval('fs_inode_seq')
2. Verify all unique

**Expected Result:**
- 100 unique inode values

---

#### TC-1.1.047: Inode Sequence Starts After Root

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.047 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Fresh database
2. Query nextval('fs_inode_seq')

**Expected Result:**
- Value > 1 (root is inode 1)

---

#### TC-1.1.048: Explicit Inode Assignment

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.048 |
| **Priority** | Medium |
| **Type** | Functional |

**Scenario:** Can assign explicit inode (for restore/import scenarios)

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (9999, 'create', 1, 'explicit_inode.txt', 33188);
```

**Expected Result:**
- Insert succeeds with explicit inode value

---

## 8. Index Effectiveness Tests

### 8.1 Query Plan Analysis

#### TC-1.1.049: Inode Lookup Uses Index

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.049 |
| **Priority** | Medium |
| **Type** | Performance |

**Test Data:**
```sql
EXPLAIN ANALYZE SELECT * FROM fs_journal WHERE inode = 100;
```

**Expected Result:**
- Query plan shows index scan (not seq scan)

---

#### TC-1.1.050: Parent Lookup Uses Index

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.050 |
| **Priority** | Medium |
| **Type** | Performance |

**Test Data:**
```sql
EXPLAIN ANALYZE SELECT * FROM fs_journal WHERE parent = 1;
```

**Expected Result:**
- Index scan on parent index

---

#### TC-1.1.051: Name Lookup Uses Index

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.051 |
| **Priority** | Medium |
| **Type** | Performance |

**Test Data:**
```sql
EXPLAIN ANALYZE SELECT * FROM fs_journal WHERE name = 'test.txt';
```

**Expected Result:**
- Index scan on name index

---

#### TC-1.1.052: Event Time Range Uses Index

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.052 |
| **Priority** | Medium |
| **Type** | Performance |

**Test Data:**
```sql
EXPLAIN ANALYZE SELECT * FROM fs_journal
WHERE event_time BETWEEN '2024-01-01' AND '2024-12-31';
```

**Expected Result:**
- Index scan on event_time index

---

#### TC-1.1.053: fs_current View Query Plan

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.053 |
| **Priority** | Medium |
| **Type** | Performance |

**Test Steps:**
1. Analyze query plan for fs_current

**Expected Result:**
- Plan uses indexes effectively
- Window function (ROW_NUMBER) optimization visible

---

## 9. Time-Travel Query Tests

### 9.1 Point-in-Time Recovery

#### TC-1.1.054: Snapshot at Event ID

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.054 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Create file (event E1)
2. Update file (event E2)
3. Update file (event E3)
4. Query snapshot at E2

**Test Data:**
```sql
WITH ranked AS (
    SELECT *,
           ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_id <= @snapshot_event_id
      AND event_type != 'delete'
)
SELECT * FROM ranked WHERE rn = 1;
```

**Expected Result:**
- File shows state from E2 (not E3)

---

#### TC-1.1.055: Snapshot Before File Creation

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.055 |
| **Priority** | High |
| **Type** | Edge Case |

**Test Steps:**
1. Note current max event_id (N)
2. Create file (event N+1)
3. Query snapshot at N

**Expected Result:**
- File does NOT appear in snapshot

---

#### TC-1.1.056: Snapshot After File Deletion

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.056 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Create file (E1)
2. Delete file (E2)
3. Query snapshot at E1

**Expected Result:**
- File APPEARS in snapshot (delete hadn't happened yet)

---

#### TC-1.1.057: Directory Listing at Point-in-Time

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.057 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Create 3 files
2. Note event_id (snapshot point)
3. Create 2 more files
4. Delete 1 original file
5. Query directory listing at snapshot point

**Expected Result:**
- Listing shows exactly 3 files (state at snapshot)

---

### 9.2 Time-Based Queries

#### TC-1.1.058: Snapshot at Timestamp

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.058 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Data:**
```sql
WITH ranked AS (
    SELECT *,
           ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_time <= @timestamp
      AND event_type != 'delete'
)
SELECT * FROM ranked WHERE rn = 1;
```

**Expected Result:**
- Returns state as of specified timestamp

---

#### TC-1.1.059: Event History for Inode

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.059 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Data:**
```sql
SELECT event_id, event_type, event_time, size
FROM fs_journal WHERE inode = @inode
ORDER BY event_id;
```

**Expected Result:**
- Complete history of all events for inode

---

#### TC-1.1.060: Changes Between Event IDs

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.060 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Data:**
```sql
SELECT * FROM fs_journal
WHERE event_id > @start_event AND event_id <= @end_event
ORDER BY event_id;
```

**Expected Result:**
- All events in the specified range

---

#### TC-1.1.061: Full Journal Audit Trail

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.061 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Data:**
```sql
SELECT event_id, inode, event_type, actor_id, session_id, event_time
FROM fs_journal
ORDER BY event_id;
```

**Expected Result:**
- Complete chronological audit trail

---

## 10. Concurrent Operation Tests

### 10.1 Parallel Inserts

#### TC-1.1.062: Concurrent Event Inserts

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.062 |
| **Priority** | Critical |
| **Type** | Concurrency |

**Test Steps:**
1. Open 10 concurrent sessions
2. Each session inserts 100 events simultaneously
3. Verify no duplicate event_ids

**Expected Result:**
- 1000 total events created
- All event_ids unique
- No constraint violations

---

#### TC-1.1.063: Concurrent File Creates

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.063 |
| **Priority** | Critical |
| **Type** | Concurrency |

**Test Steps:**
1. 50 concurrent sessions create files
2. Each uses nextval('fs_inode_seq')
3. Verify all inodes unique

**Expected Result:**
- 50 unique inodes
- All files appear in fs_current

---

#### TC-1.1.064: Concurrent Updates to Same Inode

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.064 |
| **Priority** | High |
| **Type** | Concurrency |

**Test Steps:**
1. Create file
2. 10 sessions simultaneously update the same file
3. Verify fs_current shows most recent update

**Expected Result:**
- All 10 update events recorded
- fs_current shows highest event_id's state

---

### 10.2 Read-Write Concurrency

#### TC-1.1.065: Read During Write

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.065 |
| **Priority** | High |
| **Type** | Concurrency |

**Test Steps:**
1. Session A continuously inserts events
2. Session B continuously reads fs_current
3. Run for 10 seconds

**Expected Result:**
- No errors or deadlocks
- Session B always sees consistent state

---

#### TC-1.1.066: fs_current Consistency Under Load

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.066 |
| **Priority** | High |
| **Type** | Concurrency |

**Test Steps:**
1. Populate 10000 events
2. Concurrent: Writer inserts updates
3. Concurrent: Reader queries fs_current repeatedly
4. Verify reader never sees partial state

**Expected Result:**
- View always returns complete, consistent rows

---

#### TC-1.1.067: Sequence Atomicity

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.067 |
| **Priority** | Critical |
| **Type** | Concurrency |

**Test Steps:**
1. 100 concurrent nextval('fs_event_seq') calls
2. Collect all values
3. Verify no duplicates

**Expected Result:**
- 100 unique sequence values

---

## 11. Edge Case Tests

### 11.1 Boundary Conditions

#### TC-1.1.068: Empty fs_journal

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.068 |
| **Priority** | Medium |
| **Type** | Edge Case |

**Test Steps:**
1. Fresh database (only root)
2. Query fs_current

**Expected Result:**
- Only root directory returned
- No errors

---

#### TC-1.1.069: Single Event Per Inode

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.069 |
| **Priority** | Medium |
| **Type** | Edge Case |

**Test Steps:**
1. Create file (single event)
2. Query fs_current

**Expected Result:**
- File appears correctly

---

#### TC-1.1.070: Maximum VARCHAR Length

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.070 |
| **Priority** | Low |
| **Type** | Edge Case |

**Test Steps:**
1. Insert event with very long name (10000 chars)
2. Verify storage and retrieval

**Expected Result:**
- Stored and retrieved correctly (or documented limit)

---

#### TC-1.1.071: Special Characters in Name

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.071 |
| **Priority** | Medium |
| **Type** | Edge Case |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (200, 'create', 1, 'file with spaces & "quotes".txt', 33188);

INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (201, 'create', 1, 'unicode_文件名.txt', 33188);

INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (202, 'create', 1, E'newline\nfile.txt', 33188);
```

**Expected Result:**
- All special characters preserved correctly

---

#### TC-1.1.072: NULL Metadata JSON

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.072 |
| **Priority** | Low |
| **Type** | Edge Case |

**Test Steps:**
1. Insert event with metadata = NULL
2. Query and verify

**Expected Result:**
- NULL metadata handled correctly

---

#### TC-1.1.073: Empty JSON Object Metadata

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.073 |
| **Priority** | Low |
| **Type** | Edge Case |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode, metadata)
VALUES (210, 'create', 1, 'empty_meta.txt', 33188, '{}');
```

**Expected Result:**
- Empty JSON object stored and retrieved

---

### 11.2 Invalid Input Handling

#### TC-1.1.074: Invalid Event Type

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.074 |
| **Priority** | Medium |
| **Type** | Negative |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (220, 'invalid_type', 1, 'bad.txt', 33188);
```

**Expected Result:**
- **Current behavior:** Insert succeeds (no CHECK constraint)
- **Recommended:** Add CHECK constraint to reject invalid types

**NOTE:** This is a gap identified in QA Notes - no constraint on event_type.

---

#### TC-1.1.075: Negative Inode

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.075 |
| **Priority** | Low |
| **Type** | Negative |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (-1, 'create', 1, 'negative.txt', 33188);
```

**Expected Result:**
- Rejected (UBIGINT cannot be negative)

---

#### TC-1.1.076: Invalid JSON Metadata

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.076 |
| **Priority** | Low |
| **Type** | Negative |

**Test Data:**
```sql
INSERT INTO fs_journal (inode, event_type, parent, name, mode, metadata)
VALUES (230, 'create', 1, 'badjson.txt', 33188, 'not valid json');
```

**Expected Result:**
- Rejected with JSON parse error

---

### 11.3 Orphan Handling

#### TC-1.1.077: Orphaned fs_data Chunks

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.077 |
| **Priority** | Medium |
| **Type** | Edge Case |

**Scenario:** fs_data chunks exist for deleted inode

**Test Steps:**
1. Create file, add chunks
2. Delete file (journal event)
3. Query fs_data for inode

**Expected Result:**
- fs_data chunks still exist (no cascade)
- Documented behavior: cleanup is application responsibility

---

#### TC-1.1.078: fs_data Without Corresponding Inode

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.078 |
| **Priority** | Low |
| **Type** | Edge Case |

**Test Steps:**
1. Insert fs_data for inode 9999 (doesn't exist in journal)
2. Verify insert succeeds

**Expected Result:**
- Insert succeeds (no FK constraint)
- Referential integrity is application responsibility

---

#### TC-1.1.079: Hardlink nlink Tracking

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.079 |
| **Priority** | Low |
| **Type** | Edge Case |

**Test Steps:**
1. Create file with nlink=1
2. Update nlink=2 (simulating hardlink)
3. Verify nlink in fs_current

**Expected Result:**
- nlink correctly tracked

---

## 12. Performance Baseline Tests

### 12.1 Query Performance

#### TC-1.1.080: fs_current 10K Events

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.080 |
| **Priority** | High |
| **Type** | Performance |

**Preconditions:**
- 10,000 events in fs_journal
- ~1,000 unique inodes

**Test Data:**
```sql
-- Measure execution time
SELECT * FROM fs_current;
```

**Expected Result:**
- Query completes in < 100ms
- Establish baseline for regression testing

---

#### TC-1.1.081: fs_current 100K Events

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.081 |
| **Priority** | High |
| **Type** | Performance |

**Preconditions:**
- 100,000 events in fs_journal
- ~10,000 unique inodes

**Expected Result:**
- Query completes in < 500ms (from QA Notes recommendation)
- Document actual baseline

---

#### TC-1.1.082: fs_current 1M Events

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.082 |
| **Priority** | Medium |
| **Type** | Performance |

**Preconditions:**
- 1,000,000 events in fs_journal
- ~100,000 unique inodes

**Expected Result:**
- Query completes in < 5s
- May indicate need for materialized view

---

#### TC-1.1.083: Directory Listing Performance

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.083 |
| **Priority** | High |
| **Type** | Performance |

**Preconditions:**
- Directory with 10,000 children

**Test Data:**
```sql
SELECT * FROM fs_current WHERE parent = @dir_inode;
```

**Expected Result:**
- Query completes in < 200ms

---

#### TC-1.1.084: Time-Travel Query Performance

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.084 |
| **Priority** | Medium |
| **Type** | Performance |

**Preconditions:**
- 100,000 events

**Test Data:**
```sql
WITH ranked AS (
    SELECT *,
           ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_id <= 50000
      AND event_type != 'delete'
)
SELECT * FROM ranked WHERE rn = 1;
```

**Expected Result:**
- Query completes in < 500ms

---

## 13. kv_store and tool_calls Tests

### 13.1 kv_store Operations

#### TC-1.1.085: Key-Value Insert

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.085 |
| **Priority** | High |
| **Type** | Functional |

**Test Data:**
```sql
INSERT INTO kv_store (key, value) VALUES ('config.theme', '"dark"');
SELECT value FROM kv_store WHERE key = 'config.theme';
```

**Expected Result:**
- Value stored and retrieved correctly

---

#### TC-1.1.086: Key-Value Update

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.086 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Insert key
2. Update value
3. Verify updated_at changed

---

#### TC-1.1.087: Complex JSON Value

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.087 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Data:**
```sql
INSERT INTO kv_store (key, value) VALUES
('settings', '{"nested": {"deep": {"value": 123}}}');

SELECT value->'nested'->'deep'->>'value' FROM kv_store WHERE key = 'settings';
```

**Expected Result:**
- JSON path query returns '123'

---

### 13.2 tool_calls Operations

#### TC-1.1.088: Tool Call Recording

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.088 |
| **Priority** | High |
| **Type** | Functional |

**Test Steps:**
1. Insert tool call record
2. Query tool_calls table
3. Verify all fields stored

---

#### TC-1.1.089: Tool Call Timestamps

| Field | Value |
|-------|-------|
| **ID** | TC-1.1.089 |
| **Priority** | Medium |
| **Type** | Functional |

**Test Steps:**
1. Insert tool call
2. Verify created_at auto-populated

---

---

## 14. Test Data Setup

### 14.1 Seed Data Script

```sql
-- Cleanup
DELETE FROM fs_data;
DELETE FROM fs_journal WHERE inode != 1;
DELETE FROM kv_store;
DELETE FROM tool_calls;

-- Create directory structure
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES
    (2, 'create', 1, 'documents', 16877),
    (3, 'create', 1, 'images', 16877),
    (4, 'create', 2, 'readme.txt', 33188),
    (5, 'create', 2, 'config.json', 33188),
    (6, 'create', 3, 'photo.jpg', 33188);

-- Add some updates
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES
    (4, 'update', 2, 'readme.txt', 33188, 1024),
    (5, 'update', 2, 'config.json', 33188, 512);

-- Add fs_data chunks
INSERT INTO fs_data (inode, chunk_idx, data)
VALUES
    (4, 0, '\x48656C6C6F'::BLOB),  -- "Hello"
    (5, 0, '\x7B7D'::BLOB);         -- "{}"
```

### 14.2 Test Verification Queries

```sql
-- Verify seed data
SELECT COUNT(*) as event_count FROM fs_journal;
SELECT COUNT(*) as current_files FROM fs_current;
SELECT COUNT(*) as chunk_count FROM fs_data;

-- Expected:
-- event_count: 8 (1 root + 5 creates + 2 updates)
-- current_files: 6 (root + 2 dirs + 3 files)
-- chunk_count: 2
```

---

## 15. Test Environment Requirements

### 15.1 DuckDB Configuration

| Setting | Value | Notes |
|---------|-------|-------|
| Version | >= 0.9.0 | Minimum supported |
| Memory | 1GB+ | For performance tests |
| Threads | 4+ | For concurrency tests |

### 15.2 Test Tools

| Tool | Purpose |
|------|---------|
| pytest | Python test runner |
| pytest-duckdb | DuckDB test fixtures |
| asyncio | Concurrency testing |
| pytest-benchmark | Performance measurement |

---

## 16. Risk Mitigation Test Coverage

Based on QA Notes, specific tests for identified risks:

| Risk | Test Coverage |
|------|--------------|
| fs_current view performance | TC-1.1.080 - TC-1.1.084 |
| Concurrent race conditions | TC-1.1.062 - TC-1.1.067 |
| Delete logic complexity | TC-1.1.030 - TC-1.1.032 |
| Journal unbounded growth | TC-1.1.080 - TC-1.1.082 (baselines) |
| Orphaned fs_data | TC-1.1.077 - TC-1.1.078 |

---

## 17. Traceability Matrix

| Acceptance Criteria | Test Cases |
|--------------------|------------|
| fs_journal append-only | TC-1.1.001, TC-1.1.002, TC-1.1.013-025 |
| fs_current view | TC-1.1.003, TC-1.1.026-034 |
| fs_data chunks | TC-1.1.004, TC-1.1.035-042 |
| Sequences | TC-1.1.005, TC-1.1.043-048 |
| kv_store | TC-1.1.006, TC-1.1.085-087 |
| tool_calls | TC-1.1.007, TC-1.1.088-089 |
| Indexes | TC-1.1.008, TC-1.1.049-053 |
| Root directory | TC-1.1.009 |

---

## 18. Test Execution Summary Template

```
Test Execution Report: STORY-1.1
================================
Date: YYYY-MM-DD
Executor: [Name]
Environment: DuckDB [version]

Results:
--------
Total Tests: 89
Passed: __
Failed: __
Skipped: __
Blocked: __

Coverage: __%

Failed Tests:
- TC-1.1.XXX: [Reason]

Notes:
[Any observations]
```

---

## 19. Sign-Off

| Role | Name | Date | Signature |
|------|------|------|-----------|
| QA Engineer | | | |
| Tech Lead | | | |
| Product Owner | | | |

---

**Document Version History:**

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-01-14 | QA Agent | Initial test design |
