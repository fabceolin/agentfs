# Test Design: Story BUG-002 - FUSE Write Handler Not Wired

**Date:** 2026-01-18
**Designer:** Quinn (Test Architect)
**Story:** FUSE Write Handler Not Wired to Handler Registry
**Priority:** High (Core functionality blocked)

---

## Test Strategy Overview

| Metric | Count | Percentage |
|--------|-------|------------|
| **Total test scenarios** | 18 | 100% |
| **Unit tests** | 8 | 44% |
| **Integration tests** | 7 | 39% |
| **E2E tests** | 3 | 17% |
| **P0 (Critical)** | 10 | 56% |
| **P1 (Important)** | 6 | 33% |
| **P2 (Secondary)** | 2 | 11% |

### Test Level Rationale

- **Unit (44%)**: Core wiring logic is testable in isolation with mocks
- **Integration (39%)**: Handler-to-FUSE interactions require real components
- **E2E (17%)**: Full mount-write-verify cycle for critical user journeys

---

## Test Scenarios by Acceptance Criteria

### AC1: FUSE write() calls handler_registry.handle_write()

> FUSE `write()` calls `handler_registry.handle_write()` before/instead of `file.pwrite()`

| ID | Level | Priority | Test Description | Justification |
|----|-------|----------|------------------|---------------|
| BUG002-UNIT-001 | Unit | P0 | `handle_write` invoked when FUSE write called | Verify wiring exists - pure function call |
| BUG002-UNIT-002 | Unit | P0 | Path correctly resolved from inode before `handle_write` | Path resolution is pure logic |
| BUG002-UNIT-003 | Unit | P1 | `handle_write` called with correct offset and data | Parameter passing verification |
| BUG002-INT-001 | Integration | P0 | FUSE write triggers handler registry dispatch | Real FUSE + real registry interaction |

**Given-When-Then for BUG002-UNIT-001:**
```gherkin
Given a mounted FUSE filesystem with handler registry
When FUSE::write() is called with ino=123, offset=0, data="test"
Then handler_registry.handle_write() is called exactly once
And the path is resolved from inode 123
```

---

### AC2: Handler returns Ok(Some(bytes_written)) - use result

> If handler returns `Ok(Some(bytes_written))`, use that result

| ID | Level | Priority | Test Description | Justification |
|----|-------|----------|------------------|---------------|
| BUG002-UNIT-004 | Unit | P0 | Handler returns `Some(len)` → FUSE replies with that length | Return value propagation |
| BUG002-INT-002 | Integration | P0 | ConformanceWriteHandler returns bytes → FUSE reply correct | Real handler behavior |

**Given-When-Then for BUG002-UNIT-004:**
```gherkin
Given a handler that returns Ok(Some(42)) for write
When FUSE::write() is called with 42 bytes of data
Then reply.written(42) is called
And file.pwrite() is NOT called
```

---

### AC3: Handler returns Ok(None) - fall back to file.pwrite()

> If handler returns `Ok(None)`, fall back to `file.pwrite()`

| ID | Level | Priority | Test Description | Justification |
|----|-------|----------|------------------|---------------|
| BUG002-UNIT-005 | Unit | P0 | Handler returns `None` → falls back to `file.pwrite()` | Fallback logic |
| BUG002-INT-003 | Integration | P1 | Non-.md file write goes through default handler | Real filesystem behavior |

**Given-When-Then for BUG002-UNIT-005:**
```gherkin
Given a handler that returns Ok(None) for path "/foo.txt"
When FUSE::write() is called for "/foo.txt"
Then file.pwrite() is called with the data
And reply.written() uses the data length
```

---

### AC4: ConformanceWriteHandler invoked for .md files in template directories

> `ConformanceWriteHandler.write()` is invoked for `.md` files in template directories

| ID | Level | Priority | Test Description | Justification |
|----|-------|----------|------------------|---------------|
| BUG002-INT-004 | Integration | P0 | Write to `/stories/doc.md` (has template) → ConformanceWriteHandler called | Core bug fix validation |
| BUG002-INT-005 | Integration | P1 | Write to `/docs/readme.md` (no template) → DefaultHandler called | Negative case |
| BUG002-UNIT-006 | Unit | P1 | `can_handle()` returns true for .md in template dir | Handler selection logic |

**Given-When-Then for BUG002-INT-004:**
```gherkin
Given a mounted filesystem with TEA conformance enabled
And directory "/stories" contains "story-tmpl.yaml"
When user writes to "/stories/new-doc.md"
Then ConformanceWriteHandler.write() is called
And file "/stories/new-doc.md.source" is created
```

---

### AC5: Background conformance triggered after write

> Background conformance is triggered after write completes

| ID | Level | Priority | Test Description | Justification |
|----|-------|----------|------------------|---------------|
| BUG002-INT-006 | Integration | P0 | Write completes → background task spawned | Async trigger verification |
| BUG002-E2E-001 | E2E | P0 | Write .md file → eventually .conformant file appears | Full user journey |

**Given-When-Then for BUG002-E2E-001:**
```gherkin
Given a mounted filesystem at /tmp/mnt with --tea-conformance
And /tmp/mnt/stories/ contains story-tmpl.yaml
When user writes non-conformant content to /tmp/mnt/stories/test.md
Then within 30 seconds:
  And /tmp/mnt/stories/test.md.source exists
  And /tmp/mnt/stories/test.md.conformant exists (or .conformant.failed)
```

---

### AC6: Existing tests continue to pass

> Existing tests continue to pass

| ID | Level | Priority | Test Description | Justification |
|----|-------|----------|------------------|---------------|
| BUG002-UNIT-007 | Unit | P1 | All 25 existing handler tests pass | Regression prevention |
| BUG002-INT-007 | Integration | P1 | Existing FUSE mount/read tests pass | No regression in read path |

**Given-When-Then for BUG002-UNIT-007:**
```gherkin
Given the existing handler test suite
When `cargo test --lib handler::tests` is run
Then all 25 tests pass
And no new failures introduced
```

---

### AC7: New integration test verifies conformance on FUSE write

> New integration test verifies conformance triggered on FUSE write

| ID | Level | Priority | Test Description | Justification |
|----|-------|----------|------------------|---------------|
| BUG002-E2E-002 | E2E | P0 | Integration test: mount → write → verify .source/.conformant | Explicit AC requirement |
| BUG002-E2E-003 | E2E | P2 | Integration test: verify DB sync (gd_documents, gd_sections) | Database state verification |
| BUG002-UNIT-008 | Unit | P2 | Test framework can mock FUSE write operations | Test infrastructure |

**Given-When-Then for BUG002-E2E-002:**
```gherkin
Given a new integration test in cli/tests/
When the test:
  1. Creates DuckDB with GraphDocs tables
  2. Mounts with TEA conformance enabled
  3. Writes non-conformant markdown file
Then:
  And .source file exists with written content
  And .conformant file exists with transformed content
  And test cleans up mount and database
```

---

## Risk Coverage Matrix

| Risk ID | Description | Test Coverage |
|---------|-------------|---------------|
| FUSE-001 | Write handler not invoked | BUG002-UNIT-001, BUG002-INT-001, BUG002-INT-004 |
| FUSE-002 | Path resolution fails | BUG002-UNIT-002 |
| FUSE-003 | Fallback logic broken | BUG002-UNIT-005, BUG002-INT-003 |
| FUSE-004 | Background task not spawned | BUG002-INT-006, BUG002-E2E-001 |
| FUSE-005 | Regression in existing behavior | BUG002-UNIT-007, BUG002-INT-007 |

---

## Test Implementation Recommendations

### Unit Tests (cli/src/handler.rs or cli/src/fuse.rs)

```rust
#[cfg(test)]
mod fuse_write_handler_tests {
    // BUG002-UNIT-001: Verify handle_write is called
    #[tokio::test]
    async fn test_fuse_write_calls_handler_registry() {
        // Mock handler registry with call tracking
        // Call FUSE write equivalent
        // Assert handler_registry.handle_write() was called
    }

    // BUG002-UNIT-004: Handler Some(len) result used
    #[tokio::test]
    async fn test_handler_some_result_propagated() {
        // Mock handler returning Ok(Some(42))
        // Verify reply.written(42) called
    }

    // BUG002-UNIT-005: Handler None falls back to pwrite
    #[tokio::test]
    async fn test_handler_none_falls_back_to_pwrite() {
        // Mock handler returning Ok(None)
        // Verify file.pwrite() called
    }
}
```

### Integration Tests (cli/tests/fuse_conformance.rs)

```rust
// BUG002-INT-004: ConformanceWriteHandler invoked
#[tokio::test]
async fn test_conformance_handler_invoked_for_md_in_template_dir() {
    // 1. Create test DuckDB with GraphDocs tables
    // 2. Create handler registry with ConformanceWriteHandler
    // 3. Write to path with template in parent
    // 4. Verify .source file created
}

// BUG002-INT-006: Background task spawned
#[tokio::test]
async fn test_background_conformance_spawned() {
    // 1. Setup with mock TEA (or rule-based fallback)
    // 2. Write non-conformant document
    // 3. Wait for background completion
    // 4. Verify .conformant file created
}
```

### E2E Tests (cli/tests/fuse_mount_e2e.rs)

```rust
// BUG002-E2E-001: Full mount-write-verify cycle
#[test]
fn test_fuse_mount_write_conformance_e2e() {
    // 1. agentfs mount test-db /tmp/mnt --tea-conformance -f &
    // 2. mkdir /tmp/mnt/stories && cp template
    // 3. echo "# Test" > /tmp/mnt/stories/test.md
    // 4. Wait and verify .source, .conformant
    // 5. fusermount -u /tmp/mnt
}
```

---

## Recommended Execution Order

1. **P0 Unit tests** - Fast feedback on core wiring logic
   - BUG002-UNIT-001, -002, -004, -005
2. **P0 Integration tests** - Verify component interactions
   - BUG002-INT-001, -002, -004, -006
3. **P0 E2E tests** - Validate full user journey
   - BUG002-E2E-001, -002
4. **P1 tests** - Secondary validation
   - BUG002-UNIT-003, -006, -007
   - BUG002-INT-003, -005, -007
5. **P2 tests** - Nice-to-have coverage
   - BUG002-UNIT-008, BUG002-E2E-003

---

## Quality Gate YAML Block

```yaml
test_design:
  story: BUG-002
  scenarios_total: 18
  by_level:
    unit: 8
    integration: 7
    e2e: 3
  by_priority:
    p0: 10
    p1: 6
    p2: 2
  coverage_gaps: []
  ac_coverage:
    AC1: [BUG002-UNIT-001, BUG002-UNIT-002, BUG002-UNIT-003, BUG002-INT-001]
    AC2: [BUG002-UNIT-004, BUG002-INT-002]
    AC3: [BUG002-UNIT-005, BUG002-INT-003]
    AC4: [BUG002-INT-004, BUG002-INT-005, BUG002-UNIT-006]
    AC5: [BUG002-INT-006, BUG002-E2E-001]
    AC6: [BUG002-UNIT-007, BUG002-INT-007]
    AC7: [BUG002-E2E-002, BUG002-E2E-003, BUG002-UNIT-008]
  risk_mitigations:
    FUSE-001: [BUG002-UNIT-001, BUG002-INT-001, BUG002-INT-004]
    FUSE-002: [BUG002-UNIT-002]
    FUSE-003: [BUG002-UNIT-005, BUG002-INT-003]
    FUSE-004: [BUG002-INT-006, BUG002-E2E-001]
    FUSE-005: [BUG002-UNIT-007, BUG002-INT-007]
```

---

## Quality Checklist

- [x] Every AC has test coverage
- [x] Test levels are appropriate (shift-left where possible)
- [x] No duplicate coverage across levels
- [x] Priorities align with business risk (core bug = P0)
- [x] Test IDs follow naming convention (BUG002-LEVEL-SEQ)
- [x] Scenarios are atomic and independent
- [x] Risk mitigations mapped to tests
- [x] Given-When-Then provided for P0 scenarios

---

## Trace References

```
Test design matrix: docs/qa/assessments/BUG-002-test-design-20260118.md
P0 tests identified: 10
P1 tests identified: 6
P2 tests identified: 2
```
