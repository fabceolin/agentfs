# STORY-6.4: Update Benchmarks to DuckDB-only

> **NOTE**: This story updates the benchmark suite to use DuckDB exclusively.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-6.4 |
| **Epic** | EPIC-SQLITE-REMOVAL |
| **Status** | Approved |
| **Priority** | Low |
| **Dependencies** | STORY-6.2, STORY-6.3 |
| **Blocked By** | STORY-6.2 |

## User Story

**As a** developer maintaining AgentFS
**I want** benchmarks to use DuckDB exclusively
**So that** performance testing reflects the actual production backend

## Story Context

**Gap Identified:** Benchmark suite may still reference SQLite/AgentFS for comparison testing.

**Affected Files:**

| File | Current State | Action |
|------|---------------|--------|
| `sdk/rust/benches/overlayfs.rs` | Benchmarks OverlayFS | Update or remove |
| `sdk/rust/benches/workload.rs` | Workload benchmarks | Update to DuckDB |

## Acceptance Criteria

- [ ] AC1: All benchmarks use DuckAgentFS exclusively
- [ ] AC2: No SQLite/AgentFS references in benchmark code
- [ ] AC3: Benchmarks compile and run successfully
- [ ] AC4: Benchmark results documented for comparison

## Tasks / Subtasks

- [ ] Task 1: Audit Benchmark Files
  - [ ] Check `benches/overlayfs.rs` for AgentFS usage
  - [ ] Check `benches/workload.rs` for AgentFS usage
  - [ ] Identify migration requirements

- [ ] Task 2: Update `overlayfs.rs` Benchmark (AC: 1, 2)
  - [ ] If OverlayFS removed in STORY-6.2: Delete benchmark
  - [ ] If OverlayFS kept: Update to use DuckAgentFS
  - [ ] Remove SQLite imports

- [ ] Task 3: Update `workload.rs` Benchmark (AC: 1, 2)
  - [ ] Replace AgentFS with DuckAgentFS
  - [ ] Update workload patterns for DuckDB
  - [ ] Remove SQLite imports

- [ ] Task 4: Run and Validate Benchmarks (AC: 3, 4)
  - [ ] Run `cargo bench` in sdk/rust
  - [ ] Document benchmark results
  - [ ] Compare with previous results if available

## Dev Notes

### Source Tree Reference

```
sdk/rust/benches/
├── overlayfs.rs    # OverlayFS benchmarks - UPDATE/DELETE
└── workload.rs     # Workload benchmarks - UPDATE
```

### Benchmark Pattern

```rust
// OLD
use agentfs_sdk::AgentFS;
let fs = AgentFS::open(opts).await?;

// NEW
use agentfs_sdk::filesystem::DuckAgentFS;
let config = DuckAgentFSConfig { path: ":memory:".into(), ..Default::default() };
let fs = DuckAgentFS::open(config).await?;
```

### Testing

- Command: `cd sdk/rust && cargo bench`
- Framework: Criterion

## Definition of Done

- [ ] All 4 tasks completed
- [ ] All 4 acceptance criteria verified
- [ ] `cargo bench` runs successfully
- [ ] Zero SQLite references in benches/

---

## Dev Agent Record

### Agent Model Used
(To be filled by dev agent)

### Debug Log References
(To be filled by dev agent)

### Completion Notes List
(To be filled by dev agent)

### File List
(To be filled by dev agent)

### Change Log

| Date | Change | Reason |
|------|--------|--------|
| 2026-01-17 | Story created | Part of EPIC-SQLITE-REMOVAL |

---

## QA Results

(To be filled by QA agent)
