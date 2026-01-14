# STORY-5.2: HandlerRegistry

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-5.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 5 - FUSE + Handler Registry |
| **Status** | Done |
| **Priority** | High |
| **File** | `cli/src/handler.rs` |
| **Dependencies** | STORY-5.1 |

## User Story

**As a** developer
**I want** a registry of handlers
**So that** I can manage multiple handlers by priority

## Acceptance Criteria

- [x] Dynamic registration/unregistration
- [x] Automatic sorting by priority
- [x] Fallback to DefaultHandler
- [x] Dispatch methods for all FUSE operations

## Technical Specification

### HandlerRegistry Structure

```rust
pub struct HandlerRegistry {
    handlers: Vec<Arc<dyn FileHandler>>,
    default_handler: Arc<dyn FileHandler>,
}

impl HandlerRegistry {
    /// Create with default handler
    pub fn new(default_handler: Arc<dyn FileHandler>) -> Self {
        Self {
            handlers: Vec::new(),
            default_handler,
        }
    }

    /// Create with filesystem as default
    pub fn with_filesystem(fs: Arc<dyn FileSystem>) -> Self {
        Self::new(Arc::new(DefaultHandler::new(fs)))
    }

    /// Register a handler (auto-sorts by priority)
    pub fn register(&mut self, handler: Arc<dyn FileHandler>) {
        self.handlers.push(handler);
        self.handlers.sort_by_key(|h| h.priority());
    }

    /// Unregister by name
    pub fn unregister(&mut self, name: &str) {
        self.handlers.retain(|h| h.name() != name);
    }

    /// List handlers in priority order
    pub fn list_handlers(&self) -> Vec<(&str, u32)> {
        self.handlers.iter()
            .map(|h| (h.name(), h.priority()))
            .collect()
    }
}
```

### Operation Dispatch

```rust
impl HandlerRegistry {
    /// Dispatch read operation
    pub async fn handle_read(&self, path: &str, offset: u64, size: u64) -> Result<Vec<u8>> {
        // Try custom handlers first
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(data) = handler.read(path, offset, size).await? {
                    tracing::debug!("Handler '{}' handled read for {}", handler.name(), path);
                    return Ok(data);
                }
            }
        }

        // Fall back to default
        self.default_handler.read(path, offset, size).await?
            .ok_or_else(|| Error::Custom(format!("No handler for: {}", path)))
    }

    /// Dispatch getattr operation
    pub async fn handle_getattr(&self, path: &str) -> Result<Option<Stats>> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(stats) = handler.getattr(path).await? {
                    return Ok(Some(stats));
                }
            }
        }
        self.default_handler.getattr(path).await
    }

    /// Dispatch readdir operation
    pub async fn handle_readdir(&self, path: &str) -> Result<Option<Vec<String>>> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(entries) = handler.readdir(path).await? {
                    return Ok(Some(entries));
                }
            }
        }
        self.default_handler.readdir(path).await
    }

    /// Dispatch write operation
    pub async fn handle_write(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(written) = handler.write(path, offset, data).await? {
                    return Ok(written);
                }
            }
        }
        self.default_handler.write(path, offset, data).await?
            .ok_or_else(|| Error::Custom(format!("No handler for write: {}", path)))
    }

    /// Dispatch truncate operation
    pub async fn handle_truncate(&self, path: &str, size: u64) -> Result<()> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if handler.truncate(path, size).await?.is_some() {
                    return Ok(());
                }
            }
        }
        self.default_handler.truncate(path, size).await?
            .ok_or_else(|| Error::Custom(format!("No handler for truncate: {}", path)))
    }

    /// Dispatch readlink operation
    pub async fn handle_readlink(&self, path: &str) -> Result<Option<String>> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(target) = handler.readlink(path).await? {
                    return Ok(Some(target));
                }
            }
        }
        self.default_handler.readlink(path).await
    }
}
```

### Usage Example

```rust
// Create registry with filesystem backend
let fs = Arc::new(DuckAgentFS::open(config).await?);
let mut registry = HandlerRegistry::with_filesystem(fs.clone());

// Register custom handlers
registry.register(Arc::new(GraphDocsHandler::new()));
registry.register(Arc::new(ReadmeHandler::new()));

// Use in FUSE
let data = registry.handle_read("/docs/readme.gd.md", 0, 4096).await?;
```

## Flow Diagram

```
FUSE read("/path/to/file")
        │
        ▼
HandlerRegistry.handle_read()
        │
        ├─ Handler 1: can_handle("/path/to/file")?
        │     ├─ true → read() → Some(data)? → return data
        │     └─ false → continue
        │
        ├─ Handler 2: can_handle("/path/to/file")?
        │     ├─ true → read() → None? → continue
        │     └─ false → continue
        │
        └─ DefaultHandler.read() → return data
```

## Tests

### Test 1: Handler Registration Order
```rust
#[test]
fn test_handler_order() {
    let default = Arc::new(DefaultHandler::new(mock_fs()));
    let mut registry = HandlerRegistry::new(default);

    registry.register(Arc::new(LowPriorityHandler));  // priority 100
    registry.register(Arc::new(HighPriorityHandler)); // priority 10

    let handlers = registry.list_handlers();
    assert_eq!(handlers[0].0, "high");
    assert_eq!(handlers[1].0, "low");
}
```

### Test 2: Handler Fallthrough
```rust
#[tokio::test]
async fn test_handler_fallthrough() {
    let fs = mock_fs_with_file("/test.txt", b"content");
    let mut registry = HandlerRegistry::with_filesystem(fs);

    // Handler that declines all requests
    registry.register(Arc::new(DeclineAllHandler));

    // Should fall through to default
    let data = registry.handle_read("/test.txt", 0, 100).await.unwrap();
    assert_eq!(data, b"content");
}
```

### Test 3: Handler Intercept
```rust
#[tokio::test]
async fn test_handler_intercept() {
    let fs = mock_fs_with_file("/test.txt", b"original");
    let mut registry = HandlerRegistry::with_filesystem(fs);

    // Handler that intercepts .txt files
    registry.register(Arc::new(TxtInterceptHandler)); // Returns "intercepted"

    let data = registry.handle_read("/test.txt", 0, 100).await.unwrap();
    assert_eq!(String::from_utf8(data).unwrap(), "intercepted");
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/handler.rs` | HandlerRegistry implementation |
| `cli/src/fuse.rs` | FUSE integration |
