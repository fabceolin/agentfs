# Story BUG-002: FUSE Write Handler Not Wired to Handler Registry

| Field | Value |
|-------|-------|
| **Story ID** | BUG-002 |
| **Epic** | EPIC-CONFORMANCE-001 |
| **Status** | Draft |
| **Priority** | High |
| **Type** | Bug |
| **Discovered** | 2026-01-18 |
| **Reporter** | QA Manual Testing |

---

## Bug Summary

The FUSE write operation bypasses the handler registry, causing `ConformanceWriteHandler` to never be invoked. Writes go directly to `file.pwrite()` instead of through `handler_registry.handle_write()`.

## Story

**As a** user mounting an AgentFS filesystem with TEA conformance enabled,
**I want** writes to markdown files in template directories to trigger conformance processing,
**so that** documents are automatically transformed to match their templates.

## Current Behavior

1. User mounts with `--tea-conformance` flag
2. `ConformanceWriteHandler` is registered in the handler registry
3. User writes a `.md` file to a directory containing a template
4. Write goes directly to `file.pwrite()` in `fuse.rs:1439-1441`
5. `ConformanceWriteHandler.write()` is **never called**
6. No `.source` file created, no background conformance triggered

## Expected Behavior

1. User mounts with `--tea-conformance` flag
2. `ConformanceWriteHandler` is registered in the handler registry
3. User writes a `.md` file to a directory containing a template
4. FUSE layer calls `handler_registry.handle_write()`
5. `ConformanceWriteHandler.write()` intercepts the write
6. Content saved to `.source` file, background conformance spawned
7. Eventually `.conformant` file created with transformed content

## Root Cause Analysis

In `cli/src/fuse.rs`, the `write()` method (lines 1395-1447) directly calls:

```rust
let result = self
    .runtime
    .block_on(async move { file.pwrite(offset as u64, &data_vec).await });
```

The handler registry is used for:
- `handle_lookup` (line 307)
- `handle_getattr` (line 359)
- `handle_readdir_plus` (lines 525, 631)
- `handle_read` (lines 1300, 1321)

But **NOT** for:
- `handle_write` - missing
- `handle_create` - missing (may also need handler support)

## Acceptance Criteria

1. FUSE `write()` calls `handler_registry.handle_write()` before/instead of `file.pwrite()`
2. If handler returns `Ok(Some(bytes_written))`, use that result
3. If handler returns `Ok(None)`, fall back to `file.pwrite()`
4. `ConformanceWriteHandler.write()` is invoked for `.md` files in template directories
5. Background conformance is triggered after write completes
6. Existing tests continue to pass
7. New integration test verifies conformance triggered on FUSE write

## Technical Notes

### Files to Modify

1. **`cli/src/fuse.rs`** - Wire `write()` to handler registry
   - Add call to `handler_registry.handle_write(&path, offset, &data)`
   - Handle the `HandlerResult<usize>` return type
   - Fall back to direct `file.pwrite()` if handler returns `None`

2. **`cli/src/handler.rs`** - Verify `handle_write` method exists on `HandlerRegistry`
   - Method should iterate handlers by priority
   - Call `can_handle()` then `write()` on matching handler

### Handler Registry Write Flow

```
FUSE::write(ino, fh, offset, data)
    |
    v
path = get_path(ino)
    |
    v
handler_registry.handle_write(&path, offset, &data)
    |
    +-- ConformanceWriteHandler.can_handle(path)?
    |       |
    |       +-- Yes: ConformanceWriteHandler.write() -> Ok(Some(len))
    |       |
    |       +-- No: continue to next handler
    |
    +-- DefaultHandler.can_handle(path)? -> Yes: file.pwrite() -> Ok(Some(len))
    |
    v
reply.written(len)
```

### FUSE Create Flow (May Also Need Fix)

The `create()` method (lines 894-944) also bypasses handlers:

```rust
let result = self
    .runtime
    .block_on(async move { fs.create_file(&path_for_create, mode).await });
```

Consider whether `handle_create` should also be wired.

## Testing

### Manual Test

1. Mount: `agentfs mount test-agent /tmp/mnt --tea-conformance --tea-agents-dir agents -f`
2. Create template directory: `mkdir /tmp/mnt/stories && cp story-tmpl.yaml /tmp/mnt/stories/`
3. Write test file: `echo "# Test" > /tmp/mnt/stories/test.md`
4. Verify `.source` file created: `ls /tmp/mnt/stories/test.md.source`
5. Wait for conformance: `ls /tmp/mnt/stories/test.md.conformant`

### Integration Test

Add test in `cli/tests/` that:
1. Creates DuckDB with GraphDocs tables
2. Mounts with TEA conformance enabled
3. Writes a non-conformant markdown file
4. Verifies `.source` and `.conformant` files created
5. Verifies database sync (gd_documents, gd_sections)

## Change Log

| Date | Description | Author |
|------|-------------|--------|
| 2026-01-18 | Bug discovered during manual testing | QA |
| 2026-01-18 | Story created | Quinn (Test Architect) |
