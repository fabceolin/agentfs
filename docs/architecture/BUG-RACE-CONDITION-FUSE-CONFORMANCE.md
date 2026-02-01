# Race Condition: FUSE Conformance Handler

**Date:** 2026-01-30
**Severity:** High
**Status:** Open
**Related Story:** STORY-8.1 Mass Conformance Validation

## Executive Summary

A race condition exists in the FUSE conformance handler that causes multiple parallel conformance processes to be spawned for a single file write. This results in:
1. **Timeouts** - Conformance never completes
2. **Resource waste** - Multiple Claude API calls for the same file
3. **Potential data corruption** - Multiple processes writing to same output file

## Symptoms

When writing a file to the FUSE mount with TEA conformance enabled:

```
[INFO] Starting background conformance for /stories/BUG.001.md
[INFO] Starting background conformance for /stories/BUG.001.md
[INFO] Starting background conformance for /stories/BUG.001.md
[INFO] Starting background conformance for /stories/BUG.001.md
[INFO] Starting background conformance for /stories/BUG.001.md
```

5 conformance processes spawned within 100ms for a single `cp` or `cat >` command.

## Root Cause Analysis

### 1. FUSE Write Behavior

FUSE `write()` is called **multiple times** for a single file copy:

```
File: 18KB
Write chunks: ~5-8 calls (4KB default buffer)

Each write() triggers:
  ConformanceWriteHandler::handle_write()
    → spawns background conformance task
```

### 2. No Deduplication

The current handler spawns a new task on **every** `write()` call:

```rust
// cli/src/handler.rs - ConformanceWriteHandler
async fn handle_write(&self, ...) -> Result<WriteResult> {
    // ... write data ...

    // Spawns background task on EVERY write
    self.run_background_conformance(fs, logical_path, config).await;

    Ok(WriteResult::Written(size))
}
```

### 3. Abort Check Insufficient

The "Source modified during conformance" check only aborts if mtime changed:

```rust
// Line 1679-1687
let current_mtime = fs.stat(&source_path).await?.map(|s| s.mtime);
if current_mtime != source_mtime {
    tracing::info!("Source modified during conformance for {}, aborting", logical_path);
    return Ok(());
}
```

But this doesn't prevent **parallel** processes from running - it only aborts stale results.

## Architecture Context

### FUSE Write Flow

```
┌──────────┐    ┌──────────────┐    ┌─────────────────────┐
│ cp file  │───▶│ FUSE write() │───▶│ handle_write()      │
│          │    │ (chunk 1)    │    │ → spawn conformance │
└──────────┘    └──────────────┘    └─────────────────────┘
                      │
                      │ (chunk 2)
                      ▼
                ┌──────────────┐    ┌─────────────────────┐
                │ FUSE write() │───▶│ handle_write()      │
                │ (chunk 2)    │    │ → spawn conformance │
                └──────────────┘    └─────────────────────┘
                      │
                      ▼
                   ... (N chunks)
```

### Handler Registry

```rust
pub struct HandlerRegistry {
    handlers: Vec<Box<dyn FileHandler>>,
    // No per-file state tracking
}
```

The registry has no mechanism to track in-flight operations per file.

## Proposed Solutions

### Option 1: Debounce on `flush()`/`release()` (Recommended)

Only trigger conformance on file close, not on every write:

```rust
// Move conformance trigger to flush() or release()
async fn handle_flush(&self, ...) -> Result<()> {
    // File is complete, now run conformance
    self.run_background_conformance(...).await;
    Ok(())
}
```

**Pros:**
- Clean, simple fix
- Matches user expectation (file fully written before processing)
- Single conformance run per file

**Cons:**
- Requires FUSE flush/release handling (may not be implemented)

### Option 2: Debounce with Timeout

Track pending conformance runs with debounce:

```rust
struct ConformanceState {
    pending: HashMap<String, Instant>,
    debounce_ms: u64,  // e.g., 500ms
}

async fn handle_write(&self, ...) {
    // Update pending timestamp
    self.state.pending.insert(path.clone(), Instant::now());

    // Spawn debounced task
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(debounce_ms)).await;

        // Check if still pending (no newer write)
        if self.state.pending.get(&path) == Some(original_instant) {
            self.run_conformance(path).await;
        }
    });
}
```

**Pros:**
- Works with current architecture
- Handles rapid sequential writes

**Cons:**
- Adds complexity
- Requires shared state management

### Option 3: Lock-based Exclusion

Use a mutex per file to ensure single conformance:

```rust
struct ConformanceState {
    in_progress: DashMap<String, Arc<Mutex<()>>>,
}

async fn run_background_conformance(&self, path: &str, ...) {
    let lock = self.state.in_progress
        .entry(path.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())));

    // Try to acquire lock, skip if already in progress
    if let Ok(_guard) = lock.try_lock() {
        // Run conformance
        self.do_conformance(path).await;
    } else {
        tracing::debug!("Conformance already in progress for {}", path);
    }
}
```

**Pros:**
- Simple locking mechanism
- Prevents parallel runs

**Cons:**
- Might skip conformance if file changed during processing
- Need to handle lock cleanup

### Option 4: Event Coalescing with Queue

Use a queue that coalesces events:

```rust
struct ConformanceQueue {
    pending: HashSet<String>,
    worker_tx: mpsc::Sender<String>,
}

// Single worker processes queue
async fn worker(rx: mpsc::Receiver<String>) {
    let mut batch = HashSet::new();
    loop {
        // Collect events for 1 second
        while let Ok(path) = rx.try_recv() {
            batch.insert(path);
        }

        // Process unique paths
        for path in batch.drain() {
            run_conformance(&path).await;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
```

**Pros:**
- Batch processing
- Natural deduplication

**Cons:**
- Adds latency
- More complex architecture

## Recommendation

**Option 1 (flush-based)** is the cleanest solution if FUSE flush handling is available.

If not available, **Option 3 (lock-based)** provides immediate fix with minimal changes.

## Temporary Workaround

Until fixed, users can:
1. Increase `--tea-timeout` to 300+ seconds
2. Use smaller files (<10KB)
3. Wait between file copies (5+ seconds)

## Test Case

```bash
# This should trigger exactly 1 conformance run, not 5
cp large-story.md /mnt/stories/

# Expected log:
# [INFO] Starting background conformance for /stories/large-story.md
# [INFO] Conformance completed for /stories/large-story.md

# Actual log (bug):
# [INFO] Starting background conformance for /stories/large-story.md
# [INFO] Starting background conformance for /stories/large-story.md
# [INFO] Starting background conformance for /stories/large-story.md
# [INFO] Starting background conformance for /stories/large-story.md
# [INFO] Starting background conformance for /stories/large-story.md
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/handler.rs` | ConformanceWriteHandler implementation |
| `cli/src/fuse.rs` | FUSE filesystem implementation |
| `cli/src/cmd/mount.rs` | Mount command and handler registration |

## References

- [FUSE write documentation](https://libfuse.github.io/doxygen/structfuse__operations.html#a897d1ece4b8b04c92d97b97b2dbf9768)
- [Tokio debounce patterns](https://tokio.rs/tokio/tutorial/channels)
- STORY-8.1 Mass Conformance Validation

## Action Items

1. [ ] Decide on solution approach (recommend Option 1 or 3)
2. [ ] Implement fix in `cli/src/handler.rs`
3. [ ] Add test for single conformance per file
4. [ ] Update STORY-8.1 with bug reference
5. [ ] Re-run mass conformance validation after fix
