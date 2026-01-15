# STORY-1.2.4: DentryCache Unit Tests

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.2.4 |
| **Parent** | STORY-1.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 1 - Core Storage Engine |
| **Status** | Ready for Development |
| **Priority** | Medium |
| **Estimated Effort** | Small (0.5 day) |
| **File** | `sdk/rust/src/filesystem/duckagentfs.rs` |
| **Dependencies** | None (can parallelize with STORY-1.2.1) |

## User Story

**As a** developer
**I want** unit tests for the DentryCache
**So that** cache behavior is verified independently of DuckDB

## Technical Description

Add unit tests for the `DentryCache` struct (lines 130-172 of `duckagentfs.rs`). These tests verify cache operations without requiring database connectivity, enabling parallel development.

## Acceptance Criteria

- [ ] `test_cache_get_miss` - Returns None for unknown entries
- [ ] `test_cache_insert_and_get` - Insertion and retrieval work
- [ ] `test_cache_remove` - Removal clears entry
- [ ] `test_cache_clear` - Clear empties all entries
- [ ] `test_cache_lru_eviction` - LRU eviction works at capacity
- [ ] `test_cache_different_parents` - Same name, different parents are distinct

## Tests

Add these tests to the `#[cfg(test)] mod tests` block in `duckagentfs.rs`:

```rust
#[cfg(test)]
mod dentry_cache_tests {
    use super::*;

    #[test]
    fn test_cache_get_miss() {
        let cache = DentryCache::new(100);
        assert_eq!(cache.get(1, "nonexistent"), None);
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = DentryCache::new(100);
        cache.insert(1, "child", 42);
        assert_eq!(cache.get(1, "child"), Some(42));
    }

    #[test]
    fn test_cache_remove() {
        let cache = DentryCache::new(100);
        cache.insert(1, "child", 42);
        cache.remove(1, "child");
        assert_eq!(cache.get(1, "child"), None);
    }

    #[test]
    fn test_cache_clear() {
        let cache = DentryCache::new(100);
        cache.insert(1, "a", 2);
        cache.insert(1, "b", 3);
        cache.clear();
        assert_eq!(cache.get(1, "a"), None);
        assert_eq!(cache.get(1, "b"), None);
    }

    #[test]
    fn test_cache_lru_eviction() {
        let cache = DentryCache::new(2); // Max 2 entries

        cache.insert(1, "a", 10);
        cache.insert(1, "b", 20);

        // Access 'a' to make it recently used
        cache.get(1, "a");

        // Insert 'c' - should evict 'b' (least recently used)
        cache.insert(1, "c", 30);

        assert_eq!(cache.get(1, "a"), Some(10)); // Still there
        assert_eq!(cache.get(1, "b"), None);     // Evicted
        assert_eq!(cache.get(1, "c"), Some(30)); // Newly added
    }

    #[test]
    fn test_cache_different_parents() {
        let cache = DentryCache::new(100);

        // Same name, different parents should be distinct
        cache.insert(1, "file.txt", 10);
        cache.insert(2, "file.txt", 20);

        assert_eq!(cache.get(1, "file.txt"), Some(10));
        assert_eq!(cache.get(2, "file.txt"), Some(20));
    }
}
```

## Test Reference

See: `docs/qa/STORY-1.2-filesystem-trait-test-design.md` Section 4

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/filesystem/duckagentfs.rs` | Add tests to existing test module |

## Implementation Notes

1. **No Dependencies**: These tests use only the `DentryCache` struct with no external dependencies
2. **Can Parallelize**: This story can be implemented in parallel with STORY-1.2.1
3. **Location**: Add tests within the existing `#[cfg(test)] mod tests` block or create a new `mod dentry_cache_tests`

## Created By

Sprint Change Proposal SCP-2026-01-14-STORY-1.2
