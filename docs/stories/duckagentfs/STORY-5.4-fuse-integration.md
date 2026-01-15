# STORY-5.4: FUSE Integration

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-5.4 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 5 - FUSE + Handler Registry |
| **Status** | Ready for Development |
| **Priority** | High |
| **File** | `cli/src/fuse.rs` |
| **Dependencies** | STORY-5.1, STORY-5.2, STORY-5.3 |

## User Story

**As a** developer
**I want** HandlerRegistry integrated into fuse.rs
**So that** handlers can intercept FUSE operations

## Acceptance Criteria

- [ ] Add `handler_registry` field to `AgentFSFuse`
- [ ] Modify `read()` to use handler_registry
- [ ] Modify `getattr()` to use handler_registry
- [ ] Modify other operations as needed
- [ ] Integration tests

## Technical Specification

### Modified AgentFSFuse Structure

```rust
// cli/src/fuse.rs

use crate::handler::{HandlerRegistry, DefaultHandler};

struct AgentFSFuse {
    fs: Arc<dyn FileSystem>,
    runtime: Runtime,
    path_cache: Arc<Mutex<HashMap<u64, String>>>,
    open_files: Arc<Mutex<HashMap<u64, OpenFile>>>,
    next_fh: AtomicU64,
    uid: u32,
    gid: u32,
    mountpoint_path: String,

    // NEW: Handler registry
    handler_registry: Arc<HandlerRegistry>,
}
```

### Constructor Changes

```rust
impl AgentFSFuse {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        options: &FuseMountOptions,
        handler_registry: Option<Arc<HandlerRegistry>>,
    ) -> Self {
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        // Use provided registry or create default
        let handler_registry = handler_registry.unwrap_or_else(|| {
            Arc::new(HandlerRegistry::with_filesystem(fs.clone()))
        });

        Self {
            fs,
            runtime,
            path_cache: Arc::new(Mutex::new(HashMap::new())),
            open_files: Arc::new(Mutex::new(HashMap::new())),
            next_fh: AtomicU64::new(1),
            uid: options.uid.unwrap_or_else(|| unsafe { libc::getuid() }),
            gid: options.gid.unwrap_or_else(|| unsafe { libc::getgid() }),
            mountpoint_path: options.mountpoint.to_string_lossy().into_owned(),
            handler_registry,
        }
    }
}
```

### Modified getattr Implementation

```rust
fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
    tracing::debug!("FUSE::getattr: ino={}", ino);

    let path = if ino == 1 {
        "/".to_string()
    } else {
        match self.path_cache.lock().get(&ino) {
            Some(p) => p.clone(),
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        }
    };

    // Use handler registry instead of direct fs call
    let registry = self.handler_registry.clone();
    let result = self.runtime.block_on(async move {
        registry.handle_getattr(&path).await
    });

    match result {
        Ok(Some(stats)) => {
            let attr = fillattr(&stats, self.uid, self.gid);
            reply.attr(&TTL, &attr);
        }
        Ok(None) => reply.error(libc::ENOENT),
        Err(e) => reply.error(error_to_errno(&e)),
    }
}
```

### Modified read Implementation

```rust
fn read(
    &mut self,
    _req: &Request,
    ino: u64,
    fh: u64,
    offset: i64,
    size: u32,
    _flags: i32,
    _lock_owner: Option<u64>,
    reply: ReplyData,
) {
    tracing::debug!("FUSE::read: ino={}, fh={}, offset={}, size={}", ino, fh, offset, size);

    // Get path from cache
    let path = match self.path_cache.lock().get(&ino) {
        Some(p) => p.clone(),
        None => {
            // Fallback to file handle if path not cached
            let open_files = self.open_files.lock();
            if let Some(open_file) = open_files.get(&fh) {
                let file = open_file.file.clone();
                drop(open_files);

                let result = self.runtime.block_on(async move {
                    file.pread(offset as u64, size as u64).await
                });

                match result {
                    Ok(data) => reply.data(&data),
                    Err(e) => reply.error(error_to_errno(&e)),
                }
                return;
            }
            reply.error(libc::ENOENT);
            return;
        }
    };

    // Use handler registry
    let registry = self.handler_registry.clone();
    let result = self.runtime.block_on(async move {
        registry.handle_read(&path, offset as u64, size as u64).await
    });

    match result {
        Ok(data) => reply.data(&data),
        Err(e) => reply.error(error_to_errno(&e)),
    }
}
```

### Modified readdir Implementation

```rust
fn readdir(
    &mut self,
    _req: &Request,
    ino: u64,
    _fh: u64,
    offset: i64,
    mut reply: ReplyDirectory,
) {
    let path = if ino == 1 {
        "/".to_string()
    } else {
        match self.path_cache.lock().get(&ino) {
            Some(p) => p.clone(),
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        }
    };

    let registry = self.handler_registry.clone();
    let result = self.runtime.block_on(async move {
        registry.handle_readdir(&path).await
    });

    match result {
        Ok(Some(entries)) => {
            for (i, name) in entries.into_iter().enumerate().skip(offset as usize) {
                // Determine file type (would need stats for accurate type)
                let file_type = if name == "." || name == ".." {
                    FileType::Directory
                } else {
                    FileType::RegularFile
                };

                if reply.add(ino, (i + 1) as i64, file_type, &name) {
                    break;
                }
            }
            reply.ok();
        }
        Ok(None) => reply.error(libc::ENOENT),
        Err(e) => reply.error(error_to_errno(&e)),
    }
}
```

### Mount Function Update

```rust
// cli/src/cmd/mount.rs

pub fn mount(args: MountArgs) -> Result<()> {
    let fs = open_filesystem(&args.id_or_path)?;

    // Create handler registry with optional custom handlers
    let mut registry = HandlerRegistry::with_filesystem(fs.clone());

    // Register GraphDocs handler if .gd.md files exist
    if args.enable_graphdocs {
        registry.register(Arc::new(GraphDocsHandler::new()));
    }

    let options = FuseMountOptions {
        mountpoint: args.mountpoint,
        auto_unmount: args.auto_unmount,
        allow_root: args.allow_root,
        fsname: "duckagentfs".to_string(),
        uid: args.uid,
        gid: args.gid,
    };

    let fuse = AgentFSFuse::new(
        fs,
        &options,
        Some(Arc::new(registry)),
    );

    // Mount...
    Ok(())
}
```

## Tests

### Test 1: Handler Intercepts Read
```rust
#[test]
fn test_handler_intercepts_read() {
    // Create mock fs
    let fs = MockFileSystem::new();
    fs.add_file("/real.txt", b"real content");

    // Create registry with intercepting handler
    let mut registry = HandlerRegistry::with_filesystem(Arc::new(fs));
    registry.register(Arc::new(InterceptHandler::new("*.virtual", "virtual content")));

    // Create FUSE with registry
    let fuse = AgentFSFuse::new(
        Arc::new(MockFileSystem::new()),
        &options,
        Some(Arc::new(registry)),
    );

    // Test read of virtual file
    // ... (would need FUSE test harness)
}
```

### Test 2: Fallthrough to Default
```rust
#[test]
fn test_fallthrough() {
    let fs = MockFileSystem::new();
    fs.add_file("/test.txt", b"content");

    let registry = HandlerRegistry::with_filesystem(Arc::new(fs));

    // No custom handlers registered
    // Read should fall through to DefaultHandler -> FileSystem

    let fuse = AgentFSFuse::new(
        Arc::new(MockFileSystem::new()),
        &options,
        Some(Arc::new(registry)),
    );

    // Verify file is readable
}
```

## CLI Changes

```bash
# Mount with GraphDocs support
agentfs mount my-agent /mnt/agent --graphdocs

# Mount with custom handlers (future)
agentfs mount my-agent /mnt/agent --handler graphdocs --handler readme-gen
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/fuse.rs` | Modified FUSE implementation |
| `cli/src/handler.rs` | HandlerRegistry |
| `cli/src/cmd/mount.rs` | Mount command |
| `docs/fuse-handler-integration.md` | Detailed proposal |

## Implementation Notes

1. **Thread Safety**: HandlerRegistry is behind Arc, handlers are Send + Sync

2. **Async in Sync**: FUSE callbacks are sync, use `runtime.block_on()` carefully

3. **Path Caching**: Handlers may need paths not in cache; consider lazy resolution

4. **Error Handling**: Map handler errors to appropriate errno codes
