# STORY-4.2: Virtual Directory Listing

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-4.2 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 4 - FUSE Handler |
| **Status** | Ready for Development |
| **Priority** | Medium |
| **File** | `cli/src/handler.rs` |
| **Dependencies** | STORY-4.1 |

## User Story

**As a** user
**I want** to see GraphDocs documents in `ls`
**So that** I can discover available documents

## Acceptance Criteria

- [ ] Handler returns list of documents in `readdir()`
- [ ] Documents appear as files with `.gd.md` extension
- [ ] Stats reflect rendered size

## Technical Specification

### Virtual Directory Strategy

GraphDocs can appear in a virtual directory (e.g., `/.graphdocs/`) or be mixed with real files. This story implements a dedicated virtual directory.

```
/mnt/agent/
├── .graphdocs/                  # Virtual directory
│   ├── readme.gd.md            # Rendered from gd_documents id='readme'
│   ├── api-reference.gd.md     # Rendered from gd_documents id='api-reference'
│   └── getting-started.gd.md   # Rendered from gd_documents id='getting-started'
├── src/
│   └── main.rs                  # Real file
└── README.md                    # Real file
```

### Handler Extension

```rust
// cli/src/handler.rs

/// Path for virtual GraphDocs directory
const GRAPHDOCS_DIR: &str = "/.graphdocs";

impl GraphDocsHandler {
    /// Check if path is the GraphDocs virtual directory
    fn is_graphdocs_dir(path: &str) -> bool {
        path == GRAPHDOCS_DIR || path == &format!("{}/", GRAPHDOCS_DIR)
    }

    /// Check if path is inside GraphDocs directory
    fn is_in_graphdocs_dir(path: &str) -> bool {
        path.starts_with(&format!("{}/", GRAPHDOCS_DIR))
    }

    /// List all documents in the database
    async fn list_documents(&self) -> Result<Vec<DocumentInfo>, Error> {
        let conn = self.engine.pool().get_read_connection().await?;

        let mut stmt = conn.prepare(r#"
            SELECT id, title, updated_at
            FROM gd_documents
            ORDER BY id
        "#)?;

        let docs = stmt.query_map([], |row| {
            Ok(DocumentInfo {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

        Ok(docs)
    }
}

#[derive(Debug)]
struct DocumentInfo {
    id: String,
    title: String,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
impl FileHandler for GraphDocsHandler {
    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        Self::is_graphdocs_path(path) ||
        Self::is_graphdocs_dir(path) ||
        Self::is_in_graphdocs_dir(path)
    }

    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        // Handle virtual directory
        if Self::is_graphdocs_dir(path) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            return Ok(Some(Stats {
                ino: 0,
                mode: 0o40555, // Directory, read-only + execute
                nlink: 2,
                uid: 0,
                gid: 0,
                size: 0,
                atime: now,
                mtime: now,
                ctime: now,
            }));
        }

        // Handle files in graphdocs dir
        if Self::is_in_graphdocs_dir(path) {
            let doc_id = Self::extract_doc_id_from_dir_path(path);
            if let Some(id) = doc_id {
                return self.getattr_for_doc(&id).await;
            }
        }

        // Handle .gd.md files elsewhere
        if let Some(doc_id) = Self::extract_doc_id(path) {
            return self.getattr_for_doc(&doc_id).await;
        }

        Ok(None)
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        if !Self::is_graphdocs_dir(path) {
            return Ok(None);
        }

        // List all documents
        let docs = self.list_documents().await?;

        let mut entries = vec![".".to_string(), "..".to_string()];
        for doc in docs {
            entries.push(format!("{}.gd.md", doc.id));
        }

        Ok(Some(entries))
    }

    async fn readdir_plus(&self, path: &str) -> HandlerResult<Vec<DirEntry>> {
        if !Self::is_graphdocs_dir(path) {
            return Ok(None);
        }

        let docs = self.list_documents().await?;
        let mut entries = Vec::new();

        // Add . and ..
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        entries.push(DirEntry {
            name: ".".to_string(),
            stats: Stats {
                ino: 0,
                mode: 0o40555,
                nlink: 2,
                uid: 0,
                gid: 0,
                size: 0,
                atime: now,
                mtime: now,
                ctime: now,
            },
        });

        entries.push(DirEntry {
            name: "..".to_string(),
            stats: Stats {
                ino: 0,
                mode: 0o40755,
                nlink: 2,
                uid: 0,
                gid: 0,
                size: 0,
                atime: now,
                mtime: now,
                ctime: now,
            },
        });

        // Add documents
        for doc in docs {
            // Get rendered size
            let content = self.get_content(&doc.id).await.unwrap_or_default();
            let mtime = doc.updated_at.timestamp();

            entries.push(DirEntry {
                name: format!("{}.gd.md", doc.id),
                stats: Stats {
                    ino: 0,
                    mode: 0o100444, // Regular file, read-only
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    size: content.len() as i64,
                    atime: mtime,
                    mtime,
                    ctime: mtime,
                },
            });
        }

        Ok(Some(entries))
    }
}

impl GraphDocsHandler {
    /// Extract doc_id from path like "/.graphdocs/readme.gd.md" -> "readme"
    fn extract_doc_id_from_dir_path(path: &str) -> Option<String> {
        let path = path.strip_prefix(GRAPHDOCS_DIR)?;
        let path = path.strip_prefix('/')?;
        Self::extract_doc_id(&format!("/{}", path))
    }

    async fn getattr_for_doc(&self, doc_id: &str) -> HandlerResult<Stats> {
        match self.get_content(doc_id).await {
            Ok(content) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                Ok(Some(Stats {
                    ino: 0,
                    mode: 0o100444,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    size: content.len() as i64,
                    atime: now,
                    mtime: now,
                    ctime: now,
                }))
            }
            Err(_) => Ok(None),
        }
    }
}
```

### Root Directory Integration

To make `.graphdocs` appear in root directory listings, we need to integrate with the DefaultHandler or modify how root readdir works.

```rust
// cli/src/handler.rs

/// Handler that injects .graphdocs into root directory
pub struct GraphDocsDirInjector {
    inner: Arc<dyn FileHandler>,
}

impl GraphDocsDirInjector {
    pub fn new(inner: Arc<dyn FileHandler>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl FileHandler for GraphDocsDirInjector {
    fn name(&self) -> &str {
        "graphdocs-injector"
    }

    fn priority(&self) -> u32 {
        5 // Before GraphDocsHandler
    }

    fn can_handle(&self, path: &str, stats: Option<&Stats>) -> bool {
        path == "/" // Only handle root
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        if path != "/" {
            return Ok(None);
        }

        // Get real entries from inner handler
        let mut entries = self.inner.readdir(path).await?
            .unwrap_or_default();

        // Inject .graphdocs if not already present
        if !entries.contains(&".graphdocs".to_string()) {
            entries.push(".graphdocs".to_string());
        }

        Ok(Some(entries))
    }

    // Delegate other operations
    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        Ok(None)
    }

    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        Ok(None)
    }

    async fn write(&self, path: &str, offset: u64, data: &[u8]) -> HandlerResult<usize> {
        Ok(None)
    }

    async fn truncate(&self, path: &str, size: u64) -> HandlerResult<()> {
        Ok(None)
    }

    async fn readlink(&self, path: &str) -> HandlerResult<String> {
        Ok(None)
    }
}
```

### CLI Usage

```bash
# List GraphDocs
ls /.graphdocs/
readme.gd.md
api-reference.gd.md
getting-started.gd.md

# With details
ls -la /.graphdocs/
total 0
dr-xr-xr-x 2 root root    0 Jan 15 10:00 .
drwxr-xr-x 5 root root 4096 Jan 15 10:00 ..
-r--r--r-- 1 root root 2048 Jan 15 10:00 readme.gd.md
-r--r--r-- 1 root root 4096 Jan 15 10:00 api-reference.gd.md
-r--r--r-- 1 root root 1024 Jan 15 10:00 getting-started.gd.md

# Read a document
cat /.graphdocs/readme.gd.md

# Tab completion works
cat /.graphdocs/rea<TAB>
cat /.graphdocs/readme.gd.md
```

## Tests

### Test 1: Virtual Directory Exists
```rust
#[tokio::test]
async fn test_graphdocs_dir_exists() {
    let handler = setup_handler().await;

    let stats = handler.getattr("/.graphdocs").await.unwrap();
    assert!(stats.is_some());

    let stats = stats.unwrap();
    assert!(stats.mode & 0o40000 != 0); // Is directory
    assert!(stats.mode & 0o555 != 0);   // Read + execute
}
```

### Test 2: List Documents
```rust
#[tokio::test]
async fn test_list_documents() {
    let handler = setup_handler_with_docs(&["readme", "api"]).await;

    let entries = handler.readdir("/.graphdocs").await.unwrap();
    assert!(entries.is_some());

    let entries = entries.unwrap();
    assert!(entries.contains(&"readme.gd.md".to_string()));
    assert!(entries.contains(&"api.gd.md".to_string()));
}
```

### Test 3: File Stats Show Size
```rust
#[tokio::test]
async fn test_file_stats() {
    let handler = setup_handler_with_doc("test", "# Hello World").await;

    let stats = handler.getattr("/.graphdocs/test.gd.md").await.unwrap();
    assert!(stats.is_some());

    let stats = stats.unwrap();
    assert!(stats.size > 0);
    assert!(stats.mode & 0o100000 != 0); // Is regular file
    assert!(stats.mode & 0o444 != 0);    // Read permission
}
```

### Test 4: Read From Virtual Dir
```rust
#[tokio::test]
async fn test_read_from_virtual_dir() {
    let handler = setup_handler_with_doc("test", "# Test").await;

    let content = handler.read("/.graphdocs/test.gd.md", 0, 1000).await.unwrap();
    assert!(content.is_some());

    let text = String::from_utf8(content.unwrap()).unwrap();
    assert!(text.contains("Test"));
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/handler.rs` | Handler with directory support |
| `cli/src/fuse.rs` | FUSE integration |

## Implementation Notes

1. **Virtual Directory**: `.graphdocs` doesn't exist on disk, it's synthesized by the handler
2. **Inode Assignment**: FUSE assigns inodes; we return 0 and let FUSE handle it
3. **Permissions**: Directory is 0555 (r-xr-xr-x), files are 0444 (r--r--r--)
4. **Size Calculation**: File size is determined by rendering (cached)
5. **Timestamps**: Use document's updated_at for mtime
