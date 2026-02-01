# Test Design: Story BUG-003

Date: 2026-01-29
Designer: Quinn (Test Architect)
Mode: YOLO - Comprehensive Test Design

## Test Strategy Overview

- Total test scenarios: 24
- Unit tests: 10 (42%)
- Integration tests: 9 (37%)
- E2E tests: 5 (21%)
- Priority distribution: P0: 8, P1: 10, P2: 6

## Risk-Based Test Rationale

This test design maps directly to the identified risks in `BUG-003-risk-20260129.md`:

| Risk ID | Risk Score | Test Coverage |
|---------|------------|---------------|
| TECH-001 | 9 (Critical) | 8 scenarios (UNIT-001-004, INT-001-003, E2E-001) |
| OPS-001 | 6 (High) | 3 scenarios (INT-004, E2E-002, E2E-003) |
| DATA-001 | 6 (High) | 3 scenarios (UNIT-005, INT-005, E2E-004) |
| TECH-002 | 2 (Low) | 3 scenarios (UNIT-006-008) |
| OPS-002 | 2 (Low) | 4 scenarios (UNIT-009-010, INT-006, E2E-005) |

## Test Scenarios by Acceptance Criteria

### AC1: Single Conformance Per File

**Given** a file is copied to the FUSE mount
**When** the copy completes
**Then** exactly ONE conformance process is spawned

#### Scenarios

| ID | Level | Priority | Test | Justification | Mitigates |
|----|-------|----------|------|---------------|-----------|
| BUG-003-UNIT-001 | Unit | P0 | `should_process()` returns true on first call for (inode, mtime) | Core dedup logic validation | TECH-001 |
| BUG-003-UNIT-002 | Unit | P0 | `should_process()` returns false on subsequent calls for same (inode, mtime) | Core dedup logic validation | TECH-001 |
| BUG-003-UNIT-003 | Unit | P1 | `should_process()` returns true for different mtime on same inode | Modified file gets new conformance | TECH-001 |
| BUG-003-INT-001 | Integration | P0 | Small file (<4KB) triggers exactly one conformance spawn | Single write chunk scenario | TECH-001 |
| BUG-003-INT-002 | Integration | P0 | Large file (18KB) triggers exactly one conformance spawn | Multi-chunk write scenario | TECH-001 |
| BUG-003-E2E-001 | E2E | P0 | `cp story.md ./mnt/stories/` produces single "Starting background conformance" log | Real FUSE mount validation | TECH-001 |

### AC2: Large File Support

**Given** a file larger than 10KB (e.g., 18KB story)
**When** copied to the FUSE mount
**Then** conformance completes successfully within timeout

#### Scenarios

| ID | Level | Priority | Test | Justification | Mitigates |
|----|-------|----------|------|---------------|-----------|
| BUG-003-INT-003 | Integration | P0 | 18KB file conformance completes within 60s | Timeout validation | TECH-001 |
| BUG-003-E2E-002 | E2E | P1 | BUG.001.md (18KB test file) conformance succeeds | STORY-8.1 regression test | OPS-001 |

### AC3: No Duplicate API Calls

**Given** TEA conformance uses Claude API
**When** a file triggers conformance
**Then** only one API call is made per file

#### Scenarios

| ID | Level | Priority | Test | Justification | Mitigates |
|----|-------|----------|------|---------------|-----------|
| BUG-003-INT-004 | Integration | P0 | Mock Claude API receives exactly 1 call per file | API call count verification | OPS-001 |
| BUG-003-E2E-003 | E2E | P1 | 10-file batch produces exactly 10 API calls | Batch operation validation | OPS-001 |

### AC4: Graceful Handling of Rapid Writes

**Given** multiple files are copied in quick succession
**When** each file completes
**Then** each file gets its own single conformance process

#### Scenarios

| ID | Level | Priority | Test | Justification | Mitigates |
|----|-------|----------|------|---------------|-----------|
| BUG-003-UNIT-004 | Unit | P1 | HashMap tracks multiple (inode, mtime) pairs correctly | Multi-file dedup isolation | TECH-001 |
| BUG-003-INT-005 | Integration | P1 | 10 concurrent file copies each spawn one conformance | Concurrent write handling | DATA-001 |
| BUG-003-E2E-004 | E2E | P1 | `for i in {1..10}; do cp story.md ./mnt/stories/story-$i.md; done` produces 10 logs | Rapid sequential copy | DATA-001 |

### AC5: Graceful Handling of Partial Writes

**Given** an application crashes before calling flush()
**When** release() is called by the kernel
**Then** conformance still triggers as fallback

#### Scenarios

| ID | Level | Priority | Test | Justification | Mitigates |
|----|-------|----------|------|---------------|-----------|
| BUG-003-UNIT-009 | Unit | P1 | `handle_release()` calls `should_process()` and spawns if not processed | Release fallback logic | OPS-002 |
| BUG-003-UNIT-010 | Unit | P1 | `handle_release()` does NOT spawn if already processed by flush() | No double spawn | OPS-002 |
| BUG-003-INT-006 | Integration | P1 | Simulated no-flush write triggers conformance on release() | Integration of fallback path | OPS-002 |
| BUG-003-E2E-005 | E2E | P2 | vim save (`:wq`) triggers conformance via release() | Real-world application pattern | OPS-002 |

### AC6: No Unbounded Memory Growth

**Given** many files are written over time
**When** deduplication tracking is used
**Then** memory for completed files is cleaned up (no leaks)

#### Scenarios

| ID | Level | Priority | Test | Justification | Mitigates |
|----|-------|----------|------|---------------|-----------|
| BUG-003-UNIT-005 | Unit | P1 | HashMap entry is inserted with Instant::now() timestamp | TTL tracking setup | DATA-001 |
| BUG-003-UNIT-006 | Unit | P1 | `retain()` removes entries older than 60s TTL | TTL cleanup logic | TECH-002 |
| BUG-003-UNIT-007 | Unit | P2 | HashMap size bounded after 100+ should_process() calls with cleanup | Memory growth prevention | TECH-002 |
| BUG-003-UNIT-008 | Unit | P2 | Entries for same inode with new mtime don't accumulate (old removed) | Mtime update cleanup | TECH-002 |
| BUG-003-INT-007 | Integration | P2 | 100 file operations maintain HashMap size < 50 entries | Integration memory verification | TECH-002 |

## Additional Scenarios (Edge Cases)

| ID | Level | Priority | Test | Justification | Mitigates |
|----|-------|----------|------|---------------|-----------|
| BUG-003-INT-008 | Integration | P2 | Non-template file (.txt) does NOT trigger conformance | can_handle() filter verification | - |
| BUG-003-INT-009 | Integration | P2 | File in non-template directory does NOT trigger conformance | Path pattern filter | - |

## Test Data Requirements

### Required Test Files

| File | Size | Purpose |
|------|------|---------|
| `small-story.md` | 2KB | Single chunk write test |
| `medium-story.md` | 10KB | Boundary size test |
| `large-story.md` | 18KB | Multi-chunk write test (5 chunks) |
| `batch/story-{1..50}.md` | 2KB each | Batch operation tests |

### Required Environment

| Component | Configuration |
|-----------|---------------|
| FUSE mount | `agentfs mount test ./mnt --tea-conformance --tea-agents-dir ./agents` |
| TEA agent | `document-transformer-agent.yaml` (or mock) |
| Mock API | Claude API mock for P0 tests (count verification) |
| Test database | In-memory or temp `.agentfs` for isolation |

## Coverage Validation Matrix

| AC | Unit | Integration | E2E | Status |
|----|------|-------------|-----|--------|
| AC1 | 3 | 2 | 1 | COMPLETE |
| AC2 | 0 | 1 | 1 | COMPLETE |
| AC3 | 0 | 1 | 1 | COMPLETE |
| AC4 | 1 | 1 | 1 | COMPLETE |
| AC5 | 2 | 1 | 1 | COMPLETE |
| AC6 | 4 | 1 | 0 | COMPLETE |

**Coverage Summary:**
- All 6 ACs have test coverage
- No duplicate coverage across levels (each test serves distinct purpose)
- Critical paths (AC1, AC3) have multi-level coverage

## Risk Coverage

| Risk ID | Tests Covering | Coverage Status |
|---------|----------------|-----------------|
| TECH-001 | UNIT-001-004, INT-001-003, E2E-001 | COMPLETE (8 tests) |
| OPS-001 | INT-004, E2E-002-003 | COMPLETE (3 tests) |
| DATA-001 | UNIT-005, INT-005, E2E-004 | COMPLETE (3 tests) |
| TECH-002 | UNIT-006-008, INT-007 | COMPLETE (4 tests) |
| OPS-002 | UNIT-009-010, INT-006, E2E-005 | COMPLETE (4 tests) |

## Recommended Execution Order

1. **Phase 1: P0 Unit tests** (fail fast on core logic)
   - UNIT-001, UNIT-002 (dedup logic)

2. **Phase 2: P0 Integration tests** (component interaction)
   - INT-001, INT-002, INT-003, INT-004

3. **Phase 3: P0 E2E test** (real FUSE validation)
   - E2E-001

4. **Phase 4: P1 tests** (core coverage)
   - All P1 scenarios in order

5. **Phase 5: P2 tests** (edge cases, as time permits)
   - All P2 scenarios

## Test Implementation Notes

### Unit Test Setup (Rust)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_should_process_first_call_returns_true() {
        let handler = ConformanceWriteHandler::new(...);
        let result = handler.should_process(1, 1234567890);
        assert!(result);
    }

    #[test]
    fn test_should_process_duplicate_returns_false() {
        let handler = ConformanceWriteHandler::new(...);
        handler.should_process(1, 1234567890);
        let result = handler.should_process(1, 1234567890);
        assert!(!result);
    }

    #[test]
    fn test_ttl_cleanup_removes_old_entries() {
        let handler = ConformanceWriteHandler::new(...);
        // Insert entry with old timestamp
        // Call should_process() to trigger cleanup
        // Verify old entry removed
    }
}
```

### Integration Test Setup

```bash
# Test script: test-conformance/integration-test.sh

# Setup
mkdir -p /tmp/test-mnt
agentfs init test
agentfs mount test /tmp/test-mnt --tea-conformance --tea-agents-dir ./agents

# INT-001: Small file single spawn
cp test-data/small-story.md /tmp/test-mnt/stories/
grep -c "Starting background conformance" /tmp/agentfs.log | assert_eq 1

# INT-002: Large file single spawn
cp test-data/large-story.md /tmp/test-mnt/stories/
grep -c "Starting background conformance" /tmp/agentfs.log | assert_eq 2  # cumulative

# Cleanup
fusermount -u /tmp/test-mnt
```

### E2E Test Setup

```bash
# E2E-001: Real FUSE validation
./cli/test-conformance/test-single.sh

# E2E-003: Batch test
./cli/test-conformance/test-harness.sh --count=10

# E2E-005: vim pattern
vim -c ":wq" /tmp/test-mnt/stories/test.md
grep -c "Starting background conformance" /tmp/agentfs.log | assert_eq 1
```

## Quality Checklist

- [x] Every AC has at least one test
- [x] Test levels are appropriate (not over-testing)
- [x] No duplicate coverage across levels
- [x] Priorities align with business risk (TECH-001 = P0)
- [x] Test IDs follow naming convention (BUG-003-{LEVEL}-{SEQ})
- [x] Scenarios are atomic and independent
- [x] Risk mitigations are mapped to tests
- [x] Test data requirements documented
- [x] Environment setup documented

## Gate YAML Block

```yaml
test_design:
  scenarios_total: 24
  by_level:
    unit: 10
    integration: 9
    e2e: 5
  by_priority:
    p0: 8
    p1: 10
    p2: 6
  coverage_gaps: []
  risk_coverage:
    tech_001: 8
    ops_001: 3
    data_001: 3
    tech_002: 4
    ops_002: 4
```

## Trace References

Test design matrix: docs/qa/assessments/BUG-003-test-design-20260129.md
P0 tests identified: 8
Risk coverage: 100% (all 5 risks have test scenarios)

---

Test design: docs/qa/assessments/BUG-003-test-design-20260129.md
