# STORY-5.3: DefaultHandler

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-5.3 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 5 - FUSE + Handler Registry |
| **Status** | Done |
| **Priority** | High |
| **File** | `cli/src/handler.rs` |
| **Dependencies** | STORY-5.1 |

## User Story

**As a** developer
**I want** a default handler
**So that** unhandled operations delegate to the FileSystem

## Acceptance Criteria

- [x] Implements FileHandler trait
- [x] Delegates all operations to FileSystem
- [x] Maximum priority (always last)
- [x] Accepts all paths

## Technical Specification

### Implementation

```rust
/// Default handler that delegates to the underlying FileSystem.
///
/// This handler always accepts all paths and delegates to the filesystem
/// implementation. It's used as the fallback when no custom handler matches.
pub struct DefaultHandler {
    fs: Arc<dyn FileSystem>,
}

impl DefaultHandler {
    /// Create a new default handler wrapping a filesystem.
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }
}

#[async_trait]
impl FileHandler for DefaultHandler {
    fn name(&self) -> &str {
        "default"
    }

    fn priority(&self) -> u32 {
        u32::MAX // Always last
    }

    fn can_handle(&self, _path: &str, _stats: Option<&Stats>) -> bool {
        true // Accept everything
    }

    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        match self.fs.open(path).await {
            Ok(file) => {
                let data = file.pread(offset, size).await?;
                Ok(Some(data))
            }
            Err(e) => Err(e),
        }
    }

    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        match self.fs.stat(path).await? {
            Some(stats) => Ok(Some(stats)),
            None => Ok(None),
        }
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        self.fs.readdir(path).await
    }

    async fn readdir_plus(&self, path: &str) -> HandlerResult<Vec<DirEntry>> {
        self.fs.readdir_plus(path).await
    }

    async fn write(&self, path: &str, offset: u64, data: &[u8]) -> HandlerResult<usize> {
        match self.fs.open(path).await {
            Ok(file) => {
                file.pwrite(offset, data).await?;
                Ok(Some(data.len()))
            }
            Err(e) => Err(e),
        }
    }

    async fn truncate(&self, path: &str, size: u64) -> HandlerResult<()> {
        match self.fs.open(path).await {
            Ok(file) => {
                file.truncate(size).await?;
                Ok(Some(()))
            }
            Err(e) => Err(e),
        }
    }

    async fn readlink(&self, path: &str) -> HandlerResult<String> {
        self.fs.readlink(path).await
    }
}
```

### Usage

```rust
// DefaultHandler is typically created via HandlerRegistry
let fs = Arc::new(DuckAgentFS::open(config).await?);
let registry = HandlerRegistry::with_filesystem(fs);
// DefaultHandler is automatically created and set as fallback

// Or create manually
let default = DefaultHandler::new(fs.clone());
assert_eq!(default.priority(), u32::MAX);
assert!(default.can_handle("/any/path", None));
```

## Behavior

| Operation | Behavior |
|-----------|----------|
| `read` | Opens file, calls `pread` |
| `getattr` | Calls `stat`, returns None if not found |
| `readdir` | Delegates to `fs.readdir` |
| `readdir_plus` | Delegates to `fs.readdir_plus` |
| `write` | Opens file, calls `pwrite` |
| `truncate` | Opens file, calls `truncate` |
| `readlink` | Delegates to `fs.readlink` |

## Tests

### Test 1: Accepts All Paths
```rust
#[test]
fn test_accepts_all() {
    let handler = DefaultHandler::new(mock_fs());

    assert!(handler.can_handle("/any/path", None));
    assert!(handler.can_handle("/", None));
    assert!(handler.can_handle("/deeply/nested/path/file.txt", None));
}
```

### Test 2: Maximum Priority
```rust
#[test]
fn test_priority() {
    let handler = DefaultHandler::new(mock_fs());
    assert_eq!(handler.priority(), u32::MAX);
}
```

### Test 3: Read Delegation
```rust
#[tokio::test]
async fn test_read_delegation() {
    let fs = mock_fs_with_file("/test.txt", b"hello world");
    let handler = DefaultHandler::new(fs);

    let result = handler.read("/test.txt", 0, 11).await.unwrap();
    assert_eq!(result, Some(b"hello world".to_vec()));
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/handler.rs` | DefaultHandler implementation |
| `sdk/rust/src/filesystem/mod.rs` | FileSystem trait |
