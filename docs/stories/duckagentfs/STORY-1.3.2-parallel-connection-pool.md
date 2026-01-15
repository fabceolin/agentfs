# STORY-1.3.2: Multi-Connection Pool for Parallel Read Access

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.3.2 |
| **Parent** | STORY-1.3 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Backlog (Deferred) |
| **Priority** | Low |
| **Estimated Effort** | Medium (3-5 days) |
| **File** | `sdk/rust/src/filesystem/duckagentfs.rs` |
| **Dependencies** | STORY-1.2.1 |

## Problem Statement

The current implementation (from STORY-1.2.1) uses a single `Arc<Mutex<Connection>>` with `spawn_blocking` for all database operations. This creates a **serialization bottleneck**:

```
Request 1 ─┐
Request 2 ─┼─► Mutex ─► Single Connection ─► DuckDB
Request 3 ─┘
           ↑
     All operations wait here
```

Even read operations that could run in parallel are serialized because:
1. DuckDB's `Connection` type is not `Send + Sync` (uses `RefCell` internally)
2. Our workaround wraps it in `std::sync::Mutex`
3. All operations acquire the same lock

## User Story

**As a** developer using DuckAgentFS
**I want** concurrent read operations to execute in parallel
**So that** multi-threaded applications have better throughput

## Technical Description

### DuckDB Concurrency Model

DuckDB supports **multiple concurrent readers with a single writer**:
- Multiple connections CAN read simultaneously
- Only ONE connection can write at a time (single-writer)
- Each connection is NOT thread-safe individually (RefCell)
- But MULTIPLE connections can operate from different threads

### Proposed Solution

Instead of one shared connection, create **multiple independent connections**:

```
Request 1 ─► spawn_blocking ─► Connection 1 ─┐
Request 2 ─► spawn_blocking ─► Connection 2 ─┼─► DuckDB file
Request 3 ─► spawn_blocking ─► Connection 3 ─┘
                                              ↑
                               Parallel reads at file level
```

**Key insight**: DuckDB handles locking at the file level, not the connection level. Multiple connections can read in parallel - DuckDB manages the coordination.

## Acceptance Criteria

- [ ] Implement `DuckConnectionPool` with configurable max connections
- [ ] Separate read pool (N connections) from write pool (1 connection)
- [ ] Semaphore-based acquisition with timeout
- [ ] Connections created on-demand within `spawn_blocking` context
- [ ] Pool metrics (acquired, active, timeouts, wait time)
- [ ] Benchmark proving parallel reads are faster than serial
- [ ] Graceful shutdown with `close()` method
- [ ] No regression in existing CRUD tests

## Technical Specification

### Pool Architecture

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{Semaphore, OwnedSemaphorePermit};

/// Connection pool supporting parallel reads, serialized writes.
#[derive(Clone)]
pub struct DuckConnectionPool {
    /// Path to DuckDB database
    path: String,

    /// Semaphore limiting concurrent readers
    read_semaphore: Arc<Semaphore>,

    /// Semaphore ensuring single writer (1 permit)
    write_semaphore: Arc<Semaphore>,

    /// Pool configuration
    config: PoolConfig,

    /// Runtime metrics
    metrics: Arc<PoolMetrics>,
}

#[derive(Clone)]
pub struct PoolConfig {
    /// Maximum concurrent read connections
    pub max_readers: usize,

    /// Timeout for acquiring a permit (milliseconds)
    pub acquire_timeout_ms: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_readers: 8,          // Tune based on core count
            acquire_timeout_ms: 5000,
        }
    }
}
```

### Connection Acquisition Pattern

The key innovation is creating connections **inside** `spawn_blocking`:

```rust
impl DuckConnectionPool {
    /// Get a read connection with parallel access.
    pub async fn read<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let start = std::time::Instant::now();

        // Acquire semaphore permit (async, doesn't block thread)
        let _permit = tokio::time::timeout(
            Duration::from_millis(self.config.acquire_timeout_ms),
            self.read_semaphore.clone().acquire_owned()
        ).await
        .map_err(|_| {
            self.metrics.timeouts.fetch_add(1, Ordering::Relaxed);
            Error::ConnectionPoolTimeout
        })?
        .map_err(|_| Error::ConnectionPoolClosed)?;

        self.metrics.reads_acquired.fetch_add(1, Ordering::Relaxed);
        self.metrics.active_readers.fetch_add(1, Ordering::Relaxed);
        self.metrics.total_wait_time_ms.fetch_add(
            start.elapsed().as_millis() as u64,
            Ordering::Relaxed
        );

        let path = self.path.clone();
        let metrics = self.metrics.clone();

        // Connection created INSIDE spawn_blocking (sync context)
        tokio::task::spawn_blocking(move || {
            let conn = Connection::open(&path)
                .map_err(|e| Error::Custom(format!("Failed to open connection: {}", e)))?;

            let result = f(&conn);

            // Permit dropped here, releasing semaphore slot
            metrics.active_readers.fetch_sub(1, Ordering::Relaxed);

            result
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    /// Get exclusive write connection.
    pub async fn write<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let start = std::time::Instant::now();

        // Single-writer semaphore
        let _permit = tokio::time::timeout(
            Duration::from_millis(self.config.acquire_timeout_ms),
            self.write_semaphore.clone().acquire_owned()
        ).await
        .map_err(|_| {
            self.metrics.timeouts.fetch_add(1, Ordering::Relaxed);
            Error::ConnectionPoolTimeout
        })?
        .map_err(|_| Error::ConnectionPoolClosed)?;

        self.metrics.writes_acquired.fetch_add(1, Ordering::Relaxed);
        self.metrics.active_writers.fetch_add(1, Ordering::Relaxed);
        self.metrics.total_wait_time_ms.fetch_add(
            start.elapsed().as_millis() as u64,
            Ordering::Relaxed
        );

        let path = self.path.clone();
        let metrics = self.metrics.clone();

        tokio::task::spawn_blocking(move || {
            let conn = Connection::open(&path)
                .map_err(|e| Error::Custom(format!("Failed to open connection: {}", e)))?;

            let result = f(&conn);

            metrics.active_writers.fetch_sub(1, Ordering::Relaxed);

            result
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }
}
```

### Updated FileSystem Usage

```rust
// BEFORE (serial):
async fn stat(&self, path: &str) -> Result<Option<Stats>> {
    let pool = self.pool.clone();
    let path = path.to_string();

    tokio::task::spawn_blocking(move || {
        let conn = pool.get_connection()?;  // Mutex lock!
        // ... operation
    }).await
}

// AFTER (parallel reads):
async fn stat(&self, path: &str) -> Result<Option<Stats>> {
    let dentry_cache = self.dentry_cache.clone();
    let path = path.to_string();

    self.pool.read(move |conn| {
        match Self::resolve_path_sync(conn, &dentry_cache, &path, true)? {
            Some(ino) => Self::stat_inode_sync(conn, ino),
            None => Ok(None),
        }
    }).await
}
```

### Metrics

```rust
pub struct PoolMetrics {
    pub reads_acquired: AtomicU64,
    pub writes_acquired: AtomicU64,
    pub timeouts: AtomicU64,
    pub active_readers: AtomicU64,
    pub active_writers: AtomicU64,
    pub total_wait_time_ms: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub reads_acquired: u64,
    pub writes_acquired: u64,
    pub timeouts: u64,
    pub active_readers: u64,
    pub active_writers: u64,
    pub total_wait_time_ms: u64,
    pub avg_wait_time_ms: f64,
}
```

## Tests

### Test 1: Parallel Reads are Faster

```rust
#[tokio::test]
async fn test_parallel_reads_faster_than_serial() {
    let fs = create_test_fs_with_data().await;  // Pre-populate with files

    // Measure serial reads (one at a time)
    let serial_start = Instant::now();
    for i in 0..100 {
        let _ = fs.stat(&format!("/file{}.txt", i)).await;
    }
    let serial_duration = serial_start.elapsed();

    // Measure parallel reads (concurrent)
    let parallel_start = Instant::now();
    let futures: Vec<_> = (0..100)
        .map(|i| fs.stat(format!("/file{}.txt", i)))
        .collect();
    let _ = futures::future::join_all(futures).await;
    let parallel_duration = parallel_start.elapsed();

    // Parallel should be significantly faster
    assert!(
        parallel_duration < serial_duration / 2,
        "Parallel ({:?}) should be at least 2x faster than serial ({:?})",
        parallel_duration, serial_duration
    );
}
```

### Test 2: Single Writer Enforced

```rust
#[tokio::test]
async fn test_single_writer_blocking() {
    let pool = DuckConnectionPool::new(":memory:", PoolConfig::default())?;

    // Start a long write
    let write1 = tokio::spawn({
        let pool = pool.clone();
        async move {
            pool.write(|conn| {
                std::thread::sleep(Duration::from_millis(200));
                Ok(())
            }).await
        }
    });

    // Give write1 time to acquire permit
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Second write should block
    let write2_start = Instant::now();
    pool.write(|_conn| Ok(())).await.unwrap();
    let write2_wait = write2_start.elapsed();

    // Write2 should have waited for write1
    assert!(write2_wait >= Duration::from_millis(100));

    write1.await.unwrap().unwrap();
}
```

### Test 3: Readers Don't Block Readers

```rust
#[tokio::test]
async fn test_readers_dont_block_readers() {
    let pool = DuckConnectionPool::new(":memory:", PoolConfig {
        max_readers: 8,
        ..Default::default()
    })?;

    // Start 8 concurrent reads with sleep
    let start = Instant::now();
    let futures: Vec<_> = (0..8).map(|_| {
        let pool = pool.clone();
        async move {
            pool.read(|_conn| {
                std::thread::sleep(Duration::from_millis(100));
                Ok(())
            }).await
        }
    }).collect();

    let _ = futures::future::join_all(futures).await;
    let duration = start.elapsed();

    // If parallel: ~100ms. If serial: ~800ms.
    assert!(
        duration < Duration::from_millis(300),
        "8 parallel reads should complete in ~100ms, not {:?}",
        duration
    );
}
```

### Test 4: Pool Metrics Accuracy

```rust
#[tokio::test]
async fn test_pool_metrics() {
    let pool = DuckConnectionPool::new(":memory:", PoolConfig::default())?;

    // Do some operations
    pool.read(|_| Ok(())).await?;
    pool.read(|_| Ok(())).await?;
    pool.write(|_| Ok(())).await?;

    let metrics = pool.metrics();
    assert_eq!(metrics.reads_acquired, 2);
    assert_eq!(metrics.writes_acquired, 1);
    assert_eq!(metrics.active_readers, 0);
    assert_eq!(metrics.active_writers, 0);
}
```

### Test 5: Timeout Behavior

```rust
#[tokio::test]
async fn test_read_timeout() {
    let pool = DuckConnectionPool::new(":memory:", PoolConfig {
        max_readers: 1,
        acquire_timeout_ms: 50,
    })?;

    // Hold the only read slot
    let _hold = tokio::spawn({
        let pool = pool.clone();
        async move {
            pool.read(|_conn| {
                std::thread::sleep(Duration::from_millis(200));
                Ok(())
            }).await
        }
    });

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Should timeout
    let result = pool.read(|_| Ok(())).await;
    assert!(matches!(result, Err(Error::ConnectionPoolTimeout)));

    assert_eq!(pool.metrics().timeouts, 1);
}
```

## Migration Path

The new pool API changes the interface. Migration steps:

1. Update `DuckConnectionPool` implementation
2. Change all `spawn_blocking` + `pool.get_connection()` to `pool.read(|conn| ...)`
3. Change all write operations to `pool.write(|conn| ...)`
4. Remove explicit `spawn_blocking` wrappers in FileSystem methods
5. Run existing tests to verify no regression

## Performance Expectations

| Scenario | Current (Serial) | New (Parallel) | Expected Speedup |
|----------|------------------|----------------|------------------|
| 100 stat() calls | ~500ms | ~100ms | 5x |
| 10 read_file() concurrent | ~200ms | ~50ms | 4x |
| Mixed read/write workload | ~300ms | ~150ms | 2x |

## Relationship to STORY-1.3

STORY-1.3 defined the pool interface with semaphores but assumed `PooledConnection` could hold a `Connection` directly. This story adapts that design to work with DuckDB's non-Send Connection by:

1. Creating connections inside `spawn_blocking` (not holding them)
2. Using closures instead of returning connection handles
3. Maintaining the semaphore-based concurrency control

## Error Types

Add to `error.rs`:

```rust
pub enum Error {
    // ... existing variants

    /// Connection pool timeout waiting for available connection
    ConnectionPoolTimeout,

    /// Connection pool has been closed
    ConnectionPoolClosed,
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/filesystem/duckagentfs.rs` | Pool implementation + usage |
| `sdk/rust/src/error.rs` | New error variants |
| `docs/stories/duckagentfs/STORY-1.3-connection-pool.md` | Original pool spec |

## Implementation Notes

1. **Connection Overhead**: Creating connections on every call has overhead. Monitor if this becomes a bottleneck. Future optimization: connection caching with thread-local storage.

2. **In-Memory Database**: For `:memory:` databases, each connection gets a separate database! Use file-based paths for shared access, or use `:memory:?cache=shared` mode.

3. **WAL Mode**: DuckDB uses its own WAL-like mechanism. Multiple readers don't block writers in most cases.

4. **Blocking Pool Size**: Tokio's blocking pool has 512 threads by default. Our `max_readers` should be much lower to avoid overwhelming the system.

## Created By

Architect Review Session 2026-01-15: Response to performance concern about serial mutex access in STORY-1.2.1 implementation.

---

## QA Results

**Review Date:** 2026-01-15
**Reviewer:** Quinn (Test Architect)

### Test Design Completed

A comprehensive test design has been created with **23 test scenarios**:

| Level | Count | Percentage |
|-------|-------|------------|
| Unit | 5 | 22% |
| Integration | 14 | 61% |
| E2E | 4 | 17% |

**Priority Distribution:** P0: 10, P1: 9, P2: 4

**Test Design Document:** `docs/qa/assessments/1.3.2-test-design-20260115.md`

### Deferral Decision

**Date:** 2026-01-15
**Decision By:** Product Owner (Sarah)

**Rationale:**
This story is a **performance optimization**, not a functional requirement. The current serial connection pool implementation:

- Provides **100% functional correctness**
- Only impacts **throughput under concurrent load**
- Has **lower complexity and risk** than parallel implementation

**Impact of Deferral:**
- Concurrent read operations will be serialized (slower)
- No loss of functionality
- No blocking of other stories

**Recommendation:**
Implement after core functionality is stable. Revisit when:
1. Performance profiling shows this is an actual bottleneck
2. Core DuckAgentFS features are complete and tested
3. Team has bandwidth for concurrency complexity

### Dependency Check

**Stories depending on this:** None identified
**Blocking status:** Not blocking any other work
