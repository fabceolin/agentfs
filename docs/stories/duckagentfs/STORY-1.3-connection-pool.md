# STORY-1.3: Connection Pool DuckDB

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.3 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Ready for Implementation |
| **Priority** | High |
| **File** | `sdk/rust/src/duckdb_pool.rs` (new) |
| **Dependencies** | STORY-1.1 |

### Status Notes (2026-01-14)

**Ready for Implementation** - Sprint Change Proposal approved:
- Retry mechanism descoped to STORY-1.3.1 (backlog)
- Added tests: timeout error (P1), RAII cleanup (P1), graceful shutdown (P1)
- Added `close()` method for graceful shutdown
- Updated `create_connection()` to use real DuckDB

## User Story

**As a** developer
**I want** an optimized connection pool for DuckDB
**So that** I can manage concurrency with single-writer semantics

## Technical Description

DuckDB uses single-writer semantics: only one connection can write at a time, but multiple connections can read simultaneously. The pool must manage this using semaphores.

## Acceptance Criteria

- [ ] Semaphore for write operations (1 permit)
- [ ] Pool of read connections (N permits)
- [ ] Pool usage metrics
- [ ] Configurable timeout
- [ ] Graceful shutdown support (`close()` method)
- [ ] Real DuckDB connection creation (not stubbed)

## Technical Specification

### Pool Structure

```rust
use tokio::sync::{Semaphore, SemaphorePermit};
use std::sync::Arc;

pub struct DuckConnectionPool {
    /// Path to the DuckDB database
    path: String,

    /// Semaphore for write operations (single writer)
    write_semaphore: Arc<Semaphore>,

    /// Semaphore for read operations
    read_semaphore: Arc<Semaphore>,

    /// Configuration
    config: PoolConfig,

    /// Metrics
    metrics: Arc<PoolMetrics>,
}

pub struct PoolConfig {
    /// Maximum concurrent readers
    pub max_readers: usize,

    /// Timeout for acquiring connection (ms)
    pub acquire_timeout_ms: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_readers: 10,
            acquire_timeout_ms: 5000,
        }
    }
}
```

### Metrics

```rust
use std::sync::atomic::{AtomicU64, Ordering};

pub struct PoolMetrics {
    /// Total read connections acquired
    pub reads_acquired: AtomicU64,

    /// Total write connections acquired
    pub writes_acquired: AtomicU64,

    /// Total timeouts
    pub timeouts: AtomicU64,

    /// Current active readers
    pub active_readers: AtomicU64,

    /// Current active writers (0 or 1)
    pub active_writers: AtomicU64,

    /// Total wait time (ms)
    pub total_wait_time_ms: AtomicU64,
}

impl PoolMetrics {
    pub fn new() -> Self {
        Self {
            reads_acquired: AtomicU64::new(0),
            writes_acquired: AtomicU64::new(0),
            timeouts: AtomicU64::new(0),
            active_readers: AtomicU64::new(0),
            active_writers: AtomicU64::new(0),
            total_wait_time_ms: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            reads_acquired: self.reads_acquired.load(Ordering::Relaxed),
            writes_acquired: self.writes_acquired.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
            active_readers: self.active_readers.load(Ordering::Relaxed),
            active_writers: self.active_writers.load(Ordering::Relaxed),
            total_wait_time_ms: self.total_wait_time_ms.load(Ordering::Relaxed),
        }
    }
}
```

### Pool Implementation

```rust
impl DuckConnectionPool {
    pub async fn new(path: &str, config: PoolConfig) -> Result<Self> {
        Ok(Self {
            path: path.to_string(),
            write_semaphore: Arc::new(Semaphore::new(1)),
            read_semaphore: Arc::new(Semaphore::new(config.max_readers)),
            metrics: Arc::new(PoolMetrics::new()),
            config,
        })
    }

    /// Get a read-only connection
    pub async fn get_connection(&self) -> Result<PooledConnection> {
        let start = std::time::Instant::now();

        let permit = tokio::time::timeout(
            std::time::Duration::from_millis(self.config.acquire_timeout_ms),
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

        let conn = self.create_connection()?;

        Ok(PooledConnection {
            conn,
            _permit: ConnectionPermit::Read(permit),
            metrics: self.metrics.clone(),
        })
    }

    /// Get a write connection (exclusive)
    pub async fn get_write_connection(&self) -> Result<PooledConnection> {
        let start = std::time::Instant::now();

        let permit = tokio::time::timeout(
            std::time::Duration::from_millis(self.config.acquire_timeout_ms),
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

        let conn = self.create_connection()?;

        Ok(PooledConnection {
            conn,
            _permit: ConnectionPermit::Write(permit),
            metrics: self.metrics.clone(),
        })
    }

    fn create_connection(&self) -> Result<duckdb::Connection> {
        duckdb::Connection::open(&self.path)
            .map_err(|e| Error::Database(e.to_string()))
    }

    /// Get current metrics
    pub fn metrics(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Gracefully close the pool, waiting for active connections
    pub async fn close(&self) -> Result<()> {
        // Close semaphores to prevent new acquisitions
        self.write_semaphore.close();
        self.read_semaphore.close();

        // Wait for all active connections to be released
        // by attempting to acquire all permits (blocks until released)
        let _ = self.write_semaphore.acquire().await;
        for _ in 0..self.config.max_readers {
            let _ = self.read_semaphore.acquire().await;
        }

        Ok(())
    }
}
```

### Pooled Connection with RAII

```rust
use tokio::sync::OwnedSemaphorePermit;

enum ConnectionPermit {
    Read(OwnedSemaphorePermit),
    Write(OwnedSemaphorePermit),
}

pub struct PooledConnection {
    conn: duckdb::Connection,
    _permit: ConnectionPermit,
    metrics: Arc<PoolMetrics>,
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        match &self._permit {
            ConnectionPermit::Read(_) => {
                self.metrics.active_readers.fetch_sub(1, Ordering::Relaxed);
            }
            ConnectionPermit::Write(_) => {
                self.metrics.active_writers.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

impl std::ops::Deref for PooledConnection {
    type Target = duckdb::Connection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}
```

## Tests

### Test 1: Single Writer
```rust
#[tokio::test]
async fn test_single_writer() {
    let pool = DuckConnectionPool::new(":memory:", PoolConfig::default()).await.unwrap();

    let write1 = pool.get_write_connection().await.unwrap();

    // Second write should block
    let write2_future = pool.get_write_connection();

    // Use timeout to verify it blocks
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        write2_future
    ).await;

    assert!(result.is_err()); // Should timeout

    // Release first writer
    drop(write1);

    // Now second should succeed
    let write2 = pool.get_write_connection().await;
    assert!(write2.is_ok());
}
```

### Test 2: Multiple Readers
```rust
#[tokio::test]
async fn test_multiple_readers() {
    let config = PoolConfig {
        max_readers: 5,
        ..Default::default()
    };
    let pool = DuckConnectionPool::new(":memory:", config).await.unwrap();

    // Should be able to get 5 readers concurrently
    let readers: Vec<_> = futures::future::join_all(
        (0..5).map(|_| pool.get_connection())
    ).await;

    assert!(readers.iter().all(|r| r.is_ok()));

    // 6th reader should timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        pool.get_connection()
    ).await;

    assert!(result.is_err());
}
```

### Test 3: Metrics
```rust
#[tokio::test]
async fn test_metrics() {
    let pool = DuckConnectionPool::new(":memory:", PoolConfig::default()).await.unwrap();

    let _r1 = pool.get_connection().await.unwrap();
    let _r2 = pool.get_connection().await.unwrap();
    let _w1 = pool.get_write_connection().await.unwrap();

    let metrics = pool.metrics();
    assert_eq!(metrics.reads_acquired, 2);
    assert_eq!(metrics.writes_acquired, 1);
    assert_eq!(metrics.active_readers, 2);
    assert_eq!(metrics.active_writers, 1);
}
```

### Test 4: Timeout Returns Correct Error
```rust
#[tokio::test]
async fn test_timeout_returns_error() {
    let config = PoolConfig {
        max_readers: 1,
        acquire_timeout_ms: 50,
    };
    let pool = DuckConnectionPool::new(":memory:", config).await.unwrap();

    // Exhaust the single reader slot
    let _hold = pool.get_connection().await.unwrap();

    // Next request should timeout and return specific error
    let result = pool.get_connection().await;
    assert!(matches!(result, Err(Error::ConnectionPoolTimeout)));

    // Verify timeout metric incremented
    let metrics = pool.metrics();
    assert_eq!(metrics.timeouts, 1);
}
```

### Test 5: RAII Cleanup Releases Permit
```rust
#[tokio::test]
async fn test_raii_cleanup() {
    let config = PoolConfig {
        max_readers: 1,
        acquire_timeout_ms: 100,
    };
    let pool = DuckConnectionPool::new(":memory:", config).await.unwrap();

    {
        let _conn = pool.get_connection().await.unwrap();
        assert_eq!(pool.metrics().active_readers, 1);
    }

    // After drop, permit should be released
    assert_eq!(pool.metrics().active_readers, 0);

    // Should be able to acquire again immediately
    let _conn2 = pool.get_connection().await.unwrap();
    assert_eq!(pool.metrics().active_readers, 1);
}
```

### Test 6: Graceful Shutdown
```rust
#[tokio::test]
async fn test_graceful_shutdown() {
    let pool = DuckConnectionPool::new(":memory:", PoolConfig::default()).await.unwrap();
    let conn = pool.get_connection().await.unwrap();

    // Start shutdown in background
    let pool_clone = pool.clone();
    let shutdown = tokio::spawn(async move {
        pool_clone.close().await
    });

    // Shutdown should wait for active connection
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!shutdown.is_finished());

    // Release connection
    drop(conn);

    // Now shutdown should complete
    shutdown.await.unwrap().unwrap();

    // New connections should fail with pool closed error
    assert!(matches!(pool.get_connection().await, Err(Error::ConnectionPoolClosed)));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/duckdb_pool.rs` | Pool implementation (new) |
| `sdk/rust/src/connection_pool.rs` | SQLite pool (reference) |
| `sdk/rust/src/filesystem/duckagentfs.rs` | Pool consumer |

## Implementation Notes

1. **DuckDB Specifics**: Verify if DuckDB Rust bindings support connections on different threads

2. **WAL Mode**: DuckDB doesn't use WAL like SQLite, but has checkpointing

3. **In-Memory**: For tests, use `:memory:` as path

4. **Graceful Shutdown**: Implement `close()` method that waits for active connections

## QA Notes

### Test Coverage Summary

| Area | Coverage | Notes |
|------|----------|-------|
| Single-writer semantics | ✅ Covered | Test 1 validates exclusive write access |
| Multiple readers | ✅ Covered | Test 2 validates concurrent read access |
| Pool metrics | ✅ Covered | Test 3 validates metric tracking |
| Timeout behavior | ✅ Covered | Test 4 validates timeout error and metric |
| RAII cleanup | ✅ Covered | Test 5 validates permit release on drop |
| Graceful shutdown | ✅ Covered | Test 6 validates close() behavior |
| Retry on busy | N/A | Descoped to STORY-1.3.1 |
| Connection creation errors | ⚠️ Future | Consider adding in implementation phase |

### Risk Areas Identified

1. **MEDIUM - Connection Lifetime**: No tests validate connection behavior under prolonged use or connection staleness. DuckDB connections may behave differently than SQLite under stress.

2. **MEDIUM - Thread Safety**: Implementation note #1 questions DuckDB thread support but no tests validate cross-thread connection usage.

3. **LOW - Metric Overflow**: `AtomicU64` counters could overflow under extreme load. Consider wrapping or resetting strategy.

### Recommended Future Test Scenarios

| Scenario | Priority | Given-When-Then |
|----------|----------|-----------------|
| Connection creation failure | P2 | **Given** invalid database path, **When** connection created, **Then** appropriate error returned |
| Cross-thread connection use | P2 | **Given** connection from pool, **When** used on different thread, **Then** operations succeed |
| Metrics under concurrent load | P2 | **Given** high concurrency, **When** metrics snapshotted, **Then** values are consistent |
| Stress test | P3 | **Given** high concurrency, **When** many reads/writes, **Then** semaphores behave correctly |

### Sprint Change Proposal Applied

**Date**: 2026-01-14

**Changes Made**:
- Removed "Automatic retry on busy" from scope (deferred to STORY-1.3.1)
- Removed `retry_count` and `retry_delay_ms` from PoolConfig
- Removed `retries` from PoolMetrics
- Added Test 4: Timeout Returns Correct Error
- Added Test 5: RAII Cleanup Releases Permit
- Added Test 6: Graceful Shutdown
- Added `close()` method for graceful shutdown
- Replaced stubbed `create_connection()` with real `duckdb::Connection::open()`

### QA Gate Status

**Status**: APPROVED

**Rationale**: Core functionality is well-designed with comprehensive test coverage for all acceptance criteria. Retry mechanism appropriately descoped to separate story. All P0/P1 test scenarios now covered.

**Reviewed**: 2026-01-14 | PO: Sarah | QA: Quinn (Test Architect)
