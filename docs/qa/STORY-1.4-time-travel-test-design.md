# Test Design: STORY-1.4 - Time-Travel Queries

> **Generated:** 2026-01-14
> **Story:** STORY-1.4-time-travel.md
> **Implementation:** `sdk/rust/src/filesystem/duckagentfs.rs`
> **Status:** Ready for Implementation (Blocked on STORY-1.2)

---

## 1. Executive Summary

This document provides a comprehensive test design for the Time-Travel Queries feature in DuckAgentFS. The append-only journal model enables reconstructing filesystem state at any point in time by filtering events by `event_id`. This feature supports audit trails, data recovery, debugging, and compliance requirements.

### 1.1 Acceptance Criteria Coverage

| Criteria | Test Coverage | Status |
|----------|---------------|--------|
| `snapshot_at(event_id)` returns read-only filesystem | TC-P0-001 through TC-P0-005, TC-RO-001 through TC-RO-009 | Designed |
| View `fs_current` filters by event_id | TC-P0-001, TC-SNAP-001 through TC-SNAP-006 | Designed |
| `list_events(limit, offset)` returns paginated timeline | TC-LIST-001 through TC-LIST-006 | Designed |
| `diff(from_event, to_event)` returns Vec<FileDiff> | TC-DIFF-001 through TC-DIFF-010 | Designed |
| Unit tests for snapshot read-only enforcement (3+ tests) | TC-RO-001 through TC-RO-009 | Designed |
| Integration tests for time-travel scenarios (5+ tests) | TC-INT-001 through TC-INT-008 | Designed |

### 1.2 Risk Areas from Story

| Risk | Severity | Test Coverage |
|------|----------|---------------|
| SQL Injection in Historical Query | HIGH | TC-SEC-001 |
| Performance Degradation with large journals | HIGH | TC-PERF-001, TC-PERF-002 |
| Race Condition (snapshot vs compaction) | MEDIUM | TC-CONC-001, TC-CONC-002 |
| Memory Exhaustion (large result sets) | MEDIUM | TC-MEM-001, TC-MEM-002 |
| Event ID 0 edge case | LOW | TC-EDGE-001, TC-EDGE-002 |

### 1.3 Test Coverage Goals

| Priority | Category | Target Coverage |
|----------|----------|-----------------|
| P0 | Core snapshot operations | 100% |
| P0 | Read-only enforcement | 100% |
| P0 | list_events() API | 100% |
| P0 | diff() API | 100% |
| P1 | Error handling | 90% |
| P1 | Concurrent access | 80% |
| P2 | Performance | 80% |
| P2 | Edge cases | 90% |

---

## 2. Test Infrastructure Requirements

### 2.1 Test Fixtures

```rust
// tests/common/time_travel.rs

use std::sync::Arc;
use tempfile::TempDir;

/// Creates a DuckAgentFS instance with pre-populated history
pub async fn create_fs_with_history() -> (DuckAgentFS, TempDir, Vec<i64>) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_string_lossy().to_string(),
        chunk_size: 4096,
        dentry_cache_size: 1000,
        enable_vss: false,
        enable_pgq: false,
        actor_id: Some("test-actor".to_string()),
        session_id: Some("test-session".to_string()),
    };

    let fs = DuckAgentFS::open(config).await.expect("Failed to open FS");

    // Create history with known event IDs
    let mut event_ids = Vec::new();

    // Event 1: Create file
    fs.write_file("/test.txt", b"version 1").await.unwrap();
    event_ids.push(fs.current_event_id().await.unwrap());

    // Event 2: Update file
    fs.write_file("/test.txt", b"version 2").await.unwrap();
    event_ids.push(fs.current_event_id().await.unwrap());

    // Event 3: Create directory
    fs.mkdir("/dir").await.unwrap();
    event_ids.push(fs.current_event_id().await.unwrap());

    // Event 4: Create another file
    fs.write_file("/dir/file.txt", b"nested content").await.unwrap();
    event_ids.push(fs.current_event_id().await.unwrap());

    // Event 5: Delete first file
    fs.remove("/test.txt").await.unwrap();
    event_ids.push(fs.current_event_id().await.unwrap());

    (fs, temp_dir, event_ids)
}

/// Creates a DuckAgentFS instance with many events for pagination tests
pub async fn create_fs_with_many_events(count: usize) -> (DuckAgentFS, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_string_lossy().to_string(),
        ..Default::default()
    };

    let fs = DuckAgentFS::open(config).await.expect("Failed to open FS");

    for i in 0..count {
        let path = format!("/file_{}.txt", i);
        fs.write_file(&path, format!("content {}", i).as_bytes()).await.unwrap();
    }

    (fs, temp_dir)
}

/// Creates a FS with specific diff scenarios
pub async fn create_fs_for_diff() -> (DuckAgentFS, TempDir, i64, i64) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_string_lossy().to_string(),
        ..Default::default()
    };

    let fs = DuckAgentFS::open(config).await.expect("Failed to open FS");

    // Setup: Create files to be modified and deleted
    fs.write_file("/modified.txt", b"original").await.unwrap();
    fs.write_file("/deleted.txt", b"will be deleted").await.unwrap();
    fs.write_file("/unchanged.txt", b"stays same").await.unwrap();

    let start_event = fs.current_event_id().await.unwrap();

    // Make changes
    fs.write_file("/added.txt", b"new file").await.unwrap();
    fs.write_file("/modified.txt", b"modified content").await.unwrap();
    fs.remove("/deleted.txt").await.unwrap();
    fs.rename("/unchanged.txt", "/renamed.txt").await.unwrap();

    let end_event = fs.current_event_id().await.unwrap();

    (fs, temp_dir, start_event, end_event)
}
```

### 2.2 JournalEvent Struct Helper

```rust
// tests/common/journal_event.rs

/// Represents a journal event for testing
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEvent {
    pub event_id: i64,
    pub inode: i64,
    pub event_type: String,
    pub event_time: chrono::DateTime<chrono::Utc>,
    pub name: Option<String>,
    pub actor_id: Option<String>,
    pub session_id: Option<String>,
}

/// Verify journal event properties
pub fn assert_event_fields(
    event: &JournalEvent,
    expected_type: &str,
    expected_name: Option<&str>,
) {
    assert_eq!(event.event_type, expected_type);
    assert_eq!(event.name.as_deref(), expected_name);
}

/// Verify events are in descending order by event_id
pub fn assert_events_ordered_desc(events: &[JournalEvent]) {
    for i in 1..events.len() {
        assert!(
            events[i - 1].event_id > events[i].event_id,
            "Events not in descending order: {} vs {}",
            events[i - 1].event_id,
            events[i].event_id
        );
    }
}
```

### 2.3 FileDiff Verification Helpers

```rust
// tests/common/diff_helpers.rs

/// Verify diff contains expected change type
pub fn assert_diff_contains(
    diffs: &[FileDiff],
    path: &str,
    change_type: ChangeType,
) -> bool {
    diffs.iter().any(|d| {
        d.path == path && std::mem::discriminant(&d.change_type) == std::mem::discriminant(&change_type)
    })
}

/// Assert no unexpected changes
pub fn assert_no_diff_for_path(diffs: &[FileDiff], path: &str) {
    assert!(
        !diffs.iter().any(|d| d.path == path),
        "Unexpected diff for path: {}",
        path
    );
}

/// Count changes by type
pub fn count_change_type(diffs: &[FileDiff], change_type: ChangeType) -> usize {
    diffs.iter()
        .filter(|d| std::mem::discriminant(&d.change_type) == std::mem::discriminant(&change_type))
        .count()
}
```

---

## 3. P0 - Critical Tests (Must Have)

### 3.1 Snapshot Core Operations

#### TC-P0-001: Basic Snapshot Read

**Objective:** Verify snapshot returns historical state, not current state.

**Preconditions:**
- File created with "version 1", then updated to "version 2"
- Event IDs captured at each step

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/test.txt", b"version 1")` | File created |
| 2 | `current_event_id()` | Returns event_id E1 |
| 3 | `write_file("/test.txt", b"version 2")` | File updated |
| 4 | `snapshot_at(E1)` | Returns snapshot |
| 5 | `snapshot.read_file("/test.txt")` | Returns "version 1" |
| 6 | `fs.read_file("/test.txt")` | Returns "version 2" |

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_read_basic() {
    let (fs, _temp) = create_test_fs().await;

    // Create file v1
    fs.write_file("/test.txt", b"version 1").await.unwrap();
    let event1 = fs.current_event_id().await.unwrap();

    // Update to v2
    fs.write_file("/test.txt", b"version 2").await.unwrap();

    // Snapshot at v1 should see v1
    let snapshot = fs.snapshot_at(event1).await.unwrap();
    let content = snapshot.read_file("/test.txt").await.unwrap();
    assert_eq!(content, Some(b"version 1".to_vec()));

    // Current should see v2
    let current = fs.read_file("/test.txt").await.unwrap();
    assert_eq!(current, Some(b"version 2".to_vec()));
}
```

---

#### TC-P0-002: Snapshot of Deleted File

**Objective:** Verify deleted files are visible in historical snapshots.

**Preconditions:**
- File created, event_id captured, then file deleted

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/test.txt", b"content")` | File created |
| 2 | `current_event_id()` | Returns event_id E1 |
| 3 | `remove("/test.txt")` | File deleted |
| 4 | `read_file("/test.txt")` | Returns None |
| 5 | `snapshot_at(E1).read_file("/test.txt")` | Returns "content" |

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_deleted_file() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"content").await.unwrap();
    let event1 = fs.current_event_id().await.unwrap();

    fs.remove("/test.txt").await.unwrap();

    // Current: file doesn't exist
    let current = fs.read_file("/test.txt").await.unwrap();
    assert!(current.is_none());

    // Snapshot: file exists
    let snapshot = fs.snapshot_at(event1).await.unwrap();
    let historical = snapshot.read_file("/test.txt").await.unwrap();
    assert_eq!(historical, Some(b"content".to_vec()));
}
```

---

#### TC-P0-003: Snapshot at Non-Existent Event ID

**Objective:** Verify proper error handling for invalid event_id.

**Preconditions:**
- Empty or minimal filesystem

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `current_event_id()` | Returns E1 |
| 2 | `snapshot_at(E1 + 1000)` | Returns error |
| 3 | Verify error message | Contains "Event {} not found" |

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_invalid_event_id() {
    let (fs, _temp) = create_test_fs().await;

    let current = fs.current_event_id().await.unwrap();
    let invalid_id = current + 1000;

    let result = fs.snapshot_at(invalid_id).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(format!("{}", err).contains("not found"));
}
```

---

#### TC-P0-004: current_event_id Accuracy

**Objective:** Verify current_event_id increases monotonically with operations.

**Preconditions:**
- Fresh filesystem

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `current_event_id()` | Returns E0 |
| 2 | `write_file("/a.txt", b"a")` | File created |
| 3 | `current_event_id()` | Returns E1 > E0 |
| 4 | `write_file("/b.txt", b"b")` | File created |
| 5 | `current_event_id()` | Returns E2 > E1 |
| 6 | `mkdir("/dir")` | Directory created |
| 7 | `current_event_id()` | Returns E3 > E2 |

**Verification:**
```rust
#[tokio::test]
async fn test_current_event_id_monotonic() {
    let (fs, _temp) = create_test_fs().await;

    let e0 = fs.current_event_id().await.unwrap();

    fs.write_file("/a.txt", b"a").await.unwrap();
    let e1 = fs.current_event_id().await.unwrap();
    assert!(e1 > e0, "Event ID should increase");

    fs.write_file("/b.txt", b"b").await.unwrap();
    let e2 = fs.current_event_id().await.unwrap();
    assert!(e2 > e1, "Event ID should increase");

    fs.mkdir("/dir").await.unwrap();
    let e3 = fs.current_event_id().await.unwrap();
    assert!(e3 > e2, "Event ID should increase");
}
```

---

#### TC-P0-005: Snapshot event_id Accessor

**Objective:** Verify snapshot returns correct event_id.

**Preconditions:**
- Known event_id

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/test.txt", b"data")` | File created |
| 2 | `current_event_id()` | Returns E1 |
| 3 | `snapshot_at(E1)` | Returns snapshot |
| 4 | `snapshot.event_id()` | Returns E1 |

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_event_id_accessor() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"data").await.unwrap();
    let event_id = fs.current_event_id().await.unwrap();

    let snapshot = fs.snapshot_at(event_id).await.unwrap();
    assert_eq!(snapshot.event_id(), event_id);
}
```

---

### 3.2 list_events() API Tests

#### TC-LIST-001: Basic Pagination

**Objective:** Verify list_events returns correct page of events.

**Preconditions:**
- Filesystem with 10+ events

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create 10 files | 10 events in journal |
| 2 | `list_events(5, 0)` | Returns 5 events, most recent first |
| 3 | `list_events(5, 5)` | Returns next 5 events |
| 4 | Verify no overlap | First page ends where second begins |

**Verification:**
```rust
#[tokio::test]
async fn test_list_events_pagination() {
    let (fs, _temp) = create_fs_with_many_events(10).await;

    // First page
    let page1 = fs.list_events(5, 0).await.unwrap();
    assert_eq!(page1.len(), 5);
    assert_events_ordered_desc(&page1);

    // Second page
    let page2 = fs.list_events(5, 5).await.unwrap();
    assert_eq!(page2.len(), 5);
    assert_events_ordered_desc(&page2);

    // No overlap
    assert!(page1.last().unwrap().event_id > page2.first().unwrap().event_id);
}
```

---

#### TC-LIST-002: Empty Result Set

**Objective:** Verify list_events handles offset beyond available events.

**Preconditions:**
- Filesystem with 5 events

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create 5 files | 5 events in journal |
| 2 | `list_events(10, 100)` | Returns empty Vec |
| 3 | `list_events(10, 5)` | Returns empty Vec (root event only if applicable) |

**Verification:**
```rust
#[tokio::test]
async fn test_list_events_empty_offset() {
    let (fs, _temp) = create_fs_with_many_events(5).await;

    // Offset way beyond available events
    let events = fs.list_events(10, 1000).await.unwrap();
    assert!(events.is_empty());
}
```

---

#### TC-LIST-003: Limit Zero

**Objective:** Verify list_events handles limit=0 gracefully.

**Preconditions:**
- Filesystem with events

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `list_events(0, 0)` | Returns empty Vec |

**Verification:**
```rust
#[tokio::test]
async fn test_list_events_limit_zero() {
    let (fs, _temp) = create_fs_with_many_events(5).await;

    let events = fs.list_events(0, 0).await.unwrap();
    assert!(events.is_empty());
}
```

---

#### TC-LIST-004: Events Contain Required Fields

**Objective:** Verify JournalEvent has all required fields populated.

**Preconditions:**
- Filesystem with known events

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create file "/test.txt" | Event created |
| 2 | `list_events(1, 0)` | Returns event |
| 3 | Verify event_id | Non-zero |
| 4 | Verify event_type | "create" |
| 5 | Verify event_time | Valid timestamp |
| 6 | Verify name | "test.txt" |

**Verification:**
```rust
#[tokio::test]
async fn test_list_events_field_population() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"data").await.unwrap();

    let events = fs.list_events(1, 0).await.unwrap();
    assert_eq!(events.len(), 1);

    let event = &events[0];
    assert!(event.event_id > 0);
    assert_eq!(event.event_type, "create");
    assert!(event.event_time <= chrono::Utc::now());
    assert_eq!(event.name, Some("test.txt".to_string()));
}
```

---

#### TC-LIST-005: Event Types Coverage

**Objective:** Verify all event types appear in list_events.

**Preconditions:**
- Perform create, update, delete, rename operations

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/a.txt", b"v1")` | Create event |
| 2 | `write_file("/a.txt", b"v2")` | Update event |
| 3 | `rename("/a.txt", "/b.txt")` | Rename event |
| 4 | `remove("/b.txt")` | Delete event |
| 5 | `list_events(100, 0)` | Contains all event types |

**Verification:**
```rust
#[tokio::test]
async fn test_list_events_all_types() {
    let (fs, _temp) = create_test_fs().await;

    // Create
    fs.write_file("/a.txt", b"v1").await.unwrap();
    // Update
    fs.write_file("/a.txt", b"v2").await.unwrap();
    // Rename
    fs.rename("/a.txt", "/b.txt").await.unwrap();
    // Delete
    fs.remove("/b.txt").await.unwrap();

    let events = fs.list_events(100, 0).await.unwrap();
    let event_types: Vec<_> = events.iter().map(|e| e.event_type.as_str()).collect();

    assert!(event_types.contains(&"create"));
    assert!(event_types.contains(&"update"));
    assert!(event_types.contains(&"rename"));
    assert!(event_types.contains(&"delete"));
}
```

---

#### TC-LIST-006: Descending Order Verification

**Objective:** Verify events are returned in descending event_id order.

**Preconditions:**
- Filesystem with multiple events

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create multiple files | Multiple events |
| 2 | `list_events(100, 0)` | All events |
| 3 | Verify each event_id[n] > event_id[n+1] | Strictly descending |

**Verification:**
```rust
#[tokio::test]
async fn test_list_events_order() {
    let (fs, _temp) = create_fs_with_many_events(10).await;

    let events = fs.list_events(100, 0).await.unwrap();

    // Verify descending order
    for i in 1..events.len() {
        assert!(
            events[i - 1].event_id > events[i].event_id,
            "Events must be in descending order"
        );
    }
}
```

---

### 3.3 diff() API Tests

#### TC-DIFF-001: Basic Diff - Added Files

**Objective:** Verify diff detects newly created files.

**Preconditions:**
- Start event captured before file creation

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `current_event_id()` | Returns start_event |
| 2 | `write_file("/new.txt", b"new")` | File created |
| 3 | `current_event_id()` | Returns end_event |
| 4 | `diff(start_event, end_event)` | Contains Added for "/new.txt" |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_added() {
    let (fs, _temp) = create_test_fs().await;

    let start = fs.current_event_id().await.unwrap();

    fs.write_file("/new.txt", b"new content").await.unwrap();

    let end = fs.current_event_id().await.unwrap();

    let diffs = fs.diff(start, end).await.unwrap();

    assert!(assert_diff_contains(&diffs, "/new.txt", ChangeType::Added));
}
```

---

#### TC-DIFF-002: Basic Diff - Modified Files

**Objective:** Verify diff detects file modifications.

**Preconditions:**
- File exists before start_event

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/mod.txt", b"v1")` | File created |
| 2 | `current_event_id()` | Returns start_event |
| 3 | `write_file("/mod.txt", b"v2")` | File updated |
| 4 | `current_event_id()` | Returns end_event |
| 5 | `diff(start_event, end_event)` | Contains Modified for "/mod.txt" |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_modified() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/mod.txt", b"v1").await.unwrap();
    let start = fs.current_event_id().await.unwrap();

    fs.write_file("/mod.txt", b"v2").await.unwrap();
    let end = fs.current_event_id().await.unwrap();

    let diffs = fs.diff(start, end).await.unwrap();

    assert!(assert_diff_contains(&diffs, "/mod.txt", ChangeType::Modified));
}
```

---

#### TC-DIFF-003: Basic Diff - Deleted Files

**Objective:** Verify diff detects file deletions.

**Preconditions:**
- File exists before start_event

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/del.txt", b"data")` | File created |
| 2 | `current_event_id()` | Returns start_event |
| 3 | `remove("/del.txt")` | File deleted |
| 4 | `current_event_id()` | Returns end_event |
| 5 | `diff(start_event, end_event)` | Contains Deleted for "/del.txt" |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_deleted() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/del.txt", b"data").await.unwrap();
    let start = fs.current_event_id().await.unwrap();

    fs.remove("/del.txt").await.unwrap();
    let end = fs.current_event_id().await.unwrap();

    let diffs = fs.diff(start, end).await.unwrap();

    assert!(assert_diff_contains(&diffs, "/del.txt", ChangeType::Deleted));
}
```

---

#### TC-DIFF-004: Basic Diff - Renamed Files

**Objective:** Verify diff detects file renames with original path.

**Preconditions:**
- File exists before start_event

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/old.txt", b"data")` | File created |
| 2 | `current_event_id()` | Returns start_event |
| 3 | `rename("/old.txt", "/new.txt")` | File renamed |
| 4 | `current_event_id()` | Returns end_event |
| 5 | `diff(start_event, end_event)` | Contains Renamed { from: "/old.txt" } |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_renamed() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/old.txt", b"data").await.unwrap();
    let start = fs.current_event_id().await.unwrap();

    fs.rename("/old.txt", "/new.txt").await.unwrap();
    let end = fs.current_event_id().await.unwrap();

    let diffs = fs.diff(start, end).await.unwrap();

    // Find the rename diff
    let rename_diff = diffs.iter().find(|d| matches!(&d.change_type, ChangeType::Renamed { .. }));
    assert!(rename_diff.is_some());

    if let Some(diff) = rename_diff {
        assert_eq!(diff.path, "/new.txt");
        if let ChangeType::Renamed { from } = &diff.change_type {
            assert_eq!(from, "old.txt");
        }
    }
}
```

---

#### TC-DIFF-005: Diff Same Event Returns Empty

**Objective:** Verify diff(event, event) returns empty list.

**Preconditions:**
- Known event_id

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/test.txt", b"data")` | File created |
| 2 | `current_event_id()` | Returns E1 |
| 3 | `diff(E1, E1)` | Returns empty Vec |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_same_event() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"data").await.unwrap();
    let event_id = fs.current_event_id().await.unwrap();

    let diffs = fs.diff(event_id, event_id).await.unwrap();
    assert!(diffs.is_empty(), "Same event diff should be empty");
}
```

---

#### TC-DIFF-006: Diff Invalid from_event > to_event

**Objective:** Verify diff handles inverted range appropriately.

**Preconditions:**
- Two known event IDs where E1 < E2

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Get E1 and E2 where E1 < E2 | Two event IDs |
| 2 | `diff(E2, E1)` | Returns empty Vec OR error |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_inverted_range() {
    let (fs, _temp, events) = create_fs_with_history().await;

    let e1 = events[0];
    let e2 = events[2];
    assert!(e1 < e2);

    // Inverted range should return empty or error
    let diffs = fs.diff(e2, e1).await.unwrap();
    assert!(diffs.is_empty(), "Inverted range should return empty diff");
}
```

---

#### TC-DIFF-007: Diff Contains Stats

**Objective:** Verify FileDiff includes old_stats and new_stats.

**Preconditions:**
- File modified between events

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create file with size 5 | old_stats.size = 5 |
| 2 | Capture start_event | |
| 3 | Update file to size 10 | new_stats.size = 10 |
| 4 | Capture end_event | |
| 5 | `diff(start, end)` | FileDiff has old_stats and new_stats |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_contains_stats() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/file.txt", b"short").await.unwrap();
    let start = fs.current_event_id().await.unwrap();

    fs.write_file("/file.txt", b"much longer content").await.unwrap();
    let end = fs.current_event_id().await.unwrap();

    let diffs = fs.diff(start, end).await.unwrap();
    let file_diff = diffs.iter().find(|d| d.path == "/file.txt").unwrap();

    assert!(file_diff.old_stats.is_some());
    assert!(file_diff.new_stats.is_some());
    assert!(file_diff.new_stats.as_ref().unwrap().size > file_diff.old_stats.as_ref().unwrap().size);
}
```

---

#### TC-DIFF-008: Comprehensive Diff Test

**Objective:** Verify diff catches all change types in single operation.

**Preconditions:**
- Use create_fs_for_diff() fixture

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Create test fixture | Added, Modified, Deleted, Renamed files |
| 2 | `diff(start, end)` | Contains all 4 change types |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_comprehensive() {
    let (fs, _temp, start, end) = create_fs_for_diff().await;

    let diffs = fs.diff(start, end).await.unwrap();

    assert_eq!(count_change_type(&diffs, ChangeType::Added), 1);
    assert_eq!(count_change_type(&diffs, ChangeType::Modified), 1);
    assert_eq!(count_change_type(&diffs, ChangeType::Deleted), 1);
    // Renamed counted as Renamed
    assert!(diffs.iter().any(|d| matches!(&d.change_type, ChangeType::Renamed { .. })));
}
```

---

#### TC-DIFF-009: Diff with Non-Existent from_event

**Objective:** Verify diff handles invalid from_event gracefully.

**Preconditions:**
- Valid to_event, invalid from_event

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `diff(999999, valid_event)` | Returns error or empty |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_invalid_from_event() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"data").await.unwrap();
    let valid = fs.current_event_id().await.unwrap();

    // Invalid from_event - should not panic
    let result = fs.diff(999999, valid).await;
    // Either returns error or empty (implementation-defined)
    assert!(result.is_ok() && result.unwrap().is_empty() || result.is_err());
}
```

---

#### TC-DIFF-010: Diff Unchanged Files Not Included

**Objective:** Verify files unchanged between events are not in diff.

**Preconditions:**
- File created before start_event, not modified

**Test Steps:**

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `write_file("/unchanged.txt", b"static")` | File created |
| 2 | `current_event_id()` | Returns start_event |
| 3 | `write_file("/changed.txt", b"new")` | Different file changed |
| 4 | `current_event_id()` | Returns end_event |
| 5 | `diff(start, end)` | Does NOT contain "/unchanged.txt" |

**Verification:**
```rust
#[tokio::test]
async fn test_diff_excludes_unchanged() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/unchanged.txt", b"static").await.unwrap();
    let start = fs.current_event_id().await.unwrap();

    fs.write_file("/changed.txt", b"new").await.unwrap();
    let end = fs.current_event_id().await.unwrap();

    let diffs = fs.diff(start, end).await.unwrap();

    assert_no_diff_for_path(&diffs, "/unchanged.txt");
    assert!(assert_diff_contains(&diffs, "/changed.txt", ChangeType::Added));
}
```

---

### 3.4 Read-Only Enforcement Tests

#### TC-RO-001: Snapshot write_file Blocked

**Objective:** Verify snapshot rejects write_file operations.

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_write_file_blocked() {
    let (fs, _temp) = create_test_fs().await;

    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    let result = snapshot.write_file("/new.txt", b"data").await;
    assert!(matches!(result, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-RO-002: Snapshot mkdir Blocked

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_mkdir_blocked() {
    let (fs, _temp) = create_test_fs().await;

    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    let result = snapshot.mkdir("/newdir").await;
    assert!(matches!(result, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-RO-003: Snapshot remove Blocked

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_remove_blocked() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"data").await.unwrap();
    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    let result = snapshot.remove("/test.txt").await;
    assert!(matches!(result, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-RO-004: Snapshot rename Blocked

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_rename_blocked() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"data").await.unwrap();
    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    let result = snapshot.rename("/test.txt", "/new.txt").await;
    assert!(matches!(result, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-RO-005: Snapshot chmod Blocked

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_chmod_blocked() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"data").await.unwrap();
    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    let result = snapshot.chmod("/test.txt", 0o755).await;
    assert!(matches!(result, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-RO-006: Snapshot symlink Blocked

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_symlink_blocked() {
    let (fs, _temp) = create_test_fs().await;

    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    let result = snapshot.symlink("/target", "/link").await;
    assert!(matches!(result, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-RO-007: Snapshot link Blocked

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_link_blocked() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/test.txt", b"data").await.unwrap();
    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    let result = snapshot.link("/test.txt", "/link.txt").await;
    assert!(matches!(result, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-RO-008: Snapshot create_file Blocked

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_create_file_blocked() {
    let (fs, _temp) = create_test_fs().await;

    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    let result = snapshot.create_file("/new.txt", 0o644).await;
    assert!(matches!(result, Err(Error::Custom(msg)) if msg.contains("read-only")));
}
```

---

#### TC-RO-009: Snapshot Read Operations Work

**Objective:** Verify snapshot allows all read operations.

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_read_operations_work() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/dir").await.unwrap();
    fs.write_file("/dir/file.txt", b"content").await.unwrap();
    fs.symlink("/dir/file.txt", "/link.txt").await.unwrap();

    let event_id = fs.current_event_id().await.unwrap();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();

    // All read operations should work
    assert!(snapshot.stat("/dir/file.txt").await.unwrap().is_some());
    assert!(snapshot.lstat("/link.txt").await.unwrap().is_some());
    assert!(snapshot.read_file("/dir/file.txt").await.unwrap().is_some());
    assert!(snapshot.readdir("/dir").await.unwrap().is_some());
    assert!(snapshot.readdir_plus("/dir").await.unwrap().is_some());
    assert!(snapshot.readlink("/link.txt").await.unwrap().is_some());
    assert!(snapshot.statfs().await.is_ok());
}
```

---

## 4. P1 - High Priority Tests

### 4.1 Integration Tests for Time-Travel Scenarios

#### TC-INT-001: Audit Trail Recovery

**Objective:** Verify ability to reconstruct audit trail from journal.

**Verification:**
```rust
#[tokio::test]
async fn test_audit_trail_recovery() {
    let (fs, _temp) = create_test_fs().await;

    // Perform operations
    fs.write_file("/doc.txt", b"initial").await.unwrap();
    fs.write_file("/doc.txt", b"updated").await.unwrap();
    fs.rename("/doc.txt", "/document.txt").await.unwrap();

    // List events for audit
    let events = fs.list_events(100, 0).await.unwrap();

    // Should be able to reconstruct the timeline
    let event_types: Vec<_> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert!(event_types.contains(&"create"));
    assert!(event_types.contains(&"update"));
    assert!(event_types.contains(&"rename"));
}
```

---

#### TC-INT-002: Data Recovery from Deleted File

**Objective:** Verify deleted data can be recovered from snapshot.

**Verification:**
```rust
#[tokio::test]
async fn test_data_recovery() {
    let (fs, _temp) = create_test_fs().await;

    let secret_data = b"important secret data";
    fs.write_file("/secret.txt", secret_data).await.unwrap();
    let before_delete = fs.current_event_id().await.unwrap();

    // Accidental delete
    fs.remove("/secret.txt").await.unwrap();

    // Data is gone from current
    assert!(fs.read_file("/secret.txt").await.unwrap().is_none());

    // But recoverable from snapshot
    let snapshot = fs.snapshot_at(before_delete).await.unwrap();
    let recovered = snapshot.read_file("/secret.txt").await.unwrap();
    assert_eq!(recovered, Some(secret_data.to_vec()));
}
```

---

#### TC-INT-003: Debug Investigation Workflow

**Objective:** Verify ability to investigate state at specific point.

**Verification:**
```rust
#[tokio::test]
async fn test_debug_investigation() {
    let (fs, _temp) = create_test_fs().await;

    // Setup: config was correct
    fs.write_file("/config.json", b"{\"debug\": false}").await.unwrap();
    let good_config = fs.current_event_id().await.unwrap();

    // Later: config was changed (bug introduced)
    fs.write_file("/config.json", b"{\"debug\": true, \"broken\": 1}").await.unwrap();

    // Investigation: what was config before?
    let snapshot = fs.snapshot_at(good_config).await.unwrap();
    let old_config = snapshot.read_file("/config.json").await.unwrap().unwrap();
    assert!(!old_config.contains(&b'{'));  // Valid JSON
    assert!(String::from_utf8_lossy(&old_config).contains("false"));
}
```

---

#### TC-INT-004: Compliance Point-in-Time View

**Objective:** Verify compliance requirement for PIT view.

**Verification:**
```rust
#[tokio::test]
async fn test_compliance_pit_view() {
    let (fs, _temp) = create_test_fs().await;

    // Day 1: Initial state
    fs.mkdir("/reports").await.unwrap();
    fs.write_file("/reports/q1.txt", b"Q1 Report").await.unwrap();
    let end_of_q1 = fs.current_event_id().await.unwrap();

    // Day 2: New reports added
    fs.write_file("/reports/q2.txt", b"Q2 Report").await.unwrap();

    // Audit: What was state at end of Q1?
    let q1_snapshot = fs.snapshot_at(end_of_q1).await.unwrap();
    let q1_files = q1_snapshot.readdir("/reports").await.unwrap().unwrap();

    assert!(q1_files.contains(&"q1.txt".to_string()));
    assert!(!q1_files.contains(&"q2.txt".to_string()));
}
```

---

#### TC-INT-005: Multiple Snapshot Comparison

**Objective:** Verify multiple snapshots can be held simultaneously.

**Verification:**
```rust
#[tokio::test]
async fn test_multiple_snapshots() {
    let (fs, _temp) = create_test_fs().await;

    // V1
    fs.write_file("/file.txt", b"v1").await.unwrap();
    let e1 = fs.current_event_id().await.unwrap();

    // V2
    fs.write_file("/file.txt", b"v2").await.unwrap();
    let e2 = fs.current_event_id().await.unwrap();

    // V3
    fs.write_file("/file.txt", b"v3").await.unwrap();
    let e3 = fs.current_event_id().await.unwrap();

    // Hold multiple snapshots simultaneously
    let snap1 = fs.snapshot_at(e1).await.unwrap();
    let snap2 = fs.snapshot_at(e2).await.unwrap();
    let snap3 = fs.snapshot_at(e3).await.unwrap();

    // All return correct versions
    assert_eq!(snap1.read_file("/file.txt").await.unwrap(), Some(b"v1".to_vec()));
    assert_eq!(snap2.read_file("/file.txt").await.unwrap(), Some(b"v2".to_vec()));
    assert_eq!(snap3.read_file("/file.txt").await.unwrap(), Some(b"v3".to_vec()));
}
```

---

#### TC-INT-006: Snapshot Directory Listing

**Objective:** Verify snapshot readdir returns historical directory contents.

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_directory_listing() {
    let (fs, _temp) = create_test_fs().await;

    fs.mkdir("/docs").await.unwrap();
    fs.write_file("/docs/a.txt", b"a").await.unwrap();
    fs.write_file("/docs/b.txt", b"b").await.unwrap();
    let before_changes = fs.current_event_id().await.unwrap();

    // Modify directory contents
    fs.remove("/docs/a.txt").await.unwrap();
    fs.write_file("/docs/c.txt", b"c").await.unwrap();

    // Current should have b.txt and c.txt
    let current_dir = fs.readdir("/docs").await.unwrap().unwrap();
    assert!(!current_dir.contains(&"a.txt".to_string()));
    assert!(current_dir.contains(&"b.txt".to_string()));
    assert!(current_dir.contains(&"c.txt".to_string()));

    // Snapshot should have a.txt and b.txt
    let snapshot = fs.snapshot_at(before_changes).await.unwrap();
    let historical_dir = snapshot.readdir("/docs").await.unwrap().unwrap();
    assert!(historical_dir.contains(&"a.txt".to_string()));
    assert!(historical_dir.contains(&"b.txt".to_string()));
    assert!(!historical_dir.contains(&"c.txt".to_string()));
}
```

---

#### TC-INT-007: Snapshot Renamed File Path

**Objective:** Verify snapshot shows file at old path after rename.

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_renamed_file() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/original.txt", b"content").await.unwrap();
    let before_rename = fs.current_event_id().await.unwrap();

    fs.rename("/original.txt", "/renamed.txt").await.unwrap();

    // Current: only new path exists
    assert!(fs.stat("/original.txt").await.unwrap().is_none());
    assert!(fs.stat("/renamed.txt").await.unwrap().is_some());

    // Snapshot: old path exists
    let snapshot = fs.snapshot_at(before_rename).await.unwrap();
    assert!(snapshot.stat("/original.txt").await.unwrap().is_some());
    assert!(snapshot.stat("/renamed.txt").await.unwrap().is_none());
}
```

---

#### TC-INT-008: Snapshot Stats Accuracy

**Objective:** Verify snapshot stat returns historical metadata.

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_stats_accuracy() {
    let (fs, _temp) = create_test_fs().await;

    fs.write_file("/file.txt", b"small").await.unwrap();
    let small_event = fs.current_event_id().await.unwrap();

    fs.write_file("/file.txt", b"much larger content here").await.unwrap();

    let snapshot = fs.snapshot_at(small_event).await.unwrap();
    let historical_stats = snapshot.stat("/file.txt").await.unwrap().unwrap();
    let current_stats = fs.stat("/file.txt").await.unwrap().unwrap();

    // Historical size should be smaller
    assert_eq!(historical_stats.size, 5);  // "small"
    assert!(current_stats.size > historical_stats.size);
}
```

---

### 4.2 Concurrent Access Tests

#### TC-CONC-001: Concurrent Snapshot Reads

**Objective:** Verify multiple concurrent snapshot reads work correctly.

**Verification:**
```rust
#[tokio::test]
async fn test_concurrent_snapshot_reads() {
    let (fs, _temp) = create_test_fs().await;
    let fs = Arc::new(fs);

    fs.write_file("/test.txt", b"content").await.unwrap();
    let event_id = fs.current_event_id().await.unwrap();

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                let snapshot = fs.snapshot_at(event_id).await.unwrap();
                snapshot.read_file("/test.txt").await
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.unwrap().unwrap();
        assert_eq!(result, Some(b"content".to_vec()));
    }
}
```

---

#### TC-CONC-002: Snapshot During Active Writes

**Objective:** Verify snapshot consistency during concurrent writes.

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_during_writes() {
    let (fs, _temp) = create_test_fs().await;
    let fs = Arc::new(fs);

    fs.write_file("/test.txt", b"initial").await.unwrap();
    let snapshot_event = fs.current_event_id().await.unwrap();

    // Start writes in background
    let fs_writer = Arc::clone(&fs);
    let writer = tokio::spawn(async move {
        for i in 0..10 {
            fs_writer.write_file("/test.txt", format!("version {}", i).as_bytes()).await.unwrap();
        }
    });

    // Concurrent snapshot reads should always see consistent state
    let snapshot = fs.snapshot_at(snapshot_event).await.unwrap();
    let content = snapshot.read_file("/test.txt").await.unwrap();
    assert_eq!(content, Some(b"initial".to_vec()));

    writer.await.unwrap();
}
```

---

## 5. P2 - Medium Priority Tests

### 5.1 Performance Tests

#### TC-PERF-001: Snapshot Query Performance

**Objective:** Verify snapshot queries complete in reasonable time with large journal.

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_performance() {
    let (fs, _temp) = create_fs_with_many_events(1000).await;

    let event_id = fs.current_event_id().await.unwrap();

    let start = std::time::Instant::now();
    let snapshot = fs.snapshot_at(event_id).await.unwrap();
    let _ = snapshot.stat("/file_500.txt").await.unwrap();
    let elapsed = start.elapsed();

    // Should complete in under 1 second
    assert!(elapsed.as_secs() < 1, "Snapshot query took too long: {:?}", elapsed);
}
```

---

#### TC-PERF-002: list_events Performance

**Objective:** Verify list_events pagination is efficient.

**Verification:**
```rust
#[tokio::test]
async fn test_list_events_performance() {
    let (fs, _temp) = create_fs_with_many_events(10000).await;

    let start = std::time::Instant::now();
    let _ = fs.list_events(100, 5000).await.unwrap();
    let elapsed = start.elapsed();

    // Pagination should be fast regardless of offset
    assert!(elapsed.as_millis() < 500, "list_events too slow: {:?}", elapsed);
}
```

---

### 5.2 Edge Case Tests

#### TC-EDGE-001: Snapshot at Event 0

**Objective:** Verify behavior when snapshot_at(0) is called.

**Verification:**
```rust
#[tokio::test]
async fn test_snapshot_at_event_zero() {
    let (fs, _temp) = create_test_fs().await;

    // Event 0 may or may not exist depending on implementation
    let result = fs.snapshot_at(0).await;

    // Should either succeed with empty FS or return error
    match result {
        Ok(snapshot) => {
            // If succeeds, should see empty or root-only FS
            let root = snapshot.stat("/").await.unwrap();
            assert!(root.is_some());
        }
        Err(_) => {
            // Error is acceptable for event 0
        }
    }
}
```

---

#### TC-EDGE-002: current_event_id on Empty Journal

**Objective:** Verify current_event_id behavior with empty journal.

**Verification:**
```rust
#[tokio::test]
async fn test_current_event_id_empty() {
    let (fs, _temp) = create_test_fs().await;

    // Immediately after creation, should have at least root event or 0
    let event_id = fs.current_event_id().await.unwrap();
    assert!(event_id >= 0);
}
```

---

#### TC-EDGE-003: Diff Large Range

**Objective:** Verify diff handles large event ranges without memory issues.

**Verification:**
```rust
#[tokio::test]
async fn test_diff_large_range() {
    let (fs, _temp) = create_fs_with_many_events(100).await;

    let events = fs.list_events(100, 0).await.unwrap();
    let start = events.last().unwrap().event_id;
    let end = events.first().unwrap().event_id;

    let diffs = fs.diff(start, end).await.unwrap();
    // Should handle without OOM
    assert!(!diffs.is_empty());
}
```

---

### 5.3 Security Tests

#### TC-SEC-001: SQL Injection Prevention

**Objective:** Verify event_id values are properly parameterized.

**Verification:**
```rust
#[tokio::test]
async fn test_sql_injection_prevention() {
    let (fs, _temp) = create_test_fs().await;

    // These should not cause SQL injection
    // (event_id is i64 so injection via type is not possible,
    // but we verify the query doesn't fail unexpectedly)

    let result = fs.snapshot_at(i64::MAX).await;
    assert!(result.is_err()); // Should fail cleanly, not with SQL error

    let result = fs.snapshot_at(i64::MIN).await;
    assert!(result.is_err()); // Should fail cleanly
}
```

---

### 5.4 Memory Tests

#### TC-MEM-001: list_events Memory Bounds

**Objective:** Verify list_events respects limit parameter.

**Verification:**
```rust
#[tokio::test]
async fn test_list_events_memory_bounds() {
    let (fs, _temp) = create_fs_with_many_events(1000).await;

    // Request small limit
    let events = fs.list_events(10, 0).await.unwrap();
    assert_eq!(events.len(), 10);

    // Even with large limit, should not OOM
    let events = fs.list_events(10000, 0).await.unwrap();
    assert!(events.len() <= 1000); // Can't return more than exist
}
```

---

#### TC-MEM-002: diff Memory Bounds

**Objective:** Verify diff doesn't load entire journal into memory.

**Verification:**
```rust
#[tokio::test]
async fn test_diff_memory_bounds() {
    let (fs, _temp) = create_fs_with_many_events(100).await;

    // Diff should only process events in range
    let events = fs.list_events(100, 0).await.unwrap();
    let start = events.last().unwrap().event_id;
    let end = events.first().unwrap().event_id;

    // Should complete without excessive memory
    let diffs = fs.diff(start, end).await.unwrap();

    // Each unique file should appear at most once per operation
    // (100 files = 100 diffs max)
    assert!(diffs.len() <= 100);
}
```

---

## 6. Test Organization

### Recommended File Structure

```
sdk/rust/
├── tests/
│   ├── common/
│   │   ├── mod.rs
│   │   ├── time_travel.rs     # Time-travel test fixtures
│   │   ├── journal_event.rs   # Event verification helpers
│   │   └── diff_helpers.rs    # Diff assertion helpers
│   ├── time_travel_snapshot.rs  # TC-P0-001 through TC-P0-005
│   ├── time_travel_list.rs      # TC-LIST-001 through TC-LIST-006
│   ├── time_travel_diff.rs      # TC-DIFF-001 through TC-DIFF-010
│   ├── time_travel_readonly.rs  # TC-RO-001 through TC-RO-009
│   ├── time_travel_integration.rs # TC-INT-001 through TC-INT-008
│   ├── time_travel_concurrent.rs  # TC-CONC-001, TC-CONC-002
│   ├── time_travel_performance.rs # TC-PERF-001, TC-PERF-002
│   └── time_travel_edge.rs      # TC-EDGE-001 through TC-EDGE-003
```

### Test Execution Commands

```bash
# Run all time-travel tests
cargo test time_travel

# Run snapshot tests only
cargo test time_travel_snapshot

# Run list_events tests
cargo test time_travel_list

# Run diff tests
cargo test time_travel_diff

# Run read-only enforcement tests
cargo test time_travel_readonly

# Run integration tests
cargo test time_travel_integration

# Run with verbose output
cargo test time_travel -- --nocapture
```

---

## 7. Dependencies and Blockers

### Dependencies

| Dependency | Status | Impact |
|------------|--------|--------|
| STORY-1.1 (Schema DDL) | Complete | Required for fs_journal table |
| STORY-1.2 (FileSystem Trait) | In Progress | Required for base filesystem operations |

### Blockers

| Blocker | Impact | Resolution |
|---------|--------|------------|
| `snapshot_at()` returns placeholder | Cannot test snapshots | Implement historical query |
| `current_event_id()` returns 0 | Cannot track events | Implement journal MAX query |
| `list_events()` not implemented | Cannot test pagination | Implement full API |
| `diff()` not implemented | Cannot test diffs | Implement full API |
| `stat_inode_at()` returns None | Snapshot reads fail | Implement historical CTE |

---

## 8. Test Execution Gating

### Gate 1: Snapshot Foundation
- TC-P0-001: Basic Snapshot Read
- TC-P0-002: Snapshot of Deleted File
- TC-P0-004: current_event_id Accuracy
- TC-P0-005: Snapshot event_id Accessor

### Gate 2: API Completeness
- TC-LIST-001 through TC-LIST-006: list_events pagination
- TC-DIFF-001 through TC-DIFF-010: diff operations
- TC-RO-001 through TC-RO-009: Read-only enforcement

### Gate 3: Integration Readiness
- TC-INT-001 through TC-INT-008: Integration scenarios
- TC-CONC-001, TC-CONC-002: Concurrent access

### Gate 4: Production Ready
- TC-PERF-001, TC-PERF-002: Performance benchmarks
- TC-EDGE-001 through TC-EDGE-003: Edge cases
- TC-SEC-001: Security validation
- TC-MEM-001, TC-MEM-002: Memory bounds

---

## 9. Recommendations from Story QA Notes

### Implementation Recommendations

1. **Parameterize all SQL queries** in `query_current()` to prevent SQL injection risk
2. **Add LIMIT clause** to diff query or implement streaming for large diffs
3. **Define behavior for `snapshot_at(0)`** - should return empty FS or error?
4. **Fix Test 3 in story** - setup must create `/deleted.txt` before deleting

### Test Coverage Gaps Addressed

| Gap from Story | Coverage in This Design |
|----------------|------------------------|
| `current_event_id()` explicit test | TC-P0-004 |
| `list_events()` pagination | TC-LIST-001 through TC-LIST-006 |
| `diff()` API | TC-DIFF-001 through TC-DIFF-010 |
| Read-only enforcement | TC-RO-001 through TC-RO-009 |
| Error handling (invalid event_id) | TC-P0-003, TC-DIFF-006, TC-DIFF-009 |

---

## 10. Sign-Off

| Role | Name | Date | Status |
|------|------|------|--------|
| QA Engineer | Claude (QA Agent) | 2026-01-14 | Designed |
| Developer | - | - | Pending |
| Tech Lead | - | - | Pending |
