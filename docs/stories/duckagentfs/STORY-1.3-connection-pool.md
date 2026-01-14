# STORY-1.3: Connection Pool DuckDB

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.3 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Todo |
| **Priority** | High |
| **File** | `sdk/rust/src/duckdb_pool.rs` (new) |
| **Dependencies** | STORY-1.1 |

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
- [ ] Automatic retry on busy

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

    /// Retry count on busy
    pub retry_count: usize,

    /// Retry delay (ms)
    pub retry_delay_ms: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_readers: 10,
            acquire_timeout_ms: 5000,
            retry_count: 3,
            retry_delay_ms: 100,
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

    /// Total retries
    pub retries: AtomicU64,

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
            retries: AtomicU64::new(0),
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
            retries: self.retries.load(Ordering::Relaxed),
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

    fn create_connection(&self) -> Result<DuckConnection> {
        // NOTE: Use actual DuckDB connection creation
        // duckdb::Connection::open(&self.path)
        Ok(DuckConnection { path: self.path.clone() })
    }

    /// Get current metrics
    pub fn metrics(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
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
    conn: DuckConnection,
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
    type Target = DuckConnection;

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
