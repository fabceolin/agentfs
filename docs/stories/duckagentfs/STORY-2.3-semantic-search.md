# STORY-2.3: Semantic Search API

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.3 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 2 - Vector Similarity Search |
| **Status** | Partial |
| **Priority** | High |
| **File** | `sdk/rust/src/filesystem/duckagentfs.rs` |
| **Dependencies** | STORY-2.1, STORY-2.2 |

## User Story

**As a** developer
**I want** a semantic search API
**So that** I can find files by meaning

## Technical Description

Semantic search allows finding files based on meaning rather than exact keyword match. For example, searching "meeting notes about project timeline" should find files discussing project schedules, deadlines, and planning - even if they don't contain those exact words.

## Acceptance Criteria

- [x] Method `search(query, limit)` in DuckAgentFS
- [ ] CLI: `agentfs search "query"`
- [ ] Return path, score, preview
- [ ] Support filters (file type, directory, date range)
- [ ] Support chunk-level search

## Technical Specification

### Search Result Type

```rust
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// File path
    pub path: String,

    /// Inode number
    pub inode: i64,

    /// Similarity score (0.0 - 1.0)
    pub score: f32,

    /// Content preview (first ~500 chars)
    pub preview: String,

    /// File stats
    pub stats: Stats,

    /// Chunk info if chunk-level search
    pub chunk: Option<ChunkInfo>,
}

#[derive(Debug, Clone)]
pub struct ChunkInfo {
    pub index: usize,
    pub start_offset: usize,
    pub end_offset: usize,
}
```

### Search Options

```rust
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Maximum results to return
    pub limit: usize,

    /// Minimum similarity score (0.0 - 1.0)
    pub min_score: f32,

    /// Filter by directory (prefix match)
    pub directory: Option<String>,

    /// Filter by file extension
    pub extensions: Option<Vec<String>>,

    /// Filter by modification time (after)
    pub modified_after: Option<i64>,

    /// Filter by modification time (before)
    pub modified_before: Option<i64>,

    /// Enable chunk-level search
    pub search_chunks: bool,
}
```

### DuckAgentFS Search Implementation

```rust
impl DuckAgentFS {
    /// Search files by semantic similarity
    pub async fn search(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if !self.config.enable_vss {
            return Err(Error::Custom("VSS not enabled".into()));
        }

        // Generate query embedding
        let query_embedding = self.embedding_generator.generate(query).await?;

        let conn = self.pool.get_connection().await?;

        // Build query with filters
        let mut sql = String::from(r#"
            SELECT
                t.path,
                t.inode,
                array_cosine_similarity(e.embedding, ?::FLOAT[]) as score,
                COALESCE(
                    LEFT(CAST(d.data AS VARCHAR), 500),
                    ''
                ) as preview,
                t.mode,
                t.size,
                EXTRACT(EPOCH FROM t.mtime)::BIGINT as mtime
            FROM fs_embeddings e
            JOIN fs_tree t ON t.inode = e.inode
            LEFT JOIN fs_data d ON d.inode = e.inode AND d.chunk_idx = 0
            WHERE 1=1
        "#);

        let mut params: Vec<Box<dyn ToSql>> = vec![Box::new(query_embedding)];

        // Apply filters
        if let Some(ref dir) = options.directory {
            sql.push_str(" AND t.path LIKE ?");
            params.push(Box::new(format!("{}%", dir)));
        }

        if let Some(ref exts) = options.extensions {
            let ext_filter: Vec<String> = exts.iter()
                .map(|e| format!("t.path LIKE '%{}'", e))
                .collect();
            sql.push_str(&format!(" AND ({})", ext_filter.join(" OR ")));
        }

        if let Some(after) = options.modified_after {
            sql.push_str(" AND EXTRACT(EPOCH FROM t.mtime) > ?");
            params.push(Box::new(after));
        }

        if let Some(before) = options.modified_before {
            sql.push_str(" AND EXTRACT(EPOCH FROM t.mtime) < ?");
            params.push(Box::new(before));
        }

        if options.min_score > 0.0 {
            sql.push_str(&format!(
                " AND array_cosine_similarity(e.embedding, ?::FLOAT[]) >= {}",
                options.min_score
            ));
        }

        sql.push_str(" ORDER BY score DESC");
        sql.push_str(&format!(" LIMIT {}", options.limit.max(1).min(1000)));

        // Execute query
        let results = conn.query_map(&sql, params.as_slice(), |row| {
            Ok(SearchResult {
                path: row.get(0)?,
                inode: row.get(1)?,
                score: row.get(2)?,
                preview: row.get(3)?,
                stats: Stats {
                    ino: row.get(1)?,
                    mode: row.get(4)?,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    size: row.get(5)?,
                    atime: row.get(6)?,
                    mtime: row.get(6)?,
                    ctime: row.get(6)?,
                },
                chunk: None,
            })
        })?;

        Ok(results)
    }

    /// Search at chunk level for more precise results
    pub async fn search_chunks(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if !self.config.enable_vss {
            return Err(Error::Custom("VSS not enabled".into()));
        }

        let query_embedding = self.embedding_generator.generate(query).await?;
        let conn = self.pool.get_connection().await?;

        let sql = r#"
            SELECT
                t.path,
                t.inode,
                c.chunk_idx,
                c.start_offset,
                c.end_offset,
                array_cosine_similarity(c.embedding, ?::FLOAT[]) as score,
                c.content_preview
            FROM fs_chunk_embeddings c
            JOIN fs_tree t ON t.inode = c.inode
            ORDER BY score DESC
            LIMIT ?
        "#;

        let results = conn.query_map(sql, params![query_embedding, options.limit], |row| {
            Ok(SearchResult {
                path: row.get(0)?,
                inode: row.get(1)?,
                score: row.get(5)?,
                preview: row.get(6)?,
                stats: Stats::default(), // Would need join for full stats
                chunk: Some(ChunkInfo {
                    index: row.get(2)?,
                    start_offset: row.get(3)?,
                    end_offset: row.get(4)?,
                }),
            })
        })?;

        Ok(results)
    }
}
```

### CLI Implementation

```rust
// cli/src/cmd/search.rs

pub async fn handle_search_command(
    id_or_path: String,
    query: String,
    limit: usize,
    directory: Option<String>,
    extensions: Option<Vec<String>>,
    chunks: bool,
) -> Result<()> {
    let fs = open_duckagentfs(&id_or_path).await?;

    let options = SearchOptions {
        limit,
        directory,
        extensions,
        search_chunks: chunks,
        ..Default::default()
    };

    let results = if chunks {
        fs.search_chunks(&query, options).await?
    } else {
        fs.search(&query, options).await?
    };

    // Display results
    for result in results {
        println!("{} (score: {:.3})", result.path, result.score);
        if !result.preview.is_empty() {
            let preview = result.preview
                .chars()
                .take(100)
                .collect::<String>();
            println!("  {}", preview);
        }
        if let Some(chunk) = &result.chunk {
            println!("  [chunk {} @ {}..{}]",
                chunk.index, chunk.start_offset, chunk.end_offset);
        }
        println!();
    }

    Ok(())
}
```

### CLI Usage

```bash
# Basic search
agentfs search my-agent "meeting notes about project"

# With limit
agentfs search my-agent "API documentation" --limit 5

# Filter by directory
agentfs search my-agent "test cases" --dir /src/tests

# Filter by extension
agentfs search my-agent "configuration" --ext .json --ext .yaml

# Chunk-level search
agentfs search my-agent "error handling" --chunks

# Combined
agentfs search my-agent "database schema" \
    --dir /docs \
    --ext .md \
    --limit 10 \
    --min-score 0.7
```

## Tests

### Test 1: Basic Search
```rust
#[tokio::test]
async fn test_basic_search() {
    let fs = setup_test_fs().await;

    // Create files with different content
    fs.write_file("/notes/meeting.txt", b"Discussed project timeline and deadlines").await.unwrap();
    fs.write_file("/notes/todo.txt", b"Buy groceries and milk").await.unwrap();
    fs.write_file("/docs/api.md", b"REST API documentation").await.unwrap();

    // Wait for embeddings to be generated
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Search for project-related content
    let results = fs.search("project schedule", SearchOptions {
        limit: 10,
        ..Default::default()
    }).await.unwrap();

    // meeting.txt should rank higher than groceries
    assert!(!results.is_empty());
    assert!(results[0].path.contains("meeting"));
}
```

### Test 2: Directory Filter
```rust
#[tokio::test]
async fn test_directory_filter() {
    let fs = setup_test_fs().await;

    fs.write_file("/src/main.rs", b"fn main() {}").await.unwrap();
    fs.write_file("/docs/readme.md", b"Main documentation").await.unwrap();

    let results = fs.search("main", SearchOptions {
        limit: 10,
        directory: Some("/docs".to_string()),
        ..Default::default()
    }).await.unwrap();

    // Should only return files under /docs
    assert!(results.iter().all(|r| r.path.starts_with("/docs")));
}
```

### Test 3: Extension Filter
```rust
#[tokio::test]
async fn test_extension_filter() {
    let fs = setup_test_fs().await;

    fs.write_file("/config.json", b"{}").await.unwrap();
    fs.write_file("/config.yaml", b"key: value").await.unwrap();
    fs.write_file("/config.txt", b"config").await.unwrap();

    let results = fs.search("configuration", SearchOptions {
        limit: 10,
        extensions: Some(vec![".json".to_string(), ".yaml".to_string()]),
        ..Default::default()
    }).await.unwrap();

    // Should not include .txt
    assert!(results.iter().all(|r| !r.path.ends_with(".txt")));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/filesystem/duckagentfs.rs` | search() implementation |
| `sdk/rust/src/embedding.rs` | Query embedding generation |
| `schema/duckagentfs.sql` | fs_embeddings tables |
| `cli/src/cmd/search.rs` | CLI command (new) |
| `cli/src/parser.rs` | CLI args parsing |

## Implementation Notes

1. **Query Embedding**: Use same model for queries as for indexing to ensure compatibility.

2. **Performance**: For large datasets:
   - Use HNSW index (requires VSS extension)
   - Limit result set before joining with fs_tree
   - Cache frequent queries

3. **Relevance Tuning**: Consider:
   - Boosting recent files
   - Boosting files in active directories
   - Re-ranking based on file type

4. **Hybrid Search**: Combine with keyword search:
   ```sql
   -- Keyword + semantic hybrid
   SELECT path,
          0.5 * keyword_score + 0.5 * semantic_score as combined_score
   FROM (
       SELECT path,
              CASE WHEN content LIKE '%keyword%' THEN 1.0 ELSE 0.0 END as keyword_score,
              array_cosine_similarity(embedding, ?) as semantic_score
       FROM ...
   )
   ORDER BY combined_score DESC
   ```
