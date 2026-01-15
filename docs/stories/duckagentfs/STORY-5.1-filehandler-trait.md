# STORY-5.1: FileHandler Trait

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-5.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 5 - FUSE + Handler Registry |
| **Status** | Ready for Development |
| **Priority** | High |
| **File** | `cli/src/handler.rs` |

## User Story

**As a** developer
**I want** an extensible handler system
**So that** I can intercept FUSE operations

## Technical Description

The FileHandler trait allows intercepting FUSE operations for specific file patterns. This enables features like:

- GraphDocs: Render markdown from property graphs
- Dynamic content generation
- Virtual files with computed content
- Custom access control

## Acceptance Criteria

- [x] Trait `FileHandler` with async methods
- [x] Support for priorities (lower = higher priority)
- [x] Methods: can_handle, read, getattr, write, readdir
- [x] Handler result type with Ok(Some), Ok(None), Err semantics

## Technical Specification

### Handler Result Type

```rust
/// Result type for handler operations
/// - Ok(Some(data)): Handler handled the operation
/// - Ok(None): Handler declined, try next handler
/// - Err(e): Handler failed with error
pub type HandlerResult<T> = Result<Option<T>>;
```

### FileHandler Trait

```rust
#[async_trait]
pub trait FileHandler: Send + Sync {
    /// Handler name for debugging/logging
    fn name(&self) -> &str;

    /// Priority (lower = higher priority, tried first)
    /// Default: 100
    fn priority(&self) -> u32 {
        100
    }

    /// Check if handler should handle this path
    fn can_handle(&self, path: &str, stats: Option<&Stats>) -> bool;

    /// Handle read operation
    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        let _ = (path, offset, size);
        Ok(None)
    }

    /// Handle getattr operation
    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        let _ = path;
        Ok(None)
    }

    /// Handle readdir operation
    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        let _ = path;
        Ok(None)
    }

    /// Handle readdir_plus operation
    async fn readdir_plus(&self, path: &str) -> HandlerResult<Vec<DirEntry>> {
        let _ = path;
        Ok(None)
    }

    /// Handle write operation
    async fn write(&self, path: &str, offset: u64, data: &[u8]) -> HandlerResult<usize> {
        let _ = (path, offset, data);
        Ok(None)
    }

    /// Handle truncate operation
    async fn truncate(&self, path: &str, size: u64) -> HandlerResult<()> {
        let _ = (path, size);
        Ok(None)
    }

    /// Handle readlink operation
    async fn readlink(&self, path: &str) -> HandlerResult<String> {
        let _ = path;
        Ok(None)
    }
}
```

### Priority Levels

| Range | Level | Use Case |
|-------|-------|----------|
| 0-49 | High | Intercept before others (security, virtual files) |
| 50-99 | Normal | Standard handlers (GraphDocs, computed content) |
| 100+ | Low | Fallback handlers |
| u32::MAX | Default | DefaultHandler (always last) |

### Implementation Example

```rust
struct ExampleHandler {
    pattern: String,
}

impl ExampleHandler {
    fn new(pattern: &str) -> Self {
        Self { pattern: pattern.to_string() }
    }
}

#[async_trait]
impl FileHandler for ExampleHandler {
    fn name(&self) -> &str {
        "example"
    }

    fn priority(&self) -> u32 {
        50
    }

    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        path.ends_with(&self.pattern)
    }

    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        // Generate content dynamically
        let content = format!("Generated content for: {}", path);
        let bytes = content.as_bytes();

        let start = offset as usize;
        let end = (offset + size) as usize;

        if start >= bytes.len() {
            return Ok(Some(vec![]));
        }

        Ok(Some(bytes[start..end.min(bytes.len())].to_vec()))
    }

    async fn getattr(&self, _path: &str) -> HandlerResult<Stats> {
        // Return virtual file stats
        Ok(Some(Stats {
            ino: 0,
            mode: 0o100444, // Read-only file
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 100,
            atime: 0,
            mtime: 0,
            ctime: 0,
        }))
    }
}
```

## Tests

### Test 1: Handler Priority
```rust
#[test]
fn test_handler_priority() {
    struct HighPriority;
    struct LowPriority;

    impl FileHandler for HighPriority {
        fn name(&self) -> &str { "high" }
        fn priority(&self) -> u32 { 10 }
        fn can_handle(&self, _: &str, _: Option<&Stats>) -> bool { true }
    }

    impl FileHandler for LowPriority {
        fn name(&self) -> &str { "low" }
        fn priority(&self) -> u32 { 100 }
        fn can_handle(&self, _: &str, _: Option<&Stats>) -> bool { true }
    }

    let high = HighPriority;
    let low = LowPriority;

    assert!(high.priority() < low.priority());
}
```

### Test 2: Handler Chaining
```rust
#[tokio::test]
async fn test_handler_chaining() {
    struct DeclineHandler;

    #[async_trait]
    impl FileHandler for DeclineHandler {
        fn name(&self) -> &str { "decline" }
        fn can_handle(&self, _: &str, _: Option<&Stats>) -> bool { true }

        async fn read(&self, _: &str, _: u64, _: u64) -> HandlerResult<Vec<u8>> {
            Ok(None) // Decline - try next handler
        }
    }

    let handler = DeclineHandler;
    let result = handler.read("/test", 0, 100).await.unwrap();
    assert!(result.is_none());
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/handler.rs` | FileHandler trait |
| `cli/src/fuse.rs` | FUSE integration point |
| `sdk/rust/src/filesystem/mod.rs` | FileSystem trait (similar interface) |

## QA Notes

**Review Date:** 2026-01-14
**Reviewer:** Quinn (Test Architect)
**Story Status:** Done

### Test Coverage Summary

| Area | Coverage | Notes |
|------|----------|-------|
| Priority ordering | ✅ Covered | Test 1 validates priority comparison |
| Handler chaining (decline) | ✅ Covered | Test 2 validates Ok(None) semantics |
| Read operation | ⚠️ Partial | Example impl shown, no error path test |
| Write operation | ❌ Missing | No test for write semantics |
| Truncate operation | ❌ Missing | No test coverage |
| Readdir/readdir_plus | ❌ Missing | No test coverage |
| Readlink operation | ❌ Missing | No test coverage |
| Getattr operation | ⚠️ Partial | Example only, no dedicated test |
| Error handling | ❌ Missing | No Err(e) path testing |

**Overall Coverage Assessment:** ~30% - Core trait defined, minimal test scenarios

### Risk Areas Identified

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Priority collision | Medium | Medium | Add tests for handlers with same priority |
| Async race conditions | Medium | High | Add concurrent handler access tests |
| Handler error propagation | Low | High | Test Err variant handling in registry chain |
| Offset/size boundary conditions | Medium | Medium | Add edge case tests (offset > size, zero reads) |
| Virtual file stat consistency | Low | Medium | Verify getattr values match read behavior |

### Recommended Test Scenarios

#### High Priority (Must Have)
1. **Error propagation test** - Verify `Err(e)` stops handler chain and propagates error
2. **Write operation test** - Test basic write, partial write, and write at offset
3. **Concurrent handler access** - Multiple async reads to same handler simultaneously

#### Medium Priority (Should Have)
4. **Boundary conditions** - Read with offset >= content length, zero-size reads
5. **Priority collision** - Two handlers with same priority, verify deterministic order
6. **Readdir enumeration** - Directory listing with multiple virtual entries
7. **Truncate semantics** - Verify size changes reflected in subsequent reads

#### Lower Priority (Nice to Have)
8. **Readlink for virtual symlinks** - Test symlink resolution path
9. **Handler hot-registration** - Adding handlers at runtime (if supported)
10. **Stats consistency** - Verify inode, mode, timestamps across operations

### Concerns

1. **Test depth vs. breadth**: Only 2 tests cover a trait with 8 async methods - significant gaps exist
2. **No integration test**: Missing test showing full handler→registry→FUSE flow
3. **Error semantics undocumented**: The `Err(e)` case behavior in handler chains is not specified or tested

### Blockers

None - story is complete and functional. Test gaps are technical debt to track.

### Recommendations

1. Add error path test before next release
2. Document handler chain behavior when `Err` is returned (stop chain? continue?)
3. Consider property-based testing for offset/size boundary conditions

**QA Decision:** PASS with CONCERNS - Core functionality implemented and tested. Test coverage gaps should be addressed in subsequent iteration.
