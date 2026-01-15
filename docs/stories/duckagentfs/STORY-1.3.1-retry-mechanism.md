# STORY-1.3.1: Connection Pool Retry Mechanism

> **NOTE**: This story was descoped from STORY-1.3 per Sprint Change Proposal (2026-01-14).

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.3.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Backlog |
| **Priority** | Low |
| **File** | `sdk/rust/src/duckdb_pool.rs` |
| **Dependencies** | STORY-1.3 |

## User Story

**As a** developer
**I want** automatic retry on connection busy errors
**So that** transient failures don't require manual retry logic in application code

## Background

DuckDB uses single-writer semantics. When a write operation is in progress, subsequent write attempts may receive a "busy" error. This story adds automatic retry logic with configurable backoff to handle these transient failures gracefully.

**Note**: DuckDB's "busy" semantics differ from SQLite's. Investigation is needed to determine:
1. What errors DuckDB returns when busy
2. Whether semaphore-based blocking (current design) already handles this
3. If additional retry logic provides value beyond semaphore blocking

## Acceptance Criteria

- [ ] Configurable retry count (default: 3)
- [ ] Configurable retry delay with exponential backoff
- [ ] `retries` metric added to PoolMetrics
- [ ] Test: retry succeeds within configured attempts
- [ ] Test: retry exhaustion returns appropriate error
- [ ] Documentation of DuckDB busy error behavior

## Technical Specification

### PoolConfig Extension

```rust
pub struct PoolConfig {
    /// Maximum concurrent readers
    pub max_readers: usize,

    /// Timeout for acquiring connection (ms)
    pub acquire_timeout_ms: u64,

    /// Retry count on busy (default: 3)
    pub retry_count: usize,

    /// Initial retry delay (ms), doubles each retry
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

### Retry Logic

```rust
impl DuckConnectionPool {
    async fn with_retry<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Result<T>,
    {
        let mut attempts = 0;
        let mut delay = self.config.retry_delay_ms;

        loop {
            match operation() {
                Ok(result) => return Ok(result),
                Err(Error::DatabaseBusy) if attempts < self.config.retry_count => {
                    attempts += 1;
                    self.metrics.retries.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    delay *= 2; // Exponential backoff
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

## Tests

### Test 1: Retry Success
```rust
#[tokio::test]
async fn test_retry_success() {
    // Given: pool with retry enabled and a busy database
    // When: connection requested
    // Then: retry succeeds within configured attempts
}
```

### Test 2: Retry Exhaustion
```rust
#[tokio::test]
async fn test_retry_exhaustion() {
    // Given: pool with retry_count=2 and permanently busy database
    // When: connection requested
    // Then: returns Error::DatabaseBusy after 2 retries
    // And: metrics.retries == 2
}
```

## Investigation Required

Before implementation, investigate:

1. **DuckDB Busy Errors**: What specific error does DuckDB return when write is blocked?
2. **Semaphore vs Retry**: Does the semaphore-based blocking already handle contention?
3. **Use Cases**: When would retry be needed beyond semaphore blocking?

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/duckdb_pool.rs` | Pool implementation |
| `docs/stories/duckagentfs/STORY-1.3-connection-pool.md` | Parent story |

## Origin

**Descoped from**: STORY-1.3
**Reason**: Retry mechanism was in acceptance criteria but not implemented. Semaphore-based blocking may already handle contention. Deferring to investigate DuckDB-specific busy semantics.
**Sprint Change Proposal**: 2026-01-14 | PO: Sarah
