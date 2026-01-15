# STORY-2.2: Schema VSS - Test Design Document

## Test Design Metadata

| Field | Value |
|-------|-------|
| **Story ID** | STORY-2.2 |
| **Story Title** | Schema VSS |
| **Test Design Version** | 1.0 |
| **Created** | 2026-01-14 |
| **Author** | QA Agent |
| **Schema File** | `schema/duckagentfs.sql` |
| **Test File** | `schema/test_schema_vss.sql` |

## Test Objectives

1. Verify DDL for `fs_embeddings` and `fs_chunk_embeddings` tables
2. Validate HNSW index definitions (when VSS extension available)
3. Test similarity search functions (cosine, euclidean, inner product)
4. Validate model tracking and content hash change detection
5. Test chunk embedding operations for large files
6. Verify integration with `fs_tree` view for path-based queries

## Test Environment Requirements

| Requirement | Status | Notes |
|-------------|--------|-------|
| DuckDB >= 0.9.0 | Required | Core database |
| VSS Extension | Optional | Required for HNSW index tests |
| `schema/duckagentfs.sql` loaded | Required | Base schema dependency |

## Test Categories

### Category 1: Schema Validation Tests (T1.x)

#### T1.1: Verify fs_embeddings Table Creation

**Objective:** Confirm `fs_embeddings` table exists with correct schema

```sql
-- Test: fs_embeddings table structure
SELECT 'T1.1: fs_embeddings table structure' as test_name;

SELECT
    column_name,
    data_type
FROM information_schema.columns
WHERE table_name = 'fs_embeddings'
ORDER BY ordinal_position;

-- Expected columns:
-- inode       | UBIGINT
-- embedding   | FLOAT[]
-- model       | VARCHAR
-- content_hash| VARCHAR
-- generated_at| TIMESTAMP
-- metadata    | JSON
```

**Expected:** 6 columns with correct types

**Verification:**
```sql
SELECT
    COUNT(*) = 6 as column_count_ok,
    SUM(CASE WHEN column_name = 'inode' AND data_type = 'UBIGINT' THEN 1 ELSE 0 END) = 1 as inode_ok,
    SUM(CASE WHEN column_name = 'embedding' THEN 1 ELSE 0 END) = 1 as embedding_ok,
    SUM(CASE WHEN column_name = 'model' AND data_type = 'VARCHAR' THEN 1 ELSE 0 END) = 1 as model_ok,
    SUM(CASE WHEN column_name = 'content_hash' AND data_type = 'VARCHAR' THEN 1 ELSE 0 END) = 1 as hash_ok
FROM information_schema.columns
WHERE table_name = 'fs_embeddings';
```

---

#### T1.2: Verify fs_chunk_embeddings Table Creation

**Objective:** Confirm `fs_chunk_embeddings` table exists with correct schema and composite primary key

```sql
-- Test: fs_chunk_embeddings table structure
SELECT 'T1.2: fs_chunk_embeddings table structure' as test_name;

SELECT
    column_name,
    data_type
FROM information_schema.columns
WHERE table_name = 'fs_chunk_embeddings'
ORDER BY ordinal_position;

-- Expected columns:
-- inode           | UBIGINT
-- chunk_idx       | UINTEGER
-- start_offset    | UBIGINT
-- end_offset      | UBIGINT
-- embedding       | FLOAT[]
-- content_preview | VARCHAR
-- generated_at    | TIMESTAMP
```

**Expected:** 7 columns with correct types

---

#### T1.3: Verify Primary Key Constraints

**Objective:** Confirm primary key on `fs_embeddings(inode)` and composite PK on `fs_chunk_embeddings(inode, chunk_idx)`

```sql
-- Test: Primary key constraints
SELECT 'T1.3: Primary key constraints' as test_name;

-- fs_embeddings: should reject duplicate inode
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (9001, [0.1, 0.2, 0.3]::FLOAT[3], 'test');

INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (9001, [0.4, 0.5, 0.6]::FLOAT[3], 'test');
-- Expected: Error - duplicate primary key

-- fs_chunk_embeddings: composite PK test
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (9002, 0, 0, 1000, [0.1]::FLOAT[1]);

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (9002, 0, 0, 1000, [0.2]::FLOAT[1]);
-- Expected: Error - duplicate primary key

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 9001;
DELETE FROM fs_chunk_embeddings WHERE inode = 9002;
```

---

#### T1.4: Verify Default Values

**Objective:** Confirm default values for model, generated_at

```sql
-- Test: Default values
SELECT 'T1.4: Default values' as test_name;

INSERT INTO fs_embeddings (inode, embedding)
VALUES (9003, [0.1, 0.2, 0.3]::FLOAT[3]);

SELECT
    model = 'text-embedding-ada-002' as model_default_ok,
    generated_at IS NOT NULL as timestamp_default_ok
FROM fs_embeddings
WHERE inode = 9003;
-- Expected: TRUE, TRUE

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 9003;
```

---

### Category 2: Embedding Insert/Update Tests (T2.x)

#### T2.1: Insert Valid Embedding

**Objective:** Successfully insert embedding with all fields

```sql
-- Test: Insert valid embedding
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
    inode,
    model,
    content_hash,
    metadata->>'source' as source
FROM fs_embeddings
WHERE inode = 10001;
-- Expected: 1 row with correct values

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 10001;
```

---

#### T2.2: Update Embedding on Content Change

**Objective:** Verify embedding update workflow

```sql
-- Test: Update embedding on content change
SELECT 'T2.2: Update embedding on content change' as test_name;

-- Initial insert
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (10002, [0.1, 0.2, 0.3]::FLOAT[3], 'test', 'hash_v1');

-- Simulate update
UPDATE fs_embeddings
SET
    embedding = [0.4, 0.5, 0.6]::FLOAT[3],
    content_hash = 'hash_v2',
    generated_at = current_timestamp
WHERE inode = 10002 AND content_hash != 'hash_v2';

SELECT
    content_hash = 'hash_v2' as hash_updated,
    embedding[1] = 0.4 as embedding_updated
FROM fs_embeddings
WHERE inode = 10002;
-- Expected: TRUE, TRUE

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 10002;
```

---

#### T2.3: Content Hash Idempotency

**Objective:** No update when hash matches (idempotent operation)

```sql
-- Test: Content hash idempotency
SELECT 'T2.3: Content hash idempotency' as test_name;

-- Insert with hash
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (10003, [0.1, 0.2, 0.3]::FLOAT[3], 'test', 'same_hash');

-- Get initial timestamp
SELECT generated_at as initial_time FROM fs_embeddings WHERE inode = 10003;

-- Wait a moment (in real test, use pg_sleep or similar)
-- Attempt update with same hash - should not match condition
UPDATE fs_embeddings
SET
    embedding = [0.9, 0.9, 0.9]::FLOAT[3],
    generated_at = current_timestamp
WHERE inode = 10003 AND content_hash != 'same_hash';

-- Verify no change (embedding should still be original)
SELECT
    embedding[1] = 0.1 as unchanged
FROM fs_embeddings
WHERE inode = 10003;
-- Expected: TRUE (embedding unchanged)

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 10003;
```

---

### Category 3: Similarity Search Tests (T3.x)

#### T3.1: Cosine Similarity - Identical Vectors

**Objective:** Identical vectors have cosine similarity of 1.0

```sql
-- Test: Cosine similarity - identical vectors
SELECT 'T3.1: Cosine similarity - identical' as test_name;

-- Setup test embeddings
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (20001, [1.0, 0.0, 0.0]::FLOAT[3], 'test');

SELECT
    array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3]) as similarity
FROM fs_embeddings
WHERE inode = 20001;
-- Expected: 1.0

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 20001;
```

---

#### T3.2: Cosine Similarity - Orthogonal Vectors

**Objective:** Orthogonal vectors have cosine similarity of 0.0

```sql
-- Test: Cosine similarity - orthogonal vectors
SELECT 'T3.2: Cosine similarity - orthogonal' as test_name;

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (20002, [1.0, 0.0, 0.0]::FLOAT[3], 'test');

SELECT
    array_cosine_similarity(embedding, [0.0, 1.0, 0.0]::FLOAT[3]) as similarity
FROM fs_embeddings
WHERE inode = 20002;
-- Expected: 0.0

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 20002;
```

---

#### T3.3: Cosine Similarity - Ordering

**Objective:** Similarity search returns results in correct order

```sql
-- Test: Cosine similarity ordering
SELECT 'T3.3: Cosine similarity ordering' as test_name;

-- Insert test data with varying similarity to [1,0,0]
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (20010, [1.0, 0.0, 0.0]::FLOAT[3], 'test'),     -- identical
    (20011, [0.9, 0.1, 0.0]::FLOAT[3], 'test'),     -- very similar
    (20012, [0.7, 0.3, 0.0]::FLOAT[3], 'test'),     -- somewhat similar
    (20013, [0.0, 1.0, 0.0]::FLOAT[3], 'test'),     -- orthogonal
    (20014, [-1.0, 0.0, 0.0]::FLOAT[3], 'test');    -- opposite

-- Query with ordering
SELECT
    inode,
    array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3]) as sim
FROM fs_embeddings
WHERE inode BETWEEN 20010 AND 20014
ORDER BY sim DESC;

-- Expected order: 20010 (1.0), 20011 (~0.99), 20012 (~0.92), 20013 (0.0), 20014 (-1.0)

-- Cleanup
DELETE FROM fs_embeddings WHERE inode BETWEEN 20010 AND 20014;
```

---

#### T3.4: Euclidean Distance

**Objective:** Verify euclidean distance calculation

```sql
-- Test: Euclidean distance
SELECT 'T3.4: Euclidean distance' as test_name;

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (20020, [0.0, 0.0, 0.0]::FLOAT[3], 'test'),     -- origin
    (20021, [3.0, 4.0, 0.0]::FLOAT[3], 'test');     -- distance 5 from origin

-- Distance from [0,0,0] to [3,4,0] should be 5.0
SELECT
    array_distance(
        (SELECT embedding FROM fs_embeddings WHERE inode = 20021),
        [0.0, 0.0, 0.0]::FLOAT[3]
    ) as distance;
-- Expected: 5.0

-- Cleanup
DELETE FROM fs_embeddings WHERE inode BETWEEN 20020 AND 20021;
```

---

#### T3.5: Inner Product

**Objective:** Verify inner product calculation

```sql
-- Test: Inner product
SELECT 'T3.5: Inner product' as test_name;

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (20030, [1.0, 2.0, 3.0]::FLOAT[3], 'test');

-- Inner product of [1,2,3] and [4,5,6] = 1*4 + 2*5 + 3*6 = 32
SELECT
    array_inner_product(embedding, [4.0, 5.0, 6.0]::FLOAT[3]) as inner_prod
FROM fs_embeddings
WHERE inode = 20030;
-- Expected: 32.0

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 20030;
```

---

### Category 4: Chunk Embedding Tests (T4.x)

#### T4.1: Insert Chunked Embeddings

**Objective:** Successfully insert multiple chunks for one file

```sql
-- Test: Insert chunked embeddings
SELECT 'T4.1: Insert chunked embeddings' as test_name;

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding, content_preview) VALUES
    (30001, 0, 0, 1000, [0.1, 0.2]::FLOAT[2], 'First chunk of document...'),
    (30001, 1, 1000, 2000, [0.3, 0.4]::FLOAT[2], 'Second chunk continues...'),
    (30001, 2, 2000, 3000, [0.5, 0.6]::FLOAT[2], 'Third chunk finishes...');

SELECT
    COUNT(*) as chunk_count,
    MIN(chunk_idx) as first_chunk,
    MAX(chunk_idx) as last_chunk
FROM fs_chunk_embeddings
WHERE inode = 30001;
-- Expected: 3, 0, 2

-- Cleanup
DELETE FROM fs_chunk_embeddings WHERE inode = 30001;
```

---

#### T4.2: Chunk Index Ordering

**Objective:** Verify chunks maintain correct order

```sql
-- Test: Chunk index ordering
SELECT 'T4.2: Chunk index ordering' as test_name;

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding) VALUES
    (30002, 0, 0, 100, [0.1]::FLOAT[1]),
    (30002, 2, 200, 300, [0.3]::FLOAT[1]),  -- Insert out of order
    (30002, 1, 100, 200, [0.2]::FLOAT[1]);

SELECT
    chunk_idx,
    start_offset,
    end_offset
FROM fs_chunk_embeddings
WHERE inode = 30002
ORDER BY chunk_idx;
-- Expected: 0,0,100 | 1,100,200 | 2,200,300

-- Cleanup
DELETE FROM fs_chunk_embeddings WHERE inode = 30002;
```

---

#### T4.3: Chunk-Level Similarity Search

**Objective:** Search specific passages within files

```sql
-- Test: Chunk-level similarity search
SELECT 'T4.3: Chunk-level similarity search' as test_name;

-- Insert chunks from multiple files
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding, content_preview) VALUES
    (30010, 0, 0, 500, [1.0, 0.0]::FLOAT[2], 'Machine learning intro'),
    (30010, 1, 500, 1000, [0.5, 0.5]::FLOAT[2], 'Neural networks'),
    (30011, 0, 0, 500, [0.0, 1.0]::FLOAT[2], 'Database systems'),
    (30011, 1, 500, 1000, [0.8, 0.2]::FLOAT[2], 'Query optimization');

-- Search for chunks similar to [1.0, 0.0] (ML topic)
SELECT
    inode,
    chunk_idx,
    content_preview,
    array_cosine_similarity(embedding, [1.0, 0.0]::FLOAT[2]) as score
FROM fs_chunk_embeddings
WHERE inode IN (30010, 30011)
ORDER BY score DESC
LIMIT 2;
-- Expected: First result is 30010 chunk 0 (score 1.0)

-- Cleanup
DELETE FROM fs_chunk_embeddings WHERE inode IN (30010, 30011);
```

---

#### T4.4: Content Preview Length Constraint

**Objective:** Verify content_preview respects VARCHAR(500) limit

```sql
-- Test: Content preview length constraint
SELECT 'T4.4: Content preview length' as test_name;

-- Create a string longer than 500 chars
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding, content_preview)
VALUES (
    30020,
    0,
    0,
    1000,
    [0.1]::FLOAT[1],
    REPEAT('x', 600)  -- 600 chars, exceeds 500
);
-- Expected: Truncation or error depending on DuckDB behavior

SELECT LENGTH(content_preview) as preview_length
FROM fs_chunk_embeddings
WHERE inode = 30020;

-- Cleanup
DELETE FROM fs_chunk_embeddings WHERE inode = 30020;
```

---

### Category 5: Integration Tests (T5.x)

#### T5.1: JOIN with fs_tree for Path Resolution

**Objective:** Retrieve file paths with their embedding similarity scores

```sql
-- Test: JOIN with fs_tree for path resolution
SELECT 'T5.1: JOIN with fs_tree' as test_name;

-- Setup: Create a file in filesystem first
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (40001, 'create', 1, 'document.txt', 33188, 1024);

-- Add embedding for this file
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (40001, [0.5, 0.5, 0.0]::FLOAT[3], 'test');

-- Query with path
SELECT
    t.path,
    e.model,
    array_cosine_similarity(e.embedding, [1.0, 0.0, 0.0]::FLOAT[3]) as score
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE e.inode = 40001;
-- Expected: path = 'document.txt', model = 'test', score ~ 0.707

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 40001;
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40001, 'delete', 1, 'document.txt', 33188);
```

---

#### T5.2: Filter Regular Files Only

**Objective:** Search embeddings for regular files only using mode bitmask

```sql
-- Test: Filter regular files only
SELECT 'T5.2: Filter regular files only' as test_name;

-- Create a directory and a file
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40010, 'create', 1, 'docs', 16877);  -- Directory mode

INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40011, 'create', 40010, 'readme.txt', 33188);  -- File mode

-- Add embeddings for both (shouldn't happen for directories, but test filter)
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (40010, [0.1, 0.1]::FLOAT[2], 'test'),
    (40011, [0.9, 0.1]::FLOAT[2], 'test');

-- Query with file type filter
SELECT
    t.path,
    t.mode
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE (t.mode & 61440) = 32768  -- Regular files only (S_IFREG)
  AND e.inode IN (40010, 40011);
-- Expected: Only readme.txt (mode 33188 is regular file)

-- Cleanup
DELETE FROM fs_embeddings WHERE inode IN (40010, 40011);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40011, 'delete', 40010, 'readme.txt', 33188);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40010, 'delete', 1, 'docs', 16877);
```

---

#### T5.3: LEFT JOIN for Missing Embeddings

**Objective:** Handle files without embeddings gracefully

```sql
-- Test: LEFT JOIN for missing embeddings
SELECT 'T5.3: LEFT JOIN for missing embeddings' as test_name;

-- Create files
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40020, 'create', 1, 'indexed.txt', 33188);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40021, 'create', 1, 'not_indexed.txt', 33188);

-- Add embedding only for one
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (40020, [0.5, 0.5]::FLOAT[2], 'test');

-- Query with LEFT JOIN
SELECT
    t.path,
    CASE WHEN e.inode IS NULL THEN 'no_embedding' ELSE 'has_embedding' END as status
FROM fs_tree t
LEFT JOIN fs_embeddings e ON t.inode = e.inode
WHERE t.inode IN (40020, 40021);
-- Expected: indexed.txt -> has_embedding, not_indexed.txt -> no_embedding

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 40020;
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40020, 'delete', 1, 'indexed.txt', 33188);
INSERT INTO fs_journal (inode, event_type, parent, name, mode)
VALUES (40021, 'delete', 1, 'not_indexed.txt', 33188);
```

---

### Category 6: Edge Cases and Error Handling (T6.x)

#### T6.1: NULL Embedding Handling

**Objective:** Verify NULL embeddings are handled correctly

```sql
-- Test: NULL embedding handling
SELECT 'T6.1: NULL embedding handling' as test_name;

-- Insert with NULL embedding (binary file, no text)
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (50001, NULL, 'none', 'binary_file_hash');

SELECT
    inode,
    embedding IS NULL as is_null
FROM fs_embeddings
WHERE inode = 50001;
-- Expected: TRUE

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 50001;
```

---

#### T6.2: Empty Embedding Array

**Objective:** Test behavior with empty embedding array

```sql
-- Test: Empty embedding array
SELECT 'T6.2: Empty embedding array' as test_name;

-- Try to insert empty array
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (50002, []::FLOAT[0], 'test');
-- May or may not succeed depending on DuckDB validation

SELECT COUNT(*) FROM fs_embeddings WHERE inode = 50002;

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 50002;
```

---

#### T6.3: Large Embedding Dimension

**Objective:** Test with production-size embeddings (1536 dims)

```sql
-- Test: Large embedding dimension
SELECT 'T6.3: Large embedding dimension (1536)' as test_name;

-- Generate a 1536-dim embedding (sparse for simplicity)
INSERT INTO fs_embeddings (inode, embedding, model)
SELECT
    50003,
    (SELECT list_apply(generate_series(1, 1536), x -> x::FLOAT / 1536.0)),
    'text-embedding-ada-002';

SELECT
    LENGTH(embedding::VARCHAR) > 0 as has_embedding,
    list_element(embedding, 0) as first_elem,
    list_element(embedding, 1535) as last_elem
FROM fs_embeddings
WHERE inode = 50003;
-- Expected: TRUE, ~0.00065, 1.0

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 50003;
```

---

#### T6.4: Orphan Embedding (No FK Constraint)

**Objective:** Test behavior when inode doesn't exist in fs_tree

```sql
-- Test: Orphan embedding
SELECT 'T6.4: Orphan embedding' as test_name;

-- Insert embedding for non-existent inode
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (99999, [0.1, 0.2]::FLOAT[2], 'test');
-- Expected: Succeeds (no FK constraint)

-- Query with JOIN should return nothing
SELECT COUNT(*) as orphan_visible
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE e.inode = 99999;
-- Expected: 0

-- Cleanup
DELETE FROM fs_embeddings WHERE inode = 99999;
```

---

### Category 7: VSS Extension Tests (T7.x)

> **Note:** These tests require the VSS extension. Skip if unavailable.

#### T7.1: HNSW Index Creation

**Objective:** Verify HNSW index can be created

```sql
-- Test: HNSW index creation (requires VSS extension)
SELECT 'T7.1: HNSW index creation' as test_name;

-- Check if VSS extension is available
INSTALL vss;
LOAD vss;

-- Create HNSW index
CREATE INDEX IF NOT EXISTS idx_fs_embeddings_hnsw
ON fs_embeddings USING HNSW (embedding)
WITH (metric = 'cosine');

-- Verify index exists
SELECT COUNT(*) as index_exists
FROM duckdb_indexes()
WHERE table_name = 'fs_embeddings'
  AND index_name = 'idx_fs_embeddings_hnsw';
-- Expected: 1
```

---

#### T7.2: HNSW Accelerated Search

**Objective:** Verify HNSW index accelerates similarity search

```sql
-- Test: HNSW accelerated search
SELECT 'T7.2: HNSW accelerated search' as test_name;

-- Insert many embeddings
INSERT INTO fs_embeddings (inode, embedding, model)
SELECT
    60000 + i,
    (SELECT list_apply(generate_series(1, 3), x -> random()::FLOAT)),
    'test'
FROM generate_series(1, 1000) t(i);

-- Time a similarity search (with index should be faster)
SELECT
    inode,
    array_cosine_similarity(embedding, [0.5, 0.5, 0.5]::FLOAT[3]) as sim
FROM fs_embeddings
WHERE inode BETWEEN 60001 AND 61000
ORDER BY sim DESC
LIMIT 10;

-- Cleanup
DELETE FROM fs_embeddings WHERE inode BETWEEN 60001 AND 61000;
```

---

## Test Execution Script

```sql
-- ============================================================================
-- DuckAgentFS Schema VSS Test Suite
-- ============================================================================
-- Tests all acceptance criteria for STORY-2.2
-- Run with: duckdb database.db < test_schema_vss.sql

-- Load base schema first
-- .read schema/duckagentfs.sql

-- ============================================================================
-- T1: Schema Validation Tests
-- ============================================================================

SELECT '=== T1: Schema Validation Tests ===' as section;

-- T1.1: fs_embeddings structure
SELECT 'T1.1: fs_embeddings table structure' as test_name;
SELECT COUNT(*) as column_count FROM information_schema.columns WHERE table_name = 'fs_embeddings';
-- Expected: 6

-- T1.2: fs_chunk_embeddings structure
SELECT 'T1.2: fs_chunk_embeddings table structure' as test_name;
SELECT COUNT(*) as column_count FROM information_schema.columns WHERE table_name = 'fs_chunk_embeddings';
-- Expected: 7

-- T1.4: Default values
SELECT 'T1.4: Default values' as test_name;
INSERT INTO fs_embeddings (inode, embedding) VALUES (9003, [0.1, 0.2, 0.3]::FLOAT[3]);
SELECT model = 'text-embedding-ada-002' as model_default_ok FROM fs_embeddings WHERE inode = 9003;
DELETE FROM fs_embeddings WHERE inode = 9003;

-- ============================================================================
-- T2: Embedding Insert/Update Tests
-- ============================================================================

SELECT '=== T2: Embedding Insert/Update Tests ===' as section;

-- T2.1: Insert valid embedding
SELECT 'T2.1: Insert valid embedding' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model, content_hash) VALUES (10001, [0.1, 0.2, 0.3]::FLOAT[3], 'test', 'hash1');
SELECT COUNT(*) = 1 as insert_ok FROM fs_embeddings WHERE inode = 10001;
DELETE FROM fs_embeddings WHERE inode = 10001;

-- T2.2: Update embedding
SELECT 'T2.2: Update embedding on content change' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model, content_hash) VALUES (10002, [0.1]::FLOAT[1], 'test', 'hash_v1');
UPDATE fs_embeddings SET embedding = [0.9]::FLOAT[1], content_hash = 'hash_v2' WHERE inode = 10002;
SELECT content_hash = 'hash_v2' as update_ok FROM fs_embeddings WHERE inode = 10002;
DELETE FROM fs_embeddings WHERE inode = 10002;

-- ============================================================================
-- T3: Similarity Search Tests
-- ============================================================================

SELECT '=== T3: Similarity Search Tests ===' as section;

-- T3.1: Cosine similarity - identical
SELECT 'T3.1: Cosine similarity - identical' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES (20001, [1.0, 0.0, 0.0]::FLOAT[3], 'test');
SELECT array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3]) = 1.0 as identical_ok FROM fs_embeddings WHERE inode = 20001;
DELETE FROM fs_embeddings WHERE inode = 20001;

-- T3.2: Cosine similarity - orthogonal
SELECT 'T3.2: Cosine similarity - orthogonal' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES (20002, [1.0, 0.0, 0.0]::FLOAT[3], 'test');
SELECT array_cosine_similarity(embedding, [0.0, 1.0, 0.0]::FLOAT[3]) = 0.0 as orthogonal_ok FROM fs_embeddings WHERE inode = 20002;
DELETE FROM fs_embeddings WHERE inode = 20002;

-- T3.3: Cosine similarity ordering
SELECT 'T3.3: Cosine similarity ordering' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (20010, [1.0, 0.0, 0.0]::FLOAT[3], 'test'),
    (20011, [0.9, 0.436, 0.0]::FLOAT[3], 'test'),
    (20012, [0.0, 1.0, 0.0]::FLOAT[3], 'test');
SELECT inode, array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3]) as sim
FROM fs_embeddings WHERE inode BETWEEN 20010 AND 20012 ORDER BY sim DESC;
-- Expected order: 20010, 20011, 20012
DELETE FROM fs_embeddings WHERE inode BETWEEN 20010 AND 20012;

-- ============================================================================
-- T4: Chunk Embedding Tests
-- ============================================================================

SELECT '=== T4: Chunk Embedding Tests ===' as section;

-- T4.1: Insert chunked embeddings
SELECT 'T4.1: Insert chunked embeddings' as test_name;
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding) VALUES
    (30001, 0, 0, 1000, [0.1]::FLOAT[1]),
    (30001, 1, 1000, 2000, [0.2]::FLOAT[1]),
    (30001, 2, 2000, 3000, [0.3]::FLOAT[1]);
SELECT COUNT(*) = 3 as chunks_ok FROM fs_chunk_embeddings WHERE inode = 30001;
DELETE FROM fs_chunk_embeddings WHERE inode = 30001;

-- T4.2: Chunk index ordering
SELECT 'T4.2: Chunk index ordering' as test_name;
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding) VALUES
    (30002, 0, 0, 100, [0.1]::FLOAT[1]),
    (30002, 2, 200, 300, [0.3]::FLOAT[1]),
    (30002, 1, 100, 200, [0.2]::FLOAT[1]);
SELECT chunk_idx FROM fs_chunk_embeddings WHERE inode = 30002 ORDER BY chunk_idx;
-- Expected: 0, 1, 2
DELETE FROM fs_chunk_embeddings WHERE inode = 30002;

-- ============================================================================
-- T5: Integration Tests
-- ============================================================================

SELECT '=== T5: Integration Tests ===' as section;

-- T5.3: LEFT JOIN for missing embeddings
SELECT 'T5.3: LEFT JOIN for missing embeddings' as test_name;
INSERT INTO fs_journal (inode, event_type, parent, name, mode) VALUES (40020, 'create', 1, 'indexed.txt', 33188);
INSERT INTO fs_journal (inode, event_type, parent, name, mode) VALUES (40021, 'create', 1, 'not_indexed.txt', 33188);
INSERT INTO fs_embeddings (inode, embedding, model) VALUES (40020, [0.5]::FLOAT[1], 'test');
SELECT t.name, CASE WHEN e.inode IS NULL THEN 'no_embedding' ELSE 'has_embedding' END as status
FROM fs_tree t LEFT JOIN fs_embeddings e ON t.inode = e.inode WHERE t.inode IN (40020, 40021);
DELETE FROM fs_embeddings WHERE inode = 40020;
INSERT INTO fs_journal (inode, event_type, parent, name, mode) VALUES (40020, 'delete', 1, 'indexed.txt', 33188);
INSERT INTO fs_journal (inode, event_type, parent, name, mode) VALUES (40021, 'delete', 1, 'not_indexed.txt', 33188);

-- ============================================================================
-- T6: Edge Cases
-- ============================================================================

SELECT '=== T6: Edge Cases ===' as section;

-- T6.1: NULL embedding
SELECT 'T6.1: NULL embedding handling' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES (50001, NULL, 'none');
SELECT embedding IS NULL as null_ok FROM fs_embeddings WHERE inode = 50001;
DELETE FROM fs_embeddings WHERE inode = 50001;

-- T6.4: Orphan embedding
SELECT 'T6.4: Orphan embedding' as test_name;
INSERT INTO fs_embeddings (inode, embedding, model) VALUES (99999, [0.1]::FLOAT[1], 'test');
SELECT COUNT(*) = 0 as orphan_invisible FROM fs_embeddings e JOIN fs_tree t ON t.inode = e.inode WHERE e.inode = 99999;
DELETE FROM fs_embeddings WHERE inode = 99999;

-- ============================================================================
-- COMPLETE
-- ============================================================================

SELECT '=== ALL TESTS COMPLETED ===' as status;
```

## Test Traceability Matrix

| Acceptance Criteria | Test ID(s) | Coverage |
|---------------------|------------|----------|
| Table `fs_embeddings` with embedding vector column | T1.1, T1.3, T2.1, T6.3 | Full |
| Table `fs_chunk_embeddings` for large files | T1.2, T4.1, T4.2, T4.3 | Full |
| HNSW index definition | T7.1, T7.2 | Partial (requires VSS) |
| Model tracking for reproducibility | T1.4, T2.1 | Full |
| Content hash for change detection | T2.2, T2.3 | Full |

## Risk Mitigation

| Risk | Test Coverage | Mitigation |
|------|---------------|------------|
| Dimension mismatch at query time | T3.1-T3.5 use consistent dimensions | Application-level validation recommended |
| VSS extension unavailable | T7.x tests are marked optional | Linear scan fallback works without VSS |
| Orphan embeddings | T6.4 demonstrates issue | Recommend FK constraint (see QA Notes) |
| Memory pressure from HNSW | Not directly tested | Monitor index size in production |

## Appendix: SQL Test File Location

The executable test file should be created at:
```
schema/test_schema_vss.sql
```

And can be run with:
```bash
cd /home/fabricio/src/agentfs
duckdb :memory: < schema/duckagentfs.sql
duckdb :memory: < schema/test_schema_vss.sql
```

Or combined:
```bash
cat schema/duckagentfs.sql schema/test_schema_vss.sql | duckdb :memory:
```
