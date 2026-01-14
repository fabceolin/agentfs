# STORY-2.2: Schema VSS

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 2 - Vector Similarity Search |
| **Status** | Done |
| **Priority** | High |
| **File** | `schema/duckagentfs.sql` |
| **Dependencies** | STORY-1.1 |

## User Story

**As a** developer
**I want** tables to store embeddings
**So that** I can index files semantically

## Technical Description

Vector Similarity Search (VSS) enables finding files by semantic meaning rather than exact text match. This requires:

1. Storing embedding vectors alongside file metadata
2. Creating HNSW indexes for fast approximate nearest neighbor search
3. Supporting chunked embeddings for large files

## Acceptance Criteria

- [x] Table `fs_embeddings` with embedding vector column
- [x] Table `fs_chunk_embeddings` for large files
- [x] HNSW index definition (commented, requires VSS extension)
- [x] Model tracking for reproducibility
- [x] Content hash for change detection

## Technical Specification

### fs_embeddings Table

```sql
CREATE TABLE IF NOT EXISTS fs_embeddings (
    inode           UBIGINT PRIMARY KEY,
    embedding       FLOAT[1536],   -- Configurable dimension
    model           VARCHAR NOT NULL DEFAULT 'text-embedding-ada-002',
    content_hash    VARCHAR,       -- MD5/SHA hash of content
    generated_at    TIMESTAMP DEFAULT current_timestamp,
    metadata        JSON
);
```

### fs_chunk_embeddings Table

For files larger than the embedding model's context window:

```sql
CREATE TABLE IF NOT EXISTS fs_chunk_embeddings (
    inode           UBIGINT NOT NULL,
    chunk_idx       UINTEGER NOT NULL,
    start_offset    UBIGINT NOT NULL,
    end_offset      UBIGINT NOT NULL,
    embedding       FLOAT[1536],
    content_preview VARCHAR(500),  -- First 500 chars for context
    generated_at    TIMESTAMP DEFAULT current_timestamp,
    PRIMARY KEY (inode, chunk_idx)
);
```

### HNSW Index

```sql
-- Requires: INSTALL vss; LOAD vss;

CREATE INDEX IF NOT EXISTS idx_fs_embeddings_hnsw
ON fs_embeddings USING HNSW (embedding)
WITH (metric = 'cosine');

-- For chunk-level search
CREATE INDEX IF NOT EXISTS idx_fs_chunk_embeddings_hnsw
ON fs_chunk_embeddings USING HNSW (embedding)
WITH (metric = 'cosine');
```

### Dimension Configuration

Different embedding models have different dimensions:

| Model | Provider | Dimension |
|-------|----------|-----------|
| text-embedding-ada-002 | OpenAI | 1536 |
| text-embedding-3-small | OpenAI | 1536 |
| text-embedding-3-large | OpenAI | 3072 |
| embed-english-v3.0 | Cohere | 1024 |
| all-MiniLM-L6-v2 | HuggingFace | 384 |
| bge-small-en-v1.5 | BAAI | 384 |

To change dimension, recreate table:

```sql
-- Drop and recreate with new dimension
DROP TABLE IF EXISTS fs_embeddings;
CREATE TABLE fs_embeddings (
    inode           UBIGINT PRIMARY KEY,
    embedding       FLOAT[384],  -- Changed for local model
    model           VARCHAR NOT NULL,
    content_hash    VARCHAR,
    generated_at    TIMESTAMP DEFAULT current_timestamp,
    metadata        JSON
);
```

## Similarity Functions

### Cosine Similarity

```sql
-- Using DuckDB VSS extension
SELECT
    inode,
    array_cosine_similarity(embedding, $query_embedding) as similarity
FROM fs_embeddings
ORDER BY similarity DESC
LIMIT 10;
```

### Euclidean Distance

```sql
SELECT
    inode,
    array_distance(embedding, $query_embedding) as distance
FROM fs_embeddings
ORDER BY distance ASC
LIMIT 10;
```

### Inner Product

```sql
SELECT
    inode,
    array_inner_product(embedding, $query_embedding) as score
FROM fs_embeddings
ORDER BY score DESC
LIMIT 10;
```

## Usage Examples

### Insert Embedding

```sql
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (
    42,
    [0.1, 0.2, 0.3, ...]::FLOAT[1536],
    'text-embedding-ada-002',
    'abc123def456'
);
```

### Update on Content Change

```sql
-- Check if content changed before re-embedding
SELECT content_hash FROM fs_embeddings WHERE inode = 42;

-- If hash differs, update embedding
UPDATE fs_embeddings
SET embedding = [...]::FLOAT[1536],
    content_hash = 'new_hash',
    generated_at = current_timestamp
WHERE inode = 42;
```

### Search with Path Join

```sql
SELECT
    t.path,
    e.model,
    array_cosine_similarity(e.embedding, $query) as score
FROM fs_embeddings e
JOIN fs_tree t ON t.inode = e.inode
WHERE (t.mode & 61440) = 32768  -- Regular files only
ORDER BY score DESC
LIMIT 10;
```

### Chunk-Level Search

```sql
-- Find specific passages within files
SELECT
    t.path,
    c.chunk_idx,
    c.start_offset,
    c.end_offset,
    c.content_preview,
    array_cosine_similarity(c.embedding, $query) as score
FROM fs_chunk_embeddings c
JOIN fs_tree t ON t.inode = c.inode
ORDER BY score DESC
LIMIT 20;
```

## Tests

### Test 1: Insert and Query Embedding
```sql
-- Setup
INSERT INTO fs_embeddings (inode, embedding, model)
VALUES (100, [0.1, 0.2, 0.3]::FLOAT[3], 'test');

-- Query
SELECT inode FROM fs_embeddings WHERE inode = 100;
-- Expected: 1 row
```

### Test 2: Similarity Search (without index)
```sql
-- Insert test data
INSERT INTO fs_embeddings (inode, embedding, model) VALUES
    (1, [1.0, 0.0, 0.0]::FLOAT[3], 'test'),
    (2, [0.9, 0.1, 0.0]::FLOAT[3], 'test'),
    (3, [0.0, 1.0, 0.0]::FLOAT[3], 'test');

-- Search for vector similar to [1,0,0]
SELECT
    inode,
    array_cosine_similarity(embedding, [1.0, 0.0, 0.0]::FLOAT[3]) as sim
FROM fs_embeddings
ORDER BY sim DESC;

-- Expected: inode 1 first (sim=1.0), inode 2 second (sim~0.99)
```

### Test 3: Content Hash Update Detection
```sql
-- Initial insert
INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
VALUES (200, [0.5]::FLOAT[1], 'test', 'hash1');

-- Check if update needed
SELECT
    CASE
        WHEN content_hash != 'hash2' THEN 'needs_update'
        ELSE 'current'
    END as status
FROM fs_embeddings
WHERE inode = 200;
-- Expected: 'needs_update'
```

## Related Files

| File | Description |
|------|-------------|
| `schema/duckagentfs.sql` | Complete DDL |
| `sdk/rust/src/embedding.rs` | Embedding generator trait |
| `sdk/rust/src/filesystem/duckagentfs.rs` | search() implementation |

## Implementation Notes

1. **VSS Extension**: Must be installed before creating HNSW index:
   ```sql
   INSTALL vss;
   LOAD vss;
   ```

2. **Index Performance**: HNSW provides O(log n) search but requires memory. For very large datasets, consider:
   - Partitioning by file type
   - Periodic index rebuilding
   - External vector database (Pinecone, Weaviate)

3. **Dimension Mismatch**: Queries must use same dimension as stored embeddings. Validate at application level.

4. **NULL Handling**: Files without embeddings (binary, too small) should have NULL or no row in fs_embeddings.
