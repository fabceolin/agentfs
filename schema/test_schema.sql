-- ============================================================================
-- DuckAgentFS Schema Test Suite
-- ============================================================================
-- Tests all acceptance criteria for STORY-1.1

-- ============================================================================
-- SETUP: Load the schema
-- ============================================================================

-- Sequences first
CREATE SEQUENCE IF NOT EXISTS fs_event_seq START 1;
CREATE SEQUENCE IF NOT EXISTS fs_inode_seq START 2;
CREATE SEQUENCE IF NOT EXISTS audit_seq START 1;

-- Core tables
CREATE TABLE IF NOT EXISTS fs_journal (
    event_id    UBIGINT PRIMARY KEY DEFAULT nextval('fs_event_seq'),
    inode       UBIGINT NOT NULL,
    event_type  VARCHAR NOT NULL,
    event_time  TIMESTAMP DEFAULT current_timestamp,
    parent      UBIGINT,
    name        VARCHAR,
    mode        UINTEGER,
    uid         UINTEGER DEFAULT 0,
    gid         UINTEGER DEFAULT 0,
    size        UBIGINT DEFAULT 0,
    nlink       UINTEGER DEFAULT 1,
    xattrs      JSON,
    old_parent  UBIGINT,
    old_name    VARCHAR,
    actor_id    VARCHAR,
    session_id  VARCHAR,
    metadata    JSON
);

CREATE TABLE IF NOT EXISTS fs_data (
    inode       UBIGINT NOT NULL,
    chunk_idx   UINTEGER NOT NULL,
    data        BLOB NOT NULL,
    checksum    VARCHAR,
    created_at  TIMESTAMP DEFAULT current_timestamp,
    PRIMARY KEY (inode, chunk_idx)
);

CREATE TABLE IF NOT EXISTS kv_store (
    key         VARCHAR PRIMARY KEY,
    value       JSON NOT NULL,
    created_at  TIMESTAMP DEFAULT current_timestamp,
    updated_at  TIMESTAMP DEFAULT current_timestamp,
    expires_at  TIMESTAMP,
    version     UBIGINT DEFAULT 1,
    metadata    JSON
);

CREATE TABLE IF NOT EXISTS tool_calls (
    id              VARCHAR PRIMARY KEY,
    name            VARCHAR NOT NULL,
    status          VARCHAR NOT NULL DEFAULT 'running',
    started_at      TIMESTAMP NOT NULL,
    completed_at    TIMESTAMP,
    duration_ms     DOUBLE,
    parameters      JSON,
    result          JSON,
    error           VARCHAR,
    session_id      VARCHAR,
    parent_call_id  VARCHAR,
    metadata        JSON
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_fs_journal_inode ON fs_journal(inode);
CREATE INDEX IF NOT EXISTS idx_fs_journal_parent ON fs_journal(parent);
CREATE INDEX IF NOT EXISTS idx_fs_journal_name ON fs_journal(name);
CREATE INDEX IF NOT EXISTS idx_fs_journal_event_time ON fs_journal(event_time);

-- View for current state
CREATE OR REPLACE VIEW fs_current AS
WITH ranked AS (
    SELECT
        *,
        ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_type != 'delete'
)
SELECT
    inode,
    parent,
    name,
    mode,
    uid,
    gid,
    size,
    nlink,
    xattrs,
    event_time as mtime,
    event_id as last_event_id,
    actor_id,
    session_id
FROM ranked
WHERE rn = 1
  AND inode NOT IN (
      SELECT inode FROM fs_journal WHERE event_type = 'delete'
      AND event_id = (SELECT MAX(event_id) FROM fs_journal j2 WHERE j2.inode = fs_journal.inode)
  );

-- Initialize root directory
INSERT INTO fs_journal (inode, event_type, parent, name, mode, nlink)
SELECT 1, 'create', 1, '', 16877, 2
WHERE NOT EXISTS (SELECT 1 FROM fs_journal WHERE inode = 1);

-- ============================================================================
-- TEST 1: Verify fs_journal table exists with append-only model
-- ============================================================================
SELECT 'TEST 1: fs_journal table' as test_name;
SELECT COUNT(*) as column_count FROM information_schema.columns WHERE table_name = 'fs_journal';
-- Expected: 17 columns

-- ============================================================================
-- TEST 2: Verify fs_current view with ROW_NUMBER()
-- ============================================================================
SELECT 'TEST 2: fs_current view' as test_name;
SELECT COUNT(*) as view_exists FROM information_schema.tables WHERE table_name = 'fs_current' AND table_type = 'VIEW';
-- Expected: 1

-- ============================================================================
-- TEST 3: Verify fs_data table for binary chunks
-- ============================================================================
SELECT 'TEST 3: fs_data table' as test_name;
SELECT COUNT(*) as column_count FROM information_schema.columns WHERE table_name = 'fs_data';
-- Expected: 5 columns

-- ============================================================================
-- TEST 4: Verify sequences
-- ============================================================================
SELECT 'TEST 4: Sequences' as test_name;
SELECT nextval('fs_event_seq') as event_seq_works;
SELECT nextval('fs_inode_seq') as inode_seq_works;

-- ============================================================================
-- TEST 5: Verify kv_store table
-- ============================================================================
SELECT 'TEST 5: kv_store table' as test_name;
SELECT COUNT(*) as column_count FROM information_schema.columns WHERE table_name = 'kv_store';
-- Expected: 7 columns

-- ============================================================================
-- TEST 6: Verify tool_calls table
-- ============================================================================
SELECT 'TEST 6: tool_calls table' as test_name;
SELECT COUNT(*) as column_count FROM information_schema.columns WHERE table_name = 'tool_calls';
-- Expected: 12 columns

-- ============================================================================
-- TEST 7: Verify indexes
-- ============================================================================
SELECT 'TEST 7: Indexes' as test_name;
SELECT COUNT(*) as index_count FROM duckdb_indexes() WHERE table_name = 'fs_journal';
-- Expected: 4 indexes

-- ============================================================================
-- TEST 8: Verify root directory (inode 1)
-- ============================================================================
SELECT 'TEST 8: Root directory' as test_name;
SELECT * FROM fs_current WHERE inode = 1;
-- Expected: 1 row with mode 16877 (directory)

-- ============================================================================
-- FUNCTIONAL TEST 1: File Creation (use explicit inode 2 for predictable tests)
-- ============================================================================
SELECT 'FUNCTIONAL TEST 1: File Creation' as test_name;
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (2, 'create', 1, 'test.txt', 33188, 0);

SELECT inode, name, mode, size FROM fs_current WHERE name = 'test.txt';
-- Expected: 1 row with inode=2

-- ============================================================================
-- FUNCTIONAL TEST 2: File Update
-- ============================================================================
SELECT 'FUNCTIONAL TEST 2: File Update' as test_name;
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (2, 'update', 1, 'test.txt', 33188, 100);

SELECT size FROM fs_current WHERE inode = 2;
-- Expected: size = 100

-- ============================================================================
-- FUNCTIONAL TEST 3: File Delete
-- ============================================================================
SELECT 'FUNCTIONAL TEST 3: File Delete' as test_name;
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (2, 'delete', 1, 'test.txt', 33188);

SELECT COUNT(*) as deleted_file_count FROM fs_current WHERE inode = 2;
-- Expected: 0 rows (file deleted)

-- ============================================================================
-- FUNCTIONAL TEST 4: Time-Travel Query
-- ============================================================================
SELECT 'FUNCTIONAL TEST 4: Time-Travel' as test_name;
-- Get state at event_id 3 (after update, before delete)
WITH ranked AS (
    SELECT *,
           ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_id <= 3  -- Snapshot at event 3 (update event)
      AND event_type != 'delete'
)
SELECT inode, name, size FROM ranked WHERE rn = 1 AND inode = 2;
-- Expected: 1 row with test.txt, size=100 (state after update)

SELECT 'ALL TESTS COMPLETED' as status;
