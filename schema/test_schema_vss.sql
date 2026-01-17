-- ============================================================================
-- DuckAgentFS Schema VSS Test Suite
-- ============================================================================
-- Tests all acceptance criteria for STORY-2.2
-- Run with: duckdb :memory: < schema/test_schema_vss.sql
--
-- Prerequisites:
--   - DuckDB >= 0.9.0
--   - VSS extension (optional, for HNSW tests)
--
-- NOTE: This test file creates its own test tables with smaller dimensions
-- for practical testing. Production uses FLOAT[1536] for OpenAI ada-002.
-- ============================================================================

-- ============================================================================
-- TEST SCHEMA SETUP (smaller dimensions for testing)
-- ============================================================================

-- Create sequences
CREATE SEQUENCE IF NOT EXISTS fs_event_seq START 1;
CREATE SEQUENCE IF NOT EXISTS fs_inode_seq START 2;

-- Create minimal fs_journal for integration tests
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

-- Create fs_current view
-- Uses FIRST_VALUE IGNORE NULLS to coalesce values from all events,
-- taking the most recent non-NULL value for each column.
-- Note: Explicit frame clause required for IGNORE NULLS to scan entire partition.
CREATE OR REPLACE VIEW fs_current AS
WITH coalesced AS (
    SELECT
        inode,
        FIRST_VALUE(parent IGNORE NULLS) OVER w as parent,
        FIRST_VALUE(name IGNORE NULLS) OVER w as name,
        FIRST_VALUE(mode IGNORE NULLS) OVER w as mode,
        FIRST_VALUE(uid IGNORE NULLS) OVER w as uid,
        FIRST_VALUE(gid IGNORE NULLS) OVER w as gid,
        FIRST_VALUE(size IGNORE NULLS) OVER w as size,
        FIRST_VALUE(nlink IGNORE NULLS) OVER w as nlink,
        FIRST_VALUE(xattrs IGNORE NULLS) OVER w as xattrs,
        event_time as mtime,
        event_id as last_event_id,
        actor_id,
        session_id,
        ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
    FROM fs_journal
    WHERE event_type != 'delete'
    WINDOW w AS (PARTITION BY inode ORDER BY event_id DESC ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)
)
SELECT
    inode, parent, name, mode, uid, gid, size, nlink, xattrs,
    mtime, last_event_id, actor_id, session_id
FROM coalesced
WHERE rn = 1
  AND inode NOT IN (
      SELECT inode FROM fs_journal WHERE event_type = 'delete'
      AND event_id = (SELECT MAX(event_id) FROM fs_journal j2 WHERE j2.inode = fs_journal.inode)
  );

-- Create fs_tree view
CREATE OR REPLACE VIEW fs_tree AS
WITH RECURSIVE tree AS (
    SELECT inode, parent, name, mode, size, mtime, name as path, 0 as depth
    FROM fs_current WHERE inode = 1
    UNION ALL
    SELECT c.inode, c.parent, c.name, c.mode, c.size, c.mtime,
           CASE WHEN t.path = '' THEN c.name ELSE t.path || '/' || c.name END as path,
           t.depth + 1
    FROM fs_current c
    JOIN tree t ON c.parent = t.inode
    WHERE c.inode != 1
)
SELECT * FROM tree;

-- Initialize root directory
INSERT INTO fs_journal (inode, event_type, parent, name, mode, nlink)
SELECT 1, 'create', 1, '', 16877, 2
WHERE NOT EXISTS (SELECT 1 FROM fs_journal WHERE inode = 1);

-- Create test embeddings table with smaller dimension for practical testing
-- Production schema uses FLOAT[1536], but tests use FLOAT[8] for efficiency
CREATE TABLE IF NOT EXISTS fs_embeddings (
    inode           UBIGINT PRIMARY KEY,
    embedding       FLOAT[8],  -- Smaller dimension for testing
    model           VARCHAR NOT NULL DEFAULT 'text-embedding-ada-002',
    content_hash    VARCHAR,
    generated_at    TIMESTAMP DEFAULT current_timestamp,
    metadata        JSON
);

-- Create test chunk embeddings table
CREATE TABLE IF NOT EXISTS fs_chunk_embeddings (
    inode           UBIGINT NOT NULL,
    chunk_idx       UINTEGER NOT NULL,
    start_offset    UBIGINT NOT NULL,
    end_offset      UBIGINT NOT NULL,
    embedding       FLOAT[8],  -- Smaller dimension for testing
    content_preview VARCHAR(500),
    generated_at    TIMESTAMP DEFAULT current_timestamp,
    PRIMARY KEY (inode, chunk_idx)
);

-- ============================================================================
-- T1: SCHEMA VALIDATION TESTS
-- ============================================================================

SELECT '=== T1: Schema Validation Tests ===' as section;

-- T1.1: fs_embeddings table structure
SELECT 'T1.1: fs_embeddings table structure' as test_name;
SELECT
    CASE WHEN COUNT(*) = 6 THEN 'PASS' ELSE 'FAIL: expected 6 columns, got ' || COUNT(*) END as result
FROM information_schema.columns
WHERE table_name = 'fs_embeddings';

-- T1.2: fs_chunk_embeddings table structure
SELECT 'T1.2: fs_chunk_embeddings table structure' as test_name;
SELECT
    CASE WHEN COUNT(*) = 7 THEN 'PASS' ELSE 'FAIL: expected 7 columns, got ' || COUNT(*) END as result
FROM information_schema.columns
WHERE table_name = 'fs_chunk_embeddings';

-- T1.3: Verify column types for fs_embeddings
SELECT 'T1.3: fs_embeddings column types' as test_name;
WITH expected AS (
    SELECT 'inode' as col, 'UBIGINT' as dtype UNION ALL
    SELECT 'model', 'VARCHAR' UNION ALL
    SELECT 'content_hash', 'VARCHAR'
),
actual AS (
    SELECT column_name as col, data_type as dtype
    FROM information_schema.columns
    WHERE table_name = 'fs_embeddings'
)
SELECT
    CASE WHEN COUNT(*) = 3 THEN 'PASS' ELSE 'FAIL: type mismatch' END as result
FROM expected e
JOIN actual a ON e.col = a.col AND e.dtype = a.dtype;

-- T1.4: Default value for model column
SELECT 'T1.4: Default value for model' as test_name;
INSERT INTO fs_embeddings (inode, embedding)
VALUES (9003, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]::FLOAT[8]);
SELECT
    CASE WHEN model = 'text-embedding-ada-002' THEN 'PASS' ELSE 'FAIL: model=' || model END as result
FROM fs_embeddings WHERE inode = 9003;
DELETE FROM fs_embeddings WHERE inode = 9003;

-- T1.5: generated_at default timestamp
SELECT 'T1.5: generated_at default timestamp' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (9004, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]::FLOAT[8], 'test');
SELECT
    CASE WHEN generated_at IS NOT NULL THEN 'PASS' ELSE 'FAIL: generated_at is NULL' END as result
FROM fs_embeddings WHERE inode = 9004;
DELETE FROM fs_embeddings WHERE inode = 9004;

-- ============================================================================
-- T2: EMBEDDING INSERT/UPDATE TESTS
-- ============================================================================

SELECT '=== T2: Embedding Insert/Update Tests ===' as section;

-- T2.1: Insert valid embedding with all fields
SELECT 'T2.1: Insert valid embedding' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model, content_hash, metadata)
VALUES (
    10001,
    [0.1, 0.2, 0.3, 0.4, 0.5]::FLOAT[5],
    'test-model-v1',
    'abc123hash',
    '{"source": "test", "version": 1}'::JSON
);
SELECT
    CASE
        WHEN model = 'test-model-v1' AND content_hash = 'abc123hash' AND metadata->>'source' = 'test'
        THEN 'PASS'
        ELSE 'FAIL: field mismatch'
    END as result
FROM fs_embeddings WHERE inode = 10001;
DELETE FROM fs_embeddings WHERE inode = 10001;

-- T2.2: Update embedding on content change
SELECT 'T2.2: Update embedding on content change' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (10002, [0.1, 0.2, 0.3]::FLOAT[3], 'test', 'hash_v1');

UPDATE fs_embeddings
SET embedding = [0.4, 0.5, 0.6]::FLOAT[3],
    content_hash = 'hash_v2',
    generated_at = current_timestamp
WHERE inode = 10002 AND content_hash != 'hash_v2';

SELECT
    CASE
        WHEN content_hash = 'hash_v2' AND embedding[1] BETWEEN 0.39 AND 0.41
        THEN 'PASS'
        ELSE 'FAIL: update did not apply'
    END as result
FROM fs_embeddings WHERE inode = 10002;
DELETE FROM fs_embeddings WHERE inode = 10002;

-- T2.3: Content hash idempotency (no update when hash matches)
SELECT 'T2.3: Content hash idempotency' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (10003, [0.1, 0.2, 0.3]::FLOAT[3], 'test', 'same_hash');

-- Attempt update with same hash - condition should not match
UPDATE fs_embeddings
SET embedding = [0.9, 0.9, 0.9]::FLOAT[3]
WHERE inode = 10003 AND content_hash != 'same_hash';

SELECT
    CASE
        WHEN embedding[1] BETWEEN 0.09 AND 0.11
        THEN 'PASS'
        ELSE 'FAIL: embedding was modified'
    END as result
FROM fs_embeddings WHERE inode = 10003;
DELETE FROM fs_embeddings WHERE inode = 10003;

-- ============================================================================
-- T3: SIMILARITY SEARCH TESTS
-- ============================================================================

SELECT '=== T3: Similarity Search Tests ===' as section;

-- T3.1: Cosine similarity - identical vectors (should be 1.0)
SELECT 'T3.1: Cosine similarity - identical vectors' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (20001, [1.0, 0.0, 0.0]::FLOAT[3], 'test');

SELECT
    CASE
        WHEN ABS(array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3]) - 1.0) < 0.0001
        THEN 'PASS'
        ELSE 'FAIL: expected 1.0, got ' || array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3])::VARCHAR
    END as result
FROM fs_embeddings WHERE inode = 20001;
DELETE FROM fs_embeddings WHERE inode = 20001;

-- T3.2: Cosine similarity - orthogonal vectors (should be 0.0)
SELECT 'T3.2: Cosine similarity - orthogonal vectors' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (20002, [1.0, 0.0, 0.0]::FLOAT[3], 'test');

SELECT
    CASE
        WHEN ABS(array_cosine_similarity(embedding, [0.0, 1.0, 0.0]::FLOAT[3])) < 0.0001
        THEN 'PASS'
        ELSE 'FAIL: expected 0.0, got ' || array_cosine_similarity(embedding, [0.0, 1.0, 0.0]::FLOAT[3])::VARCHAR
    END as result
FROM fs_embeddings WHERE inode = 20002;
DELETE FROM fs_embeddings WHERE inode = 20002;

-- T3.3: Cosine similarity - opposite vectors (should be -1.0)
SELECT 'T3.3: Cosine similarity - opposite vectors' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (20003, [1.0, 0.0, 0.0]::FLOAT[3], 'test');

SELECT
    CASE
        WHEN ABS(array_cosine_similarity(embedding, [-1.0, 0.0, 0.0]::FLOAT[3]) + 1.0) < 0.0001
        THEN 'PASS'
        ELSE 'FAIL: expected -1.0, got ' || array_cosine_similarity(embedding, [-1.0, 0.0, 0.0]::FLOAT[3])::VARCHAR
    END as result
FROM fs_embeddings WHERE inode = 20003;
DELETE FROM fs_embeddings WHERE inode = 20003;

-- T3.4: Cosine similarity ordering
SELECT 'T3.4: Cosine similarity ordering' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (20010, [1.0, 0.0, 0.0]::FLOAT[3], 'test'),
    (20011, [0.9, 0.436, 0.0]::FLOAT[3], 'test'),
    (20012, [0.0, 1.0, 0.0]::FLOAT[3], 'test');

WITH ranked AS (
    SELECT
        inode,
        ROW_NUMBER() OVER (ORDER BY array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3]) DESC) as rank
    FROM fs_embeddings
    WHERE inode BETWEEN 20010 AND 20012
)
SELECT
    CASE
        WHEN (SELECT rank FROM ranked WHERE inode = 20010) = 1
         AND (SELECT rank FROM ranked WHERE inode = 20011) = 2
         AND (SELECT rank FROM ranked WHERE inode = 20012) = 3
        THEN 'PASS'
        ELSE 'FAIL: incorrect ordering'
    END as result;
DELETE FROM fs_embeddings WHERE inode BETWEEN 20010 AND 20012;

-- T3.5: Euclidean distance
SELECT 'T3.5: Euclidean distance' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (20020, [3.0, 4.0, 0.0]::FLOAT[3], 'test');

SELECT
    CASE
        WHEN ABS(array_distance(embedding, [0.0, 0.0, 0.0]::FLOAT[3]) - 5.0) < 0.0001
        THEN 'PASS'
        ELSE 'FAIL: expected 5.0, got ' || array_distance(embedding, [0.0, 0.0, 0.0]::FLOAT[3])::VARCHAR
    END as result
FROM fs_embeddings WHERE inode = 20020;
DELETE FROM fs_embeddings WHERE inode = 20020;

-- T3.6: Inner product
SELECT 'T3.6: Inner product' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (20030, [1.0, 2.0, 3.0]::FLOAT[3], 'test');

-- Inner product of [1,2,3] and [4,5,6] = 1*4 + 2*5 + 3*6 = 32
SELECT
    CASE
        WHEN ABS(array_inner_product(embedding, [4.0, 5.0, 6.0]::FLOAT[3]) - 32.0) < 0.0001
        THEN 'PASS'
        ELSE 'FAIL: expected 32.0, got ' || array_inner_product(embedding, [4.0, 5.0, 6.0]::FLOAT[3])::VARCHAR
    END as result
FROM fs_embeddings WHERE inode = 20030;
DELETE FROM fs_embeddings WHERE inode = 20030;

-- ============================================================================
-- T4: CHUNK EMBEDDING TESTS
-- ============================================================================

SELECT '=== T4: Chunk Embedding Tests ===' as section;

-- T4.1: Insert chunked embeddings
SELECT 'T4.1: Insert chunked embeddings' as test_name;
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding, content_preview) VALUES
    (30001, 0, 0, 1000, [0.1, 0.2]::FLOAT[2], 'First chunk of document...'),
    (30001, 1, 1000, 2000, [0.3, 0.4]::FLOAT[2], 'Second chunk continues...'),
    (30001, 2, 2000, 3000, [0.5, 0.6]::FLOAT[2], 'Third chunk finishes...');

SELECT
    CASE
        WHEN COUNT(*) = 3 AND MIN(chunk_idx) = 0 AND MAX(chunk_idx) = 2
        THEN 'PASS'
        ELSE 'FAIL: expected 3 chunks (0-2)'
    END as result
FROM fs_chunk_embeddings WHERE inode = 30001;
DELETE FROM fs_chunk_embeddings WHERE inode = 30001;

-- T4.2: Chunk index ordering (insert out of order, retrieve in order)
SELECT 'T4.2: Chunk index ordering' as test_name;
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding) VALUES
    (30002, 0, 0, 100, [0.1]::FLOAT[1]),
    (30002, 2, 200, 300, [0.3]::FLOAT[1]),
    (30002, 1, 100, 200, [0.2]::FLOAT[1]);

SELECT
    CASE
        WHEN STRING_AGG(chunk_idx::VARCHAR, ',' ORDER BY chunk_idx) = '0,1,2'
        THEN 'PASS'
        ELSE 'FAIL: chunks not in order'
    END as result
FROM fs_chunk_embeddings WHERE inode = 30002;
DELETE FROM fs_chunk_embeddings WHERE inode = 30002;

-- T4.3: Chunk offset continuity
SELECT 'T4.3: Chunk offset continuity' as test_name;
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding) VALUES
    (30003, 0, 0, 1000, [0.1]::FLOAT[1]),
    (30003, 1, 1000, 2000, [0.2]::FLOAT[1]),
    (30003, 2, 2000, 3000, [0.3]::FLOAT[1]);

WITH offset_check AS (
    SELECT
        chunk_idx,
        start_offset,
        end_offset,
        LAG(end_offset) OVER (ORDER BY chunk_idx) as prev_end
    FROM fs_chunk_embeddings
    WHERE inode = 30003
)
SELECT
    CASE
        WHEN COUNT(*) = 0
        THEN 'PASS'
        ELSE 'FAIL: offset discontinuity'
    END as result
FROM offset_check
WHERE prev_end IS NOT NULL AND start_offset != prev_end;
DELETE FROM fs_chunk_embeddings WHERE inode = 30003;

-- T4.4: Chunk-level similarity search
SELECT 'T4.4: Chunk-level similarity search' as test_name;
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding, content_preview) VALUES
    (30010, 0, 0, 500, [1.0, 0.0]::FLOAT[2], 'Machine learning intro'),
    (30010, 1, 500, 1000, [0.5, 0.5]::FLOAT[2], 'Neural networks'),
    (30011, 0, 0, 500, [0.0, 1.0]::FLOAT[2], 'Database systems');

WITH ranked AS (
    SELECT
        inode,
        chunk_idx,
        array_cosine_similarity(embedding, [1.0, 0.0]::FLOAT[2]) as score,
        ROW_NUMBER() OVER (ORDER BY array_cosine_similarity(embedding, [1.0, 0.0]::FLOAT[2]) DESC) as rank
    FROM fs_chunk_embeddings
    WHERE inode IN (30010, 30011)
)
SELECT
    CASE
        WHEN (SELECT inode FROM ranked WHERE rank = 1) = 30010
         AND (SELECT chunk_idx FROM ranked WHERE rank = 1) = 0
        THEN 'PASS'
        ELSE 'FAIL: wrong top result'
    END as result;
DELETE FROM fs_chunk_embeddings WHERE inode IN (30010, 30011);

-- T4.5: Content preview populated
SELECT 'T4.5: Content preview populated' as test_name;
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding, content_preview)
VALUES (30020, 0, 0, 100, [0.1]::FLOAT[1], 'Preview text for context');

SELECT
    CASE
        WHEN content_preview = 'Preview text for context'
        THEN 'PASS'
        ELSE 'FAIL: content_preview mismatch'
    END as result
FROM fs_chunk_embeddings WHERE inode = 30020;
DELETE FROM fs_chunk_embeddings WHERE inode = 30020;

-- ============================================================================
-- T5: INTEGRATION TESTS
-- ============================================================================

SELECT '=== T5: Integration Tests ===' as section;

-- T5.1: JOIN with fs_tree for path resolution
SELECT 'T5.1: JOIN with fs_tree for path' as test_name;
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (40001, 'create', 1, 'test_doc.txt', 33188, 1024);
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (40001, [0.5, 0.5, 0.0]::FLOAT[3], 'test');

SELECT
    CASE
        WHEN t.path = 'test_doc.txt' AND e.model = 'test'
        THEN 'PASS'
        ELSE 'FAIL: path or model mismatch'
    END as result
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE e.inode = 40001;

DELETE FROM fs_embeddings WHERE inode = 40001;
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40001, 'delete', 1, 'test_doc.txt', 33188);

-- T5.2: Filter regular files only using mode bitmask
SELECT 'T5.2: Filter regular files only' as test_name;
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40010, 'create', 1, 'test_dir', 16877);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40011, 'create', 40010, 'readme.txt', 33188);
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (40010, [0.1]::FLOAT[1], 'test'),
    (40011, [0.2]::FLOAT[1], 'test');

SELECT
    CASE
        WHEN COUNT(*) = 1 AND MIN(t.name) = 'readme.txt'
        THEN 'PASS'
        ELSE 'FAIL: directory should be filtered out'
    END as result
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE (t.mode & 61440) = 32768
  AND e.inode IN (40010, 40011);

DELETE FROM fs_embeddings WHERE inode IN (40010, 40011);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40011, 'delete', 40010, 'readme.txt', 33188);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40010, 'delete', 1, 'test_dir', 16877);

-- T5.3: LEFT JOIN for missing embeddings
SELECT 'T5.3: LEFT JOIN for missing embeddings' as test_name;
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40020, 'create', 1, 'indexed.txt', 33188);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40021, 'create', 1, 'not_indexed.txt', 33188);
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (40020, [0.5]::FLOAT[1], 'test');

WITH results AS (
    SELECT
        t.name,
        CASE WHEN e.inode IS NULL THEN 0 ELSE 1 END as has_embedding
    FROM fs_tree t
    LEFT JOIN fs_embeddings e ON t.inode = e.inode
    WHERE t.inode IN (40020, 40021)
)
SELECT
    CASE
        WHEN (SELECT has_embedding FROM results WHERE name = 'indexed.txt') = 1
         AND (SELECT has_embedding FROM results WHERE name = 'not_indexed.txt') = 0
        THEN 'PASS'
        ELSE 'FAIL: embedding presence mismatch'
    END as result;

DELETE FROM fs_embeddings WHERE inode = 40020;
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40020, 'delete', 1, 'indexed.txt', 33188);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40021, 'delete', 1, 'not_indexed.txt', 33188);

-- ============================================================================
-- T6: EDGE CASES AND ERROR HANDLING
-- ============================================================================

SELECT '=== T6: Edge Cases ===' as section;

-- T6.1: NULL embedding handling
SELECT 'T6.1: NULL embedding handling' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (50001, NULL, 'none', 'binary_file_hash');

SELECT
    CASE
        WHEN embedding IS NULL AND model = 'none'
        THEN 'PASS'
        ELSE 'FAIL: NULL embedding not stored correctly'
    END as result
FROM fs_embeddings WHERE inode = 50001;
DELETE FROM fs_embeddings WHERE inode = 50001;

-- T6.2: Orphan embedding (inode not in fs_tree)
SELECT 'T6.2: Orphan embedding visibility' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (99999, [0.1]::FLOAT[1], 'test');

SELECT
    CASE
        WHEN COUNT(*) = 0
        THEN 'PASS'
        ELSE 'FAIL: orphan visible in JOIN'
    END as result
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE e.inode = 99999;
DELETE FROM fs_embeddings WHERE inode = 99999;

-- T6.3: Metadata JSON storage
SELECT 'T6.3: Metadata JSON storage' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model, metadata)
VALUES (50003, [0.1]::FLOAT[1], 'test', '{"key": "value", "nested": {"a": 1}}'::JSON);

SELECT
    CASE
        WHEN metadata->>'key' = 'value' AND (metadata->'nested'->>'a')::INT = 1
        THEN 'PASS'
        ELSE 'FAIL: JSON not stored/retrieved correctly'
    END as result
FROM fs_embeddings WHERE inode = 50003;
DELETE FROM fs_embeddings WHERE inode = 50003;

-- T6.4: Different embedding dimensions work
SELECT 'T6.4: Variable embedding dimensions' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (50010, [0.1]::FLOAT[1], 'dim-1'),
    (50011, [0.1, 0.2]::FLOAT[2], 'dim-2'),
    (50012, [0.1, 0.2, 0.3]::FLOAT[3], 'dim-3');

SELECT
    CASE
        WHEN COUNT(DISTINCT list_value(embedding)) = 3
        THEN 'PASS'
        ELSE 'FAIL: dimension variance not supported'
    END as result
FROM fs_embeddings WHERE inode BETWEEN 50010 AND 50012;
DELETE FROM fs_embeddings WHERE inode BETWEEN 50010 AND 50012;

-- ============================================================================
-- T7: MODEL TRACKING TESTS
-- ============================================================================

SELECT '=== T7: Model Tracking Tests ===' as section;

-- T7.1: Multiple models in same table
SELECT 'T7.1: Multiple models' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (60001, [0.1]::FLOAT[1], 'text-embedding-ada-002'),
    (60002, [0.2]::FLOAT[1], 'text-embedding-3-small'),
    (60003, [0.3]::FLOAT[1], 'all-MiniLM-L6-v2');

SELECT
    CASE
        WHEN COUNT(DISTINCT model) = 3
        THEN 'PASS'
        ELSE 'FAIL: model diversity not preserved'
    END as result
FROM fs_embeddings WHERE inode BETWEEN 60001 AND 60003;
DELETE FROM fs_embeddings WHERE inode BETWEEN 60001 AND 60003;

-- T7.2: Query by model
SELECT 'T7.2: Query by model' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (60010, [0.1]::FLOAT[1], 'ada'),
    (60011, [0.2]::FLOAT[1], 'ada'),
    (60012, [0.3]::FLOAT[1], 'bert');

SELECT
    CASE
        WHEN COUNT(*) = 2
        THEN 'PASS'
        ELSE 'FAIL: model filter not working'
    END as result
FROM fs_embeddings
WHERE model = 'ada' AND inode BETWEEN 60010 AND 60012;
DELETE FROM fs_embeddings WHERE inode BETWEEN 60010 AND 60012;

-- ============================================================================
-- SUMMARY
-- ============================================================================

SELECT '=== TEST SUITE COMPLETE ===' as status;
SELECT 'Run "grep FAIL" on output to find failures' as hint;
