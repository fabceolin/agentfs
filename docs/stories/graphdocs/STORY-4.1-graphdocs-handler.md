# STORY-4.1: GraphDocsHandler

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-4.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 4 - FUSE Handler |
| **Status** | Done |
| **Priority** | High |
| **File** | `cli/src/handler.rs` |
| **Dependencies** | STORY-3.1, EPIC-DUCKAGENTFS-001 (STORY-5.1) |

## User Story

**As a** developer
**I want** a FUSE handler for GraphDocs
**So that** documents are rendered transparently when accessed

## Acceptance Criteria

- [x] Implements `FileHandler` trait
- [x] Intercepts files with `.gd.md` extension
- [x] Renders document from graph on `read()`
- [x] Returns virtual stats on `getattr()`
- [x] Write returns error (read-only)

## Technical Specification

### Handler Implementation

```rust
// cli/src/handler.rs

use crate::graphdocs::engine::GraphDocsEngine;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handler for GraphDocs (.gd.md) files
pub struct GraphDocsHandler {
    engine: Arc<GraphDocsEngine>,
    /// Cache of rendered documents: path -> (content, timestamp)
    cache: RwLock<HashMap<String, CachedDoc>>,
    cache_ttl: Duration,
}

struct CachedDoc {
    content: Vec<u8>,
    rendered_at: Instant,
}

impl GraphDocsHandler {
    pub fn new(engine: Arc<GraphDocsEngine>) -> Self {
        Self {
            engine,
            cache: RwLock::new(HashMap::new()),
            cache_ttl: Duration::from_secs(5), // 5 second cache
        }
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// Extract document ID from path
    /// "/docs/my-doc.gd.md" -> "my-doc"
    fn extract_doc_id(path: &str) -> Option<String> {
        let path = Path::new(path);
        let file_name = path.file_name()?.to_str()?;

        if !file_name.ends_with(".gd.md") {
            return None;
        }

        // Remove .gd.md extension
        let doc_id = file_name.strip_suffix(".gd.md")?;
        Some(doc_id.to_string())
    }

    /// Check if path matches GraphDocs pattern
    fn is_graphdocs_path(path: &str) -> bool {
        path.ends_with(".gd.md")
    }

    /// Get or render document content
    async fn get_content(&self, doc_id: &str) -> Result<Vec<u8>, Error> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(doc_id) {
                if cached.rendered_at.elapsed() < self.cache_ttl {
                    return Ok(cached.content.clone());
                }
            }
        }

        // Render document
        let markdown = self.engine.render(doc_id).await?;
        let content = markdown.into_bytes();

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(doc_id.to_string(), CachedDoc {
                content: content.clone(),
                rendered_at: Instant::now(),
            });
        }

        Ok(content)
    }

    /// Invalidate cache for a document
    pub async fn invalidate(&self, doc_id: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(doc_id);
    }

    /// Clear entire cache
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

#[async_trait]
impl FileHandler for GraphDocsHandler {
    fn name(&self) -> &str {
        "graphdocs"
    }

    fn priority(&self) -> u32 {
        10 // High priority, before DefaultHandler
    }

    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        Self::is_graphdocs_path(path)
    }

    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        let doc_id = match Self::extract_doc_id(path) {
            Some(id) => id,
            None => return Ok(None), // Let another handler try
        };

        match self.get_content(&doc_id).await {
            Ok(content) => {
                // Handle offset and size
                let start = (offset as usize).min(content.len());
                let end = ((offset + size) as usize).min(content.len());
                Ok(Some(content[start..end].to_vec()))
            }
            Err(e) => {
                tracing::warn!("Failed to render GraphDoc {}: {}", doc_id, e);
                Err(e)
            }
        }
    }

    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        let doc_id = match Self::extract_doc_id(path) {
            Some(id) => id,
            None => return Ok(None),
        };

        // Try to get content (cached) to determine size
        match self.get_content(&doc_id).await {
            Ok(content) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                Ok(Some(Stats {
                    ino: 0, // FUSE will assign
                    mode: 0o100444, // Regular file, read-only
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    size: content.len() as i64,
                    atime: now,
                    mtime: now,
                    ctime: now,
                }))
            }
            Err(_) => Ok(None), // Document doesn't exist
        }
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        // GraphDocsHandler doesn't handle directories
        // See STORY-4.2 for directory listing
        Ok(None)
    }

    async fn readdir_plus(&self, path: &str) -> HandlerResult<Vec<DirEntry>> {
        Ok(None)
    }

    async fn write(&self, path: &str, _offset: u64, _data: &[u8]) -> HandlerResult<usize> {
        if Self::is_graphdocs_path(path) {
            // GraphDocs are read-only via FUSE
            // Edit via graphdocs CLI commands instead
            Err(Error::Custom(
                "GraphDocs are read-only. Use 'agentfs graphdocs' CLI to edit.".to_string()
            ))
        } else {
            Ok(None)
        }
    }

    async fn truncate(&self, path: &str, _size: u64) -> HandlerResult<()> {
        if Self::is_graphdocs_path(path) {
            Err(Error::Custom("GraphDocs are read-only".to_string()))
        } else {
            Ok(None)
        }
    }

    async fn readlink(&self, _path: &str) -> HandlerResult<String> {
        Ok(None) // GraphDocs are never symlinks
    }
}
```

### Integration with FUSE

```rust
// cli/src/cmd/mount.rs

pub async fn handle_mount(args: MountArgs) -> Result<()> {
    let fs = open_filesystem(&args.id_or_path).await?;

    // Create handler registry
    let mut registry = HandlerRegistry::with_filesystem(fs.clone());

    // Register GraphDocs handler if enabled
    if args.enable_graphdocs {
        let pool = get_duckdb_pool(&args.id_or_path).await?;
        let engine = Arc::new(GraphDocsEngine::new(pool));
        let handler = GraphDocsHandler::new(engine);

        if let Some(ttl) = args.graphdocs_cache_ttl {
            handler = handler.with_cache_ttl(Duration::from_secs(ttl));
        }

        registry.register(Arc::new(handler));
        tracing::info!("GraphDocs handler enabled");
    }

    // Create FUSE with registry
    let fuse = AgentFSFuse::new(fs, &options, Some(Arc::new(registry)));

    // Mount...
    Ok(())
}
```

### Mount Command Extension

```rust
// cli/src/parser.rs

#[derive(Args)]
pub struct MountArgs {
    /// Agent ID or database path
    pub id_or_path: String,

    /// Mount point
    pub mountpoint: PathBuf,

    /// Enable GraphDocs handler for .gd.md files
    #[clap(long)]
    pub enable_graphdocs: bool,

    /// GraphDocs cache TTL in seconds
    #[clap(long)]
    pub graphdocs_cache_ttl: Option<u64>,

    // ... other mount options
}
```

### Usage

```bash
# Mount with GraphDocs enabled
agentfs mount my-agent /mnt/agent --enable-graphdocs

# Mount with custom cache TTL
agentfs mount my-agent /mnt/agent --enable-graphdocs --graphdocs-cache-ttl 30

# Access rendered documents
cat /mnt/agent/docs/readme.gd.md
# Output: Rendered markdown with variables substituted

# Attempt to write (will fail)
echo "test" > /mnt/agent/docs/readme.gd.md
# Error: GraphDocs are read-only. Use 'agentfs graphdocs' CLI to edit.
```

### Rendering Flow

```
1. User: cat /mnt/agent/docs/project.gd.md
              |
              v
2. FUSE: read("/docs/project.gd.md", offset=0, size=4096)
              |
              v
3. HandlerRegistry.handle_read()
              |
              v
4. GraphDocsHandler.can_handle("/docs/project.gd.md") -> true
              |
              v
5. GraphDocsHandler.read()
              |
              +-- Extract doc_id: "project"
              |
              +-- Check cache
              |     |
              |     +-- Cache hit & valid? -> Return cached content
              |     |
              |     +-- Cache miss/expired?
              |              |
              |              v
              +-- engine.render("project")
              |     |
              |     +-- Resolve inheritance
              |     +-- Load sections
              |     +-- Load variables
              |     +-- Substitute {{variables}}
              |     +-- Generate markdown
              |              |
              |              v
              +-- Update cache
              |
              v
6. Return markdown bytes
```

## Tests

### Test 1: Can Handle GraphDocs Path
```rust
#[test]
fn test_can_handle() {
    let handler = GraphDocsHandler::new(mock_engine());

    assert!(handler.can_handle("/docs/readme.gd.md", None));
    assert!(handler.can_handle("/project.gd.md", None));
    assert!(!handler.can_handle("/readme.md", None));
    assert!(!handler.can_handle("/file.txt", None));
}
```

### Test 2: Extract Document ID
```rust
#[test]
fn test_extract_doc_id() {
    assert_eq!(
        GraphDocsHandler::extract_doc_id("/docs/readme.gd.md"),
        Some("readme".to_string())
    );
    assert_eq!(
        GraphDocsHandler::extract_doc_id("/my-project.gd.md"),
        Some("my-project".to_string())
    );
    assert_eq!(
        GraphDocsHandler::extract_doc_id("/readme.md"),
        None
    );
}
```

### Test 3: Read Returns Rendered Content
```rust
#[tokio::test]
async fn test_read_renders_content() {
    let engine = setup_engine_with_doc("test", "# Hello {{name}}").await;
    engine.set_variable("test", "name", json!("World")).await.unwrap();

    let handler = GraphDocsHandler::new(Arc::new(engine));
    let result = handler.read("/test.gd.md", 0, 1000).await.unwrap();

    let content = String::from_utf8(result.unwrap()).unwrap();
    assert!(content.contains("Hello World"));
}
```

### Test 4: Write Returns Error
```rust
#[tokio::test]
async fn test_write_returns_error() {
    let handler = GraphDocsHandler::new(mock_engine());
    let result = handler.write("/test.gd.md", 0, b"data").await;

    assert!(result.is_err());
}
```

### Test 5: Cache Works
```rust
#[tokio::test]
async fn test_cache() {
    let engine = setup_engine_with_doc("test", "# Cached").await;
    let handler = GraphDocsHandler::new(Arc::new(engine))
        .with_cache_ttl(Duration::from_secs(60));

    // First read populates cache
    let _ = handler.read("/test.gd.md", 0, 1000).await.unwrap();

    // Second read should use cache (verify via mock)
    let _ = handler.read("/test.gd.md", 0, 1000).await.unwrap();

    // Verify engine.render was called only once
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/handler.rs` | GraphDocsHandler implementation |
| `sdk/rust/src/graphdocs/engine.rs` | Rendering engine |
| `cli/src/fuse.rs` | FUSE integration |
| `cli/src/cmd/mount.rs` | Mount command |

## Implementation Notes

1. **Read-Only**: GraphDocs via FUSE are read-only; use CLI to edit
2. **Caching**: Short TTL cache prevents repeated renders on rapid reads
3. **Priority**: High priority (10) ensures GraphDocs are checked before DefaultHandler
4. **Error Handling**: Missing documents return None, letting other handlers try
