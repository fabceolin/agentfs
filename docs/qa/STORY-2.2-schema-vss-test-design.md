# Test Design: STORY-2.2 Schema VSS

## Document Metadata

| Field | Value |
|-------|-------|
| **Story ID** | STORY-2.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 2 - Vector Similarity Search |
| **Test Design Author** | QA Agent |
| **Created** | 2026-01-14 |
| **Target File** | `schema/duckagentfs.sql` |
| **Dependencies** | STORY-1.1 |
| **Status** | Ready for Implementation |

---

## 1. Test Scope Overview

### 1.1 Components Under Test

| Component | Description | Test Type |
|-----------|-------------|-----------|
| `fs_embeddings` table | File-level embedding storage with model tracking | DDL + Unit tests |
| `fs_chunk_embeddings` table | Chunk-level embeddings for large files | DDL + Unit tests |
| HNSW Index definitions | Approximate nearest neighbor search indexes | DDL + Conditional tests |
| Similarity functions | Cosine, Euclidean, Inner Product queries | SQL Query tests |
| Content hash tracking | Change detection for re-embedding | Unit + Integration tests |
| fs_tree JOIN patterns | Embedding-to-path lookups | Integration tests |

### 1.2 Test Categories Distribution

```
DDL Schema Tests:         35%  (table creation, column types, constraints)
Similarity Query Tests:   25%  (cosine, euclidean, inner product)
Content Hash Tests:       15%  (change detection, update logic)
Chunk Embedding Tests:    15%  (offset tracking, composite PK)
Integration Tests:        10%  (JOIN with fs_tree, path resolution)
```

### 1.3 Extension Dependencies

| Extension | Required For | Availability |
|-----------|--------------|--------------|
| VSS (vss) | HNSW index creation | Optional - tests degrade gracefully |
| Core DuckDB | All other tests | Always available |

---

## 2. DDL Schema Tests

### 2.1 fs_embeddings Table Tests

#### TEST-2.2-DDL001: fs_embeddings table creation
```sql
-- Test: Verify fs_embeddings table exists with correct structure
-- Setup: Load schema/duckagentfs.sql

SELECT table_name FROM information_schema.tables
WHERE table_name = 'fs_embeddings';
-- Expected: 1 row returned

SELECT column_name, data_type
FROM information_schema.columns
WHERE table_name = 'fs_embeddings'
ORDER BY ordinal_position;
-- Expected columns:
--   inode: UBIGINT (PRIMARY KEY)
--   embedding: FLOAT[] (array)
--   model: VARCHAR
--   content_hash: VARCHAR
--   generated_at: TIMESTAMP
--   metadata: JSON
```

#### TEST-2.2-DDL002: fs_embeddings primary key constraint
```sql
-- Test: Verify inode is primary key
-- Setup: Create test embeddings

INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (100, [0.1, 0.2]::FLOAT[2], 'test');

-- Attempt duplicate inode
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (100, [0.3, 0.4]::FLOAT[2], 'test');
-- Expected: PRIMARY KEY constraint violation error
```

#### TEST-2.2-DDL003: fs_embeddings model NOT NULL constraint
```sql
-- Test: Verify model column is NOT NULL
INSERT INTO fs_embeddings (inode, embedding)
VALUES (101, [0.1]::FLOAT[1]);
-- Expected: NOT NULL constraint violation for 'model'
```

#### TEST-2.2-DDL004: fs_embeddings default model value
```sql
-- Test: Verify default model is 'text-embedding-ada-002'
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (102, [0.1]::FLOAT[1], DEFAULT);

SELECT model FROM fs_embeddings WHERE inode = 102;
-- Expected: 'text-embedding-ada-002'
```

#### TEST-2.2-DDL005: fs_embeddings generated_at default
```sql
-- Test: Verify generated_at defaults to current_timestamp
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (103, [0.1]::FLOAT[1], 'test');

SELECT
    CASE WHEN generated_at IS NOT NULL
         AND generated_at <= current_timestamp
         AND generated_at >= current_timestamp - INTERVAL '1 second'
         THEN 'PASS' ELSE 'FAIL'
    END as test_result
FROM fs_embeddings WHERE inode = 103;
-- Expected: 'PASS'
```

#### TEST-2.2-DDL006: fs_embeddings nullable columns
```sql
-- Test: Verify content_hash, metadata are nullable
INSERT INTO fs_embeddings (inode, embedding, model, content_hash, metadata)
VALUES (104, [0.1]::FLOAT[1], 'test', NULL, NULL);

SELECT inode FROM fs_embeddings WHERE inode = 104;
-- Expected: 1 row (NULL values accepted)
```

### 2.2 fs_chunk_embeddings Table Tests

#### TEST-2.2-DDL010: fs_chunk_embeddings table creation
```sql
-- Test: Verify fs_chunk_embeddings table exists
SELECT table_name FROM information_schema.tables
WHERE table_name = 'fs_chunk_embeddings';
-- Expected: 1 row

SELECT column_name, data_type
FROM information_schema.columns
WHERE table_name = 'fs_chunk_embeddings'
ORDER BY ordinal_position;
-- Expected columns:
--   inode: UBIGINT (NOT NULL)
--   chunk_idx: UINTEGER (NOT NULL)
--   start_offset: UBIGINT (NOT NULL)
--   end_offset: UBIGINT (NOT NULL)
--   embedding: FLOAT[]
--   content_preview: VARCHAR(500)
--   generated_at: TIMESTAMP
--   PRIMARY KEY (inode, chunk_idx)
```

#### TEST-2.2-DDL011: fs_chunk_embeddings composite primary key
```sql
-- Test: Verify (inode, chunk_idx) is composite PK
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (200, 0, 0, 100, [0.1]::FLOAT[1]);

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (200, 1, 100, 200, [0.2]::FLOAT[1]);
-- Expected: Both inserts succeed (different chunk_idx)

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (200, 0, 50, 150, [0.3]::FLOAT[1]);
-- Expected: PRIMARY KEY constraint violation
```

#### TEST-2.2-DDL012: fs_chunk_embeddings NOT NULL constraints
```sql
-- Test: Verify inode, chunk_idx, start_offset, end_offset are NOT NULL
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (NULL, 0, 0, 100, [0.1]::FLOAT[1]);
-- Expected: NOT NULL constraint violation

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (201, NULL, 0, 100, [0.1]::FLOAT[1]);
-- Expected: NOT NULL constraint violation

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (201, 0, NULL, 100, [0.1]::FLOAT[1]);
-- Expected: NOT NULL constraint violation

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (201, 0, 0, NULL, [0.1]::FLOAT[1]);
-- Expected: NOT NULL constraint violation
```

#### TEST-2.2-DDL013: fs_chunk_embeddings content_preview length
```sql
-- Test: Verify content_preview truncates at 500 chars
INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding, content_preview)
VALUES (202, 0, 0, 100, [0.1]::FLOAT[1], REPEAT('x', 600));

SELECT LENGTH(content_preview) as preview_len FROM fs_chunk_embeddings WHERE inode = 202;
-- Expected: 500 (truncated to VARCHAR(500) limit)
-- Note: DuckDB may error if strict mode; otherwise truncates
```

#### TEST-2.2-DDL014: fs_chunk_embeddings multiple chunks per inode
```sql
-- Test: Verify multiple chunks can be stored per inode
INSERT INTO fs_chunk_embeddings VALUES
    (203, 0, 0, 1000, [0.1, 0.2, 0.3]::FLOAT[3], 'Chunk 0 preview', current_timestamp),
    (203, 1, 1000, 2000, [0.4, 0.5, 0.6]::FLOAT[3], 'Chunk 1 preview', current_timestamp),
    (203, 2, 2000, 3000, [0.7, 0.8, 0.9]::FLOAT[3], 'Chunk 2 preview', current_timestamp);

SELECT COUNT(*) as chunk_count FROM fs_chunk_embeddings WHERE inode = 203;
-- Expected: 3
```

---

## 3. Embedding Array Tests

### 3.1 Embedding Dimension Tests

#### TEST-2.2-EMB001: Insert embedding with 1536 dimension (ada-002)
```sql
-- Test: Verify 1536-dimension embedding storage
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (300, array_value(0.1::FLOAT, 1536), 'text-embedding-ada-002');

SELECT array_length(embedding) as dim FROM fs_embeddings WHERE inode = 300;
-- Expected: 1536
```

#### TEST-2.2-EMB002: Insert embedding with 3072 dimension (3-large)
```sql
-- Test: Verify 3072-dimension embedding storage
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (301, array_value(0.1::FLOAT, 3072), 'text-embedding-3-large');

SELECT array_length(embedding) as dim FROM fs_embeddings WHERE inode = 301;
-- Expected: 3072
```

#### TEST-2.2-EMB003: Insert embedding with 384 dimension (local model)
```sql
-- Test: Verify 384-dimension embedding storage
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (302, array_value(0.1::FLOAT, 384), 'all-MiniLM-L6-v2');

SELECT array_length(embedding) as dim FROM fs_embeddings WHERE inode = 302;
-- Expected: 384
```

#### TEST-2.2-EMB004: Mixed dimension embeddings coexist
```sql
-- Test: Verify different dimensions can coexist (when schema allows)
-- Note: If schema uses FLOAT[1536], this tests behavior with smaller arrays

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (310, [0.1, 0.2, 0.3]::FLOAT[3], 'test-3d'),
    (311, [0.1, 0.2, 0.3, 0.4, 0.5]::FLOAT[5], 'test-5d');

SELECT inode, array_length(embedding) as dim, model
FROM fs_embeddings
WHERE inode IN (310, 311)
ORDER BY inode;
-- Expected: Two rows with dimensions 3 and 5
```

#### TEST-2.2-EMB005: Empty embedding array
```sql
-- Test: Verify empty embedding array behavior
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (312, []::FLOAT[], 'empty-test');

SELECT array_length(embedding) as dim FROM fs_embeddings WHERE inode = 312;
-- Expected: 0 or NULL (empty array)
```

---

## 4. Similarity Function Tests

### 4.1 Cosine Similarity Tests

#### TEST-2.2-SIM001: Cosine similarity of identical vectors
```sql
-- Test: Identical vectors have similarity 1.0
SELECT array_cosine_similarity(
    [1.0, 0.0, 0.0]::FLOAT[3],
    [1.0, 0.0, 0.0]::FLOAT[3]
) as similarity;
-- Expected: 1.0
```

#### TEST-2.2-SIM002: Cosine similarity of orthogonal vectors
```sql
-- Test: Orthogonal vectors have similarity 0.0
SELECT array_cosine_similarity(
    [1.0, 0.0, 0.0]::FLOAT[3],
    [0.0, 1.0, 0.0]::FLOAT[3]
) as similarity;
-- Expected: 0.0
```

#### TEST-2.2-SIM003: Cosine similarity of opposite vectors
```sql
-- Test: Opposite vectors have similarity -1.0
SELECT array_cosine_similarity(
    [1.0, 0.0, 0.0]::FLOAT[3],
    [-1.0, 0.0, 0.0]::FLOAT[3]
) as similarity;
-- Expected: -1.0
```

#### TEST-2.2-SIM004: Cosine similarity ordering
```sql
-- Test: Verify similarity ordering is correct
-- Setup: Insert test embeddings
DELETE FROM fs_embeddings WHERE inode >= 400 AND inode < 500;

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (400, [1.0, 0.0, 0.0]::FLOAT[3], 'test'),
    (401, [0.95, 0.05, 0.0]::FLOAT[3], 'test'),  -- Very similar to query
    (402, [0.5, 0.5, 0.0]::FLOAT[3], 'test'),    -- Moderately similar
    (403, [0.0, 1.0, 0.0]::FLOAT[3], 'test');    -- Orthogonal

-- Query with [1,0,0] - should return inodes in order: 400, 401, 402, 403
SELECT inode,
       array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3]) as sim
FROM fs_embeddings
WHERE inode >= 400 AND inode < 500
ORDER BY sim DESC;
-- Expected order: 400 (1.0), 401 (~0.998), 402 (~0.707), 403 (0.0)
```

#### TEST-2.2-SIM005: Cosine similarity with stored embeddings
```sql
-- Test: Similarity search using table JOIN
DELETE FROM fs_embeddings WHERE inode >= 500 AND inode < 600;

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (500, [0.8, 0.6, 0.0]::FLOAT[3], 'test'),
    (501, [0.6, 0.8, 0.0]::FLOAT[3], 'test'),
    (502, [0.0, 0.0, 1.0]::FLOAT[3], 'test');

-- Query for files similar to [1,0,0]
WITH query_vec AS (SELECT [1.0, 0.0, 0.0]::FLOAT[3] as vec)
SELECT
    e.inode,
    array_cosine_similarity(e.embedding, q.vec) as score
FROM fs_embeddings e, query_vec q
WHERE e.inode >= 500 AND e.inode < 600
ORDER BY score DESC
LIMIT 2;
-- Expected: 500 first (higher x component), then 501
```

### 4.2 Euclidean Distance Tests

#### TEST-2.2-SIM010: Euclidean distance of identical vectors
```sql
-- Test: Identical vectors have distance 0.0
SELECT array_distance(
    [1.0, 2.0, 3.0]::FLOAT[3],
    [1.0, 2.0, 3.0]::FLOAT[3]
) as distance;
-- Expected: 0.0
```

#### TEST-2.2-SIM011: Euclidean distance calculation
```sql
-- Test: Verify correct distance calculation
-- Distance between [0,0,0] and [3,4,0] should be 5
SELECT array_distance(
    [0.0, 0.0, 0.0]::FLOAT[3],
    [3.0, 4.0, 0.0]::FLOAT[3]
) as distance;
-- Expected: 5.0
```

#### TEST-2.2-SIM012: Euclidean distance ordering
```sql
-- Test: Verify distance ordering (ASC for nearest)
DELETE FROM fs_embeddings WHERE inode >= 600 AND inode < 700;

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (600, [0.0, 0.0, 0.0]::FLOAT[3], 'test'),
    (601, [1.0, 0.0, 0.0]::FLOAT[3], 'test'),
    (602, [5.0, 0.0, 0.0]::FLOAT[3], 'test');

SELECT inode,
       array_distance(embedding, [0.0, 0.0, 0.0]::FLOAT[3]) as dist
FROM fs_embeddings
WHERE inode >= 600 AND inode < 700
ORDER BY dist ASC;
-- Expected order: 600 (0.0), 601 (1.0), 602 (5.0)
```

### 4.3 Inner Product Tests

#### TEST-2.2-SIM020: Inner product calculation
```sql
-- Test: Verify inner product calculation
-- [1,2,3] . [4,5,6] = 1*4 + 2*5 + 3*6 = 4 + 10 + 18 = 32
SELECT array_inner_product(
    [1.0, 2.0, 3.0]::FLOAT[3],
    [4.0, 5.0, 6.0]::FLOAT[3]
) as inner_prod;
-- Expected: 32.0
```

#### TEST-2.2-SIM021: Inner product of orthogonal vectors
```sql
-- Test: Orthogonal vectors have inner product 0
SELECT array_inner_product(
    [1.0, 0.0]::FLOAT[2],
    [0.0, 1.0]::FLOAT[2]
) as inner_prod;
-- Expected: 0.0
```

#### TEST-2.2-SIM022: Inner product ordering
```sql
-- Test: Higher inner product = more similar (for normalized vectors)
DELETE FROM fs_embeddings WHERE inode >= 700 AND inode < 800;

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (700, [1.0, 1.0]::FLOAT[2], 'test'),
    (701, [0.5, 0.5]::FLOAT[2], 'test'),
    (702, [-1.0, -1.0]::FLOAT[2], 'test');

SELECT inode,
       array_inner_product(embedding, [1.0, 1.0]::FLOAT[2]) as score
FROM fs_embeddings
WHERE inode >= 700 AND inode < 800
ORDER BY score DESC;
-- Expected order: 700 (2.0), 701 (1.0), 702 (-2.0)
```

---

## 5. Content Hash Tests

### 5.1 Change Detection Tests

#### TEST-2.2-HASH001: Insert embedding with content hash
```sql
-- Test: Store embedding with content hash
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (800, [0.1]::FLOAT[1], 'test', 'abc123def456');

SELECT content_hash FROM fs_embeddings WHERE inode = 800;
-- Expected: 'abc123def456'
```

#### TEST-2.2-HASH002: Detect content change via hash comparison
```sql
-- Test: Check if hash differs (simulating change detection)
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (801, [0.1]::FLOAT[1], 'test', 'hash_v1');

-- Simulate checking if update needed
SELECT
    CASE WHEN content_hash != 'hash_v2' THEN 'needs_update' ELSE 'current' END as status
FROM fs_embeddings WHERE inode = 801;
-- Expected: 'needs_update'
```

#### TEST-2.2-HASH003: Update embedding when hash changes
```sql
-- Test: Update embedding and hash on content change
INSERT INTO fs_embeddings (inode, embedding, model, content_hash, generated_at)
VALUES (802, [0.1]::FLOAT[1], 'test', 'old_hash', '2024-01-01 00:00:00');

-- Update with new embedding and hash
UPDATE fs_embeddings
SET embedding = [0.2, 0.3]::FLOAT[2],
    content_hash = 'new_hash',
    generated_at = current_timestamp
WHERE inode = 802;

SELECT
    content_hash,
    array_length(embedding) as dim,
    CASE WHEN generated_at > '2024-01-01 00:00:00' THEN 'UPDATED' ELSE 'NOT_UPDATED' END as status
FROM fs_embeddings WHERE inode = 802;
-- Expected: 'new_hash', 2, 'UPDATED'
```

#### TEST-2.2-HASH004: Skip update when hash matches
```sql
-- Test: Idempotency check - no update when hash matches
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (803, [0.1]::FLOAT[1], 'test', 'unchanged_hash');

-- Check if update needed (simulate application logic)
SELECT COUNT(*) as needs_update
FROM fs_embeddings
WHERE inode = 803 AND content_hash != 'unchanged_hash';
-- Expected: 0 (no update needed)
```

---

## 6. Chunk Embedding Tests

### 6.1 Offset Tracking Tests

#### TEST-2.2-CHUNK001: Insert chunks with correct offsets
```sql
-- Test: Store chunked embeddings with byte offsets
DELETE FROM fs_chunk_embeddings WHERE inode = 900;

INSERT INTO fs_chunk_embeddings
    (inode, chunk_idx, start_offset, end_offset, embedding, content_preview)
VALUES
    (900, 0, 0, 1000, [0.1, 0.2]::FLOAT[2], 'First chunk preview...'),
    (900, 1, 1000, 2000, [0.3, 0.4]::FLOAT[2], 'Second chunk preview...'),
    (900, 2, 2000, 3500, [0.5, 0.6]::FLOAT[2], 'Third chunk preview...');

SELECT chunk_idx, start_offset, end_offset
FROM fs_chunk_embeddings
WHERE inode = 900
ORDER BY chunk_idx;
-- Expected: 3 rows with sequential offsets
```

#### TEST-2.2-CHUNK002: Verify chunk_idx ordering
```sql
-- Test: Chunks should be ordered by chunk_idx
SELECT
    chunk_idx,
    LAG(end_offset) OVER (ORDER BY chunk_idx) as prev_end,
    start_offset
FROM fs_chunk_embeddings WHERE inode = 900
ORDER BY chunk_idx;
-- Expected: Each start_offset equals previous end_offset (contiguous)
```

#### TEST-2.2-CHUNK003: Query specific chunk by offset
```sql
-- Test: Find chunk containing byte offset 1500
SELECT chunk_idx, content_preview
FROM fs_chunk_embeddings
WHERE inode = 900
  AND start_offset <= 1500
  AND end_offset > 1500;
-- Expected: chunk_idx = 1 (1000-2000 contains 1500)
```

#### TEST-2.2-CHUNK004: Chunk similarity search
```sql
-- Test: Find most similar chunk
INSERT INTO fs_chunk_embeddings
    (inode, chunk_idx, start_offset, end_offset, embedding, content_preview)
VALUES
    (901, 0, 0, 500, [1.0, 0.0]::FLOAT[2], 'High similarity chunk'),
    (901, 1, 500, 1000, [0.0, 1.0]::FLOAT[2], 'Low similarity chunk');

SELECT
    chunk_idx,
    content_preview,
    array_cosine_similarity(embedding, [1.0, 0.0]::FLOAT[2]) as sim
FROM fs_chunk_embeddings
WHERE inode = 901
ORDER BY sim DESC
LIMIT 1;
-- Expected: chunk_idx = 0 with sim = 1.0
```

#### TEST-2.2-CHUNK005: Overlapping chunk offsets
```sql
-- Test: Handle overlapping chunks (sliding window)
DELETE FROM fs_chunk_embeddings WHERE inode = 902;

INSERT INTO fs_chunk_embeddings
    (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES
    (902, 0, 0, 500, [0.1]::FLOAT[1]),
    (902, 1, 400, 900, [0.2]::FLOAT[1]),  -- Overlaps with chunk 0
    (902, 2, 800, 1300, [0.3]::FLOAT[1]); -- Overlaps with chunk 1

SELECT chunk_idx, start_offset, end_offset FROM fs_chunk_embeddings WHERE inode = 902;
-- Expected: 3 rows (overlapping chunks allowed)
```

---

## 7. Integration Tests (fs_tree JOIN)

### 7.1 Path Resolution Tests

#### TEST-2.2-INT001: Join embeddings with file paths
```sql
-- Test: Retrieve embedding with file path
-- Prerequisite: fs_tree view and fs_journal populated

-- Setup: Create test file in journal
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (1000, 'create', 1, 'embedded_file.txt', 33188, 1024);

INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (1000, [0.1, 0.2, 0.3]::FLOAT[3], 'test');

-- Query with path
SELECT
    t.path,
    e.model,
    array_length(e.embedding) as dim
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE e.inode = 1000;
-- Expected: path='embedded_file.txt', model='test', dim=3
```

#### TEST-2.2-INT002: Filter regular files only in similarity search
```sql
-- Test: Mode bitmask filters directories
-- Setup: Create directory and file

INSERT INTO fs_journal (inode, event_type, parent, name, mode) VALUES
    (1001, 'create', 1, 'test_dir', 16877),     -- Directory: S_IFDIR | 0755
    (1002, 'create', 1001, 'test_file.txt', 33188); -- File: S_IFREG | 0644

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (1001, [0.1]::FLOAT[1], 'test'),
    (1002, [0.1]::FLOAT[1], 'test');

-- Query regular files only
SELECT
    t.path,
    (t.mode & 61440) as file_type_bits
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE (t.mode & 61440) = 32768  -- S_IFREG = 0100000 = 32768
  AND e.inode IN (1001, 1002);
-- Expected: Only 1002 returned (regular file)
```

#### TEST-2.2-INT003: Semantic search with path and score
```sql
-- Test: Full semantic search with path resolution
DELETE FROM fs_embeddings WHERE inode >= 1100 AND inode < 1200;

-- Setup test files
INSERT INTO fs_journal (inode, event_type, parent, name, mode, size) VALUES
    (1100, 'create', 1, 'readme.md', 33188, 500),
    (1101, 'create', 1, 'config.json', 33188, 200),
    (1102, 'create', 1, 'script.py', 33188, 1000);

INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (1100, [0.9, 0.1, 0.0]::FLOAT[3], 'test'),  -- Similar to query
    (1101, [0.1, 0.9, 0.0]::FLOAT[3], 'test'),  -- Less similar
    (1102, [0.0, 0.0, 1.0]::FLOAT[3], 'test');  -- Orthogonal

-- Semantic search
SELECT
    t.path,
    e.model,
    array_cosine_similarity(e.embedding, [1.0, 0.0, 0.0]::FLOAT[3]) as score
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE (t.mode & 61440) = 32768
  AND e.inode >= 1100 AND e.inode < 1200
ORDER BY score DESC
LIMIT 3;
-- Expected: readme.md first (highest similarity)
```

#### TEST-2.2-INT004: Chunk-level search with path
```sql
-- Test: Chunk search returns file path and byte range
DELETE FROM fs_chunk_embeddings WHERE inode = 1200;
DELETE FROM fs_journal WHERE inode = 1200;

INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (1200, 'create', 1, 'large_document.txt', 33188, 5000);

INSERT INTO fs_chunk_embeddings
    (inode, chunk_idx, start_offset, end_offset, embedding, content_preview)
VALUES
    (1200, 0, 0, 2000, [1.0, 0.0]::FLOAT[2], 'Introduction...'),
    (1200, 1, 2000, 4000, [0.5, 0.5]::FLOAT[2], 'Middle section...'),
    (1200, 2, 4000, 5000, [0.0, 1.0]::FLOAT[2], 'Conclusion...');

SELECT
    t.path,
    c.chunk_idx,
    c.start_offset,
    c.end_offset,
    c.content_preview,
    array_cosine_similarity(c.embedding, [1.0, 0.0]::FLOAT[2]) as score
FROM fs_chunk_embeddings c
JOIN fs_tree t ON t.inode = c.inode
WHERE c.inode = 1200
ORDER BY score DESC
LIMIT 2;
-- Expected: First two most similar chunks with paths
```

#### TEST-2.2-INT005: Handle missing embeddings (LEFT JOIN)
```sql
-- Test: Files without embeddings should be handled gracefully
DELETE FROM fs_journal WHERE inode = 1300;

INSERT INTO fs_journal (inode, event_type, parent, name, mode, size)
VALUES (1300, 'create', 1, 'no_embedding.txt', 33188, 100);

-- No embedding inserted for 1300

SELECT
    t.path,
    CASE WHEN e.embedding IS NULL THEN 'NO_EMBEDDING' ELSE 'HAS_EMBEDDING' END as status
FROM fs_tree t
LEFT JOIN fs_embeddings e ON e.inode = t.inode
WHERE t.inode = 1300;
-- Expected: 'NO_EMBEDDING'
```

---

## 8. HNSW Index Tests (Conditional)

### 8.1 VSS Extension Tests

#### TEST-2.2-HNSW001: Check VSS extension availability
```sql
-- Test: Verify if VSS extension can be loaded
-- This test determines if HNSW tests should run

SELECT
    CASE
        WHEN loaded THEN 'VSS_AVAILABLE'
        ELSE 'VSS_NOT_AVAILABLE'
    END as vss_status
FROM (
    SELECT COUNT(*) > 0 as loaded
    FROM duckdb_extensions()
    WHERE extension_name = 'vss' AND loaded = true
);
-- Expected: Either 'VSS_AVAILABLE' or 'VSS_NOT_AVAILABLE'
-- Subsequent HNSW tests should be skipped if not available
```

#### TEST-2.2-HNSW002: Create HNSW index on fs_embeddings (conditional)
```sql
-- Test: HNSW index creation (requires VSS extension)
-- Skip if VSS not available

-- INSTALL vss;
-- LOAD vss;

CREATE INDEX idx_fs_embeddings_hnsw
ON fs_embeddings USING HNSW (embedding)
WITH (metric = 'cosine');
-- Expected: Index created successfully (if VSS available)
```

#### TEST-2.2-HNSW003: Create HNSW index on fs_chunk_embeddings (conditional)
```sql
-- Test: HNSW index on chunk embeddings
-- Skip if VSS not available

CREATE INDEX idx_fs_chunk_embeddings_hnsw
ON fs_chunk_embeddings USING HNSW (embedding)
WITH (metric = 'cosine');
-- Expected: Index created successfully
```

#### TEST-2.2-HNSW004: HNSW index used in query plan
```sql
-- Test: Verify HNSW index is used for similarity queries
-- Skip if VSS not available

EXPLAIN ANALYZE
SELECT inode, array_cosine_similarity(embedding, [0.1, 0.2, 0.3]::FLOAT[3]) as sim
FROM fs_embeddings
ORDER BY sim DESC
LIMIT 10;
-- Expected: Query plan shows HNSW index scan (not table scan)
```

---

## 9. Edge Case Tests

### 9.1 Dimension Mismatch Tests

#### TEST-2.2-EDGE001: Query with mismatched dimension
```sql
-- Test: Cosine similarity with different dimensions
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (1400, [0.1, 0.2, 0.3]::FLOAT[3], 'test-3d');

-- Query with 2D vector against 3D embedding
SELECT array_cosine_similarity(embedding, [1.0, 0.0]::FLOAT[2]) as sim
FROM fs_embeddings WHERE inode = 1400;
-- Expected: Error or undefined behavior (depends on DuckDB version)
-- Application should validate dimension match before query
```

#### TEST-2.2-EDGE002: Zero vector handling
```sql
-- Test: Cosine similarity with zero vector
SELECT array_cosine_similarity(
    [0.0, 0.0, 0.0]::FLOAT[3],
    [1.0, 0.0, 0.0]::FLOAT[3]
) as similarity;
-- Expected: NaN or error (zero magnitude vector)
```

#### TEST-2.2-EDGE003: Very small embedding values
```sql
-- Test: Similarity with tiny float values
SELECT array_cosine_similarity(
    [1e-38, 1e-38, 1e-38]::FLOAT[3],
    [1e-38, 1e-38, 1e-38]::FLOAT[3]
) as similarity;
-- Expected: Close to 1.0 (same direction despite tiny magnitude)
```

#### TEST-2.2-EDGE004: Very large embedding values
```sql
-- Test: Similarity with large float values
SELECT array_cosine_similarity(
    [1e38, 0.0, 0.0]::FLOAT[3],
    [1e38, 0.0, 0.0]::FLOAT[3]
) as similarity;
-- Expected: 1.0 (or potential overflow handling)
```

### 9.2 Null Handling Tests

#### TEST-2.2-EDGE010: NULL embedding array
```sql
-- Test: NULL embedding behavior
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (1500, NULL, 'test-null');

SELECT inode, embedding IS NULL as is_null FROM fs_embeddings WHERE inode = 1500;
-- Expected: is_null = true
```

#### TEST-2.2-EDGE011: Similarity with NULL embedding
```sql
-- Test: Cosine similarity when one operand is NULL
SELECT array_cosine_similarity(
    NULL::FLOAT[3],
    [1.0, 0.0, 0.0]::FLOAT[3]
) as similarity;
-- Expected: NULL
```

#### TEST-2.2-EDGE012: Filter NULL embeddings
```sql
-- Test: Exclude NULL embeddings from similarity search
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (1501, NULL, 'null-test'),
    (1502, [1.0, 0.0]::FLOAT[2], 'valid-test');

SELECT inode FROM fs_embeddings
WHERE inode IN (1501, 1502) AND embedding IS NOT NULL;
-- Expected: Only 1502
```

### 9.3 Boundary Conditions

#### TEST-2.2-EDGE020: Single element embedding
```sql
-- Test: 1-dimensional embedding
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (1600, [0.5]::FLOAT[1], 'test-1d');

SELECT array_cosine_similarity(embedding, [1.0]::FLOAT[1]) as sim
FROM fs_embeddings WHERE inode = 1600;
-- Expected: 1.0 (same sign = similar)
```

#### TEST-2.2-EDGE021: Maximum chunk index
```sql
-- Test: Large chunk_idx value
INSERT INTO fs_chunk_embeddings
    (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (1700, 4294967295, 0, 100, [0.1]::FLOAT[1]);  -- Max UINTEGER

SELECT chunk_idx FROM fs_chunk_embeddings WHERE inode = 1700;
-- Expected: 4294967295
```

#### TEST-2.2-EDGE022: Maximum offset values
```sql
-- Test: Large offset values (UBIGINT max)
INSERT INTO fs_chunk_embeddings
    (inode, chunk_idx, start_offset, end_offset, embedding)
VALUES (1701, 0, 18446744073709551610, 18446744073709551615, [0.1]::FLOAT[1]);

SELECT start_offset, end_offset FROM fs_chunk_embeddings WHERE inode = 1701;
-- Expected: Large values preserved
```

---

## 10. Performance Tests

### 10.1 Query Performance

#### PERF-2.2-001: Similarity search on 10K embeddings
```sql
-- Test: Measure similarity search time
-- Setup: Generate 10K test embeddings

-- Create test data (conceptual - use actual generation)
INSERT INTO fs_embeddings (inode, embedding, model)
SELECT
    seq as inode,
    array_value(RANDOM()::FLOAT, 128) as embedding,  -- 128D for speed
    'perf-test' as model
FROM generate_series(10000, 19999) as t(seq);

-- Time the query
SELECT inode, array_cosine_similarity(embedding, array_value(0.5::FLOAT, 128)) as sim
FROM fs_embeddings
WHERE inode >= 10000 AND inode < 20000
ORDER BY sim DESC
LIMIT 10;
-- Expected: Completes in < 1 second without HNSW, < 100ms with HNSW
```

#### PERF-2.2-002: Chunk embedding retrieval
```sql
-- Test: Retrieve all chunks for a file
-- Setup: File with 1000 chunks

INSERT INTO fs_chunk_embeddings (inode, chunk_idx, start_offset, end_offset, embedding)
SELECT
    20000 as inode,
    seq as chunk_idx,
    seq * 1000 as start_offset,
    (seq + 1) * 1000 as end_offset,
    array_value(RANDOM()::FLOAT, 64) as embedding
FROM generate_series(0, 999) as t(seq);

-- Time retrieval
SELECT COUNT(*) FROM fs_chunk_embeddings WHERE inode = 20000;
-- Expected: 1000 rows in < 100ms
```

---

## 11. Test Data Fixtures

### 11.1 Standard Test Embeddings

```sql
-- Fixture: Standard test embeddings for unit tests
CREATE OR REPLACE TABLE test_embeddings_fixture AS
SELECT * FROM (VALUES
    (1, [1.0, 0.0, 0.0]::FLOAT[3], 'x-axis'),
    (2, [0.0, 1.0, 0.0]::FLOAT[3], 'y-axis'),
    (3, [0.0, 0.0, 1.0]::FLOAT[3], 'z-axis'),
    (4, [0.707, 0.707, 0.0]::FLOAT[3], 'xy-diagonal'),
    (5, [0.577, 0.577, 0.577]::FLOAT[3], 'xyz-diagonal')
) AS t(id, embedding, description);
```

### 11.2 Test Models Reference

| Model ID | Dimension | Use Case |
|----------|-----------|----------|
| `test-3d` | 3 | Unit tests |
| `test-128d` | 128 | Performance tests |
| `text-embedding-ada-002` | 1536 | OpenAI compatibility |
| `text-embedding-3-large` | 3072 | Large embedding tests |
| `all-MiniLM-L6-v2` | 384 | Local model tests |

---

## 12. Test Execution Plan

### 12.1 Test Categories by Priority

| Priority | Category | Test Count | Dependencies |
|----------|----------|------------|--------------|
| P0 | DDL Schema | 14 | None |
| P0 | Embedding Array | 5 | DDL complete |
| P0 | Similarity Functions | 11 | DDL complete |
| P1 | Content Hash | 4 | DDL complete |
| P1 | Chunk Embeddings | 5 | DDL complete |
| P2 | Integration (fs_tree) | 5 | fs_journal, fs_tree |
| P2 | HNSW Index | 4 | VSS extension |
| P3 | Edge Cases | 13 | All above |
| P3 | Performance | 2 | 10K+ test data |

### 12.2 Test Execution Order

1. **Schema Setup**: Load `schema/duckagentfs.sql`
2. **DDL Verification**: Run TEST-2.2-DDL* tests
3. **Embedding Tests**: Run TEST-2.2-EMB* tests
4. **Similarity Tests**: Run TEST-2.2-SIM* tests
5. **Hash Tests**: Run TEST-2.2-HASH* tests
6. **Chunk Tests**: Run TEST-2.2-CHUNK* tests
7. **Integration Tests**: Run TEST-2.2-INT* tests (requires fs_journal)
8. **HNSW Tests**: Run TEST-2.2-HNSW* tests (skip if VSS unavailable)
9. **Edge Cases**: Run TEST-2.2-EDGE* tests
10. **Performance**: Run PERF-2.2-* tests (optional)

### 12.3 CI/CD Integration

```yaml
# .github/workflows/schema-tests.yaml
jobs:
  test-vss-schema:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install DuckDB
        run: pip install duckdb

      - name: Run DDL tests
        run: |
          python -c "
          import duckdb
          conn = duckdb.connect(':memory:')
          with open('schema/duckagentfs.sql') as f:
              conn.execute(f.read())
          with open('schema/test_schema_vss.sql') as f:
              for stmt in f.read().split(';'):
                  if stmt.strip():
                      print(conn.execute(stmt).fetchall())
          "

      - name: Run VSS tests (conditional)
        run: |
          python -c "
          import duckdb
          conn = duckdb.connect()
          try:
              conn.execute('INSTALL vss; LOAD vss;')
              print('VSS available - running HNSW tests')
              # Run HNSW tests
          except:
              print('VSS not available - skipping HNSW tests')
          "
```

---

## 13. Coverage Requirements

### 13.1 Minimum Coverage Targets

| Component | Line Coverage | Branch Coverage |
|-----------|---------------|-----------------|
| `fs_embeddings` DDL | 100% | 100% |
| `fs_chunk_embeddings` DDL | 100% | 100% |
| Similarity functions | 95% | 90% |
| Content hash logic | 90% | 85% |
| HNSW index | 80% | 75% (conditional) |

### 13.2 Coverage Exclusions

- Live VSS extension loading (requires installation)
- Production-scale performance (10M+ embeddings)
- Network-based embedding generation (covered by STORY-2.1)

---

## 14. Risk Mitigation

### 14.1 Identified Risks from Story QA

| Risk | Test Coverage | Mitigation |
|------|---------------|------------|
| **Dimension Mismatch** | TEST-2.2-EDGE001 | Tests verify error/undefined behavior; recommend app-level validation |
| **VSS Extension Missing** | TEST-2.2-HNSW001 | Conditional test execution; fallback to linear scan |
| **No FK Constraint** | TEST-2.2-INT005 | LEFT JOIN tests handle missing embeddings |
| **Content Hash Collision** | TEST-2.2-HASH* | MD5/SHA collision tested implicitly; documented as low risk |
| **Memory Pressure (HNSW)** | PERF-2.2-* | Performance tests with 10K embeddings |

### 14.2 Recommendations for Production

1. **Add FK Constraint**: Consider `FOREIGN KEY (inode) REFERENCES fs_current(inode) ON DELETE CASCADE`
2. **Dimension Validation**: Implement at application layer before INSERT/UPDATE
3. **VSS Fallback**: Document linear scan behavior when HNSW unavailable
4. **Index Monitoring**: Track HNSW index size and rebuild periodically

---

## 15. Appendix

### 15.1 Test Naming Convention

```
TEST-{story}-{category}{number}: {description}

Categories:
- DDL: Schema definition tests
- EMB: Embedding array tests
- SIM: Similarity function tests
- HASH: Content hash tests
- CHUNK: Chunk embedding tests
- INT: Integration tests
- HNSW: HNSW index tests
- EDGE: Edge case tests

PERF-{story}-{number}: Performance benchmarks
```

### 15.2 Related Documentation

- [STORY-2.2 Schema VSS](../stories/duckagentfs/STORY-2.2-schema-vss.md)
- [STORY-1.1 Schema DDL](../stories/duckagentfs/STORY-1.1-schema-ddl.md)
- [DuckDB VSS Extension](https://duckdb.org/docs/extensions/vss)
- [Cosine Similarity in DuckDB](https://duckdb.org/docs/sql/functions/array)

---

**BMAD_QA_COMPLETED**
