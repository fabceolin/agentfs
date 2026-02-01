# Test Design: Mass Conformance Testing with Claude YAML Agent

Date: 2026-01-29
Designer: Quinn (Test Architect)

## Test Strategy Overview

- **Total test scenarios**: 24
- **Unit tests**: 6 (25%)
- **Integration tests**: 10 (42%)
- **E2E tests**: 8 (33%)
- **Priority distribution**: P0: 8, P1: 10, P2: 6

## Objective

Design comprehensive real-world tests for the TEA document conformance system using:
- **Backend**: Claude YAML agent (`document-transformer-claude.yaml`)
- **Test Corpus**: 345 stories from `/home/fabricio/src/the_edge_agent/docs/stories`
- **Template**: `/home/fabricio/src/the_edge_agent/.bmad-core/templates/story-tmpl.yaml`
- **Mount Interface**: FUSE filesystem

## Test Corpus Analysis

### Story Variance Characteristics

Based on sampling the 345 stories, the corpus exhibits significant structural diversity:

| Characteristic | Examples | Frequency |
|----------------|----------|-----------|
| **Well-structured** | `TEA-RUST-014-library-api.md` | ~40% |
| **Epic-style** (parent stories) | `TEA-CLI-005-interactive-hitl-mode.md` | ~10% |
| **Minimal/Bug fixes** | `TD.2.add-future-import.md` | ~30% |
| **Non-conformant naming** | `YE.3.langgraph-interrupt-behavior.md` | ~15% |
| **Missing sections** | Various | ~50% |

### Template Schema (story-tmpl.yaml)

Required sections per template:
1. **Status** (choice: Draft, Approved, InProgress, Review, Done)
2. **Story** (template-text: As a... I want... so that...)
3. **Acceptance Criteria** (numbered-list)
4. **Tasks / Subtasks** (bullet-list with checkboxes)
5. **Dev Notes** (with nested Testing section)
6. **Change Log** (table: Date, Version, Description, Author)
7. **Dev Agent Record** (with nested sections)
8. **QA Results**

---

## Test Scenarios by Category

### Category 1: Infrastructure & Setup (Unit Level)

#### Scenarios

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| MASS-UNIT-001 | Unit | P0 | Claude shell provider connectivity | Verify Claude CLI is accessible from TEA |
| MASS-UNIT-002 | Unit | P0 | Template YAML parsing | Ensure story-tmpl.yaml loads correctly |
| MASS-UNIT-003 | Unit | P1 | FUSE mount initialization | Verify agentfs FUSE mount succeeds |
| MASS-UNIT-004 | Unit | P1 | Agent YAML validation | Validate document-transformer-claude.yaml syntax |
| MASS-UNIT-005 | Unit | P2 | Overlay configuration merge | Test claude-transformer.yaml overlay applies |
| MASS-UNIT-006 | Unit | P2 | DuckDB database initialization | Verify demo.duckdb initializes for FUSE |

### Category 2: Single-Document Conformance (Integration Level)

#### Scenarios

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| MASS-INT-001 | Integration | P0 | Well-structured story transformation | Verify conformant documents pass through unchanged |
| MASS-INT-002 | Integration | P0 | Missing sections detection | Detect stories missing QA Results, Dev Agent Record |
| MASS-INT-003 | Integration | P0 | Status normalization | Transform "In Progress" → "InProgress", "**Done**" → "Done" |
| MASS-INT-004 | Integration | P1 | Story format enforcement | Convert freeform descriptions to "As a... I want..." |
| MASS-INT-005 | Integration | P1 | Task checkbox normalization | Ensure `- [ ]` and `- [x]` patterns |
| MASS-INT-006 | Integration | P1 | Change Log table structure | Verify Date/Version/Description/Author columns |
| MASS-INT-007 | Integration | P2 | Extra sections handling | Preserve Story Context, Risk Assessment, etc. |
| MASS-INT-008 | Integration | P2 | Epic vs Story detection | Handle parent stories with Child Stories table |
| MASS-INT-009 | Integration | P1 | Claude response parsing | Extract transformed content from LLM output |
| MASS-INT-010 | Integration | P1 | Error recovery | Handle Claude API timeouts gracefully |

### Category 3: FUSE Mount Pipeline (E2E Level)

#### Scenarios

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| MASS-E2E-001 | E2E | P0 | Single file write-transform-read | Write .md → auto-transform → read conformed |
| MASS-E2E-002 | E2E | P0 | Batch conformance (10 files) | Process 10 stories in sequence via FUSE |
| MASS-E2E-003 | E2E | P0 | Mass conformance (100 files) | Stress test with 100 stories batch |
| MASS-E2E-004 | E2E | P1 | Full corpus conformance (345 files) | Complete test of all stories |
| MASS-E2E-005 | E2E | P1 | Concurrent write handling | Multiple simultaneous writes via FUSE |
| MASS-E2E-006 | E2E | P2 | Checkpoint persistence | Verify DuckDB stores transformed content |
| MASS-E2E-007 | E2E | P1 | Conformance report generation | Run `agentfs graphdocs conformance-report` on results |
| MASS-E2E-008 | E2E | P2 | Rollback on transformation failure | Verify original preserved on Claude error |

---

## Test Implementation Plan

### Phase 1: Environment Setup

```bash
# 1. Build agentfs CLI with FUSE support
cd /home/fabricio/src/agentfs/cli
cargo build --release

# 2. Create test database
./target/release/agentfs duckdb init test-conformance.duckdb

# 3. Verify TEA with Claude shell provider
tea-python run agents/document-transformer-claude.yaml --input '{"test": true}' --dry-run

# 4. Copy template to test location
cp /home/fabricio/src/the_edge_agent/.bmad-core/templates/story-tmpl.yaml \
   /tmp/conformance-test/
```

### Phase 2: FUSE Mount Test Harness

```bash
#!/bin/bash
# test-harness.sh - Mass conformance test runner

TEST_DB="/tmp/conformance-test/mass-test.duckdb"
MOUNT_POINT="/tmp/agentfs-mount"
STORIES_SRC="/home/fabricio/src/the_edge_agent/docs/stories"
TEMPLATE="/home/fabricio/src/the_edge_agent/.bmad-core/templates/story-tmpl.yaml"
AGENTS_DIR="/home/fabricio/src/agentfs/agents"
RESULTS_DIR="/tmp/conformance-results"

# Initialize
mkdir -p "$MOUNT_POINT" "$RESULTS_DIR"

# Start FUSE mount with TEA conformance enabled
./target/release/agentfs mount "$TEST_DB" "$MOUNT_POINT" \
  --tea-conformance \
  --tea-agents-dir "$AGENTS_DIR" \
  --tea-overlay agents/overlay/claude-transformer.yaml \
  --foreground &
MOUNT_PID=$!
sleep 3

# Run batch test
for story in "$STORIES_SRC"/*.md; do
    filename=$(basename "$story")
    echo "Processing: $filename"

    # Copy to FUSE mount (triggers conformance)
    cp "$story" "$MOUNT_POINT/$filename"

    # Read back (get transformed version)
    cat "$MOUNT_POINT/$filename" > "$RESULTS_DIR/$filename"

    # Run conformance report
    ./target/release/agentfs graphdocs "$TEST_DB" conformance-report \
      "$RESULTS_DIR/$filename" --json >> "$RESULTS_DIR/reports.jsonl"
done

# Cleanup
fusermount -u "$MOUNT_POINT"
kill $MOUNT_PID 2>/dev/null
```

### Phase 3: Conformance Validation Script

```python
#!/usr/bin/env python3
"""conformance_validator.py - Validate mass conformance results"""

import json
import sys
from pathlib import Path
from dataclasses import dataclass
from typing import List

@dataclass
class ConformanceResult:
    file_path: str
    is_conformant: bool
    missing_sections: List[str]
    type_violations: List[dict]
    choice_violations: List[dict]
    extra_sections: List[str]

def load_reports(jsonl_path: str) -> List[ConformanceResult]:
    results = []
    with open(jsonl_path) as f:
        for line in f:
            data = json.loads(line)
            results.append(ConformanceResult(
                file_path=data["file_path"],
                is_conformant=data["is_conformant"],
                missing_sections=[s["section_title"] for s in data.get("missing_sections", [])],
                type_violations=data.get("type_violations", []),
                choice_violations=data.get("choice_violations", []),
                extra_sections=data.get("extra_sections", [])
            ))
    return results

def analyze_results(results: List[ConformanceResult]):
    total = len(results)
    conformant = sum(1 for r in results if r.is_conformant)

    print(f"\n{'='*60}")
    print(f"MASS CONFORMANCE TEST RESULTS")
    print(f"{'='*60}")
    print(f"Total files processed: {total}")
    print(f"Fully conformant: {conformant} ({100*conformant/total:.1f}%)")
    print(f"Non-conformant: {total - conformant} ({100*(total-conformant)/total:.1f}%)")

    # Section analysis
    missing_counts = {}
    for r in results:
        for section in r.missing_sections:
            missing_counts[section] = missing_counts.get(section, 0) + 1

    print(f"\nMost commonly missing sections:")
    for section, count in sorted(missing_counts.items(), key=lambda x: -x[1])[:10]:
        print(f"  {section}: {count} ({100*count/total:.1f}%)")

    # Status violations
    status_violations = [r for r in results if any(
        v.get("section_id") == "status" for v in r.choice_violations
    )]
    print(f"\nStatus choice violations: {len(status_violations)}")

    return conformant == total

if __name__ == "__main__":
    reports_file = sys.argv[1] if len(sys.argv) > 1 else "/tmp/conformance-results/reports.jsonl"
    results = load_reports(reports_file)
    success = analyze_results(results)
    sys.exit(0 if success else 1)
```

---

## Success Criteria

### P0 Gate (Must Pass)

| Criterion | Threshold | Measurement |
|-----------|-----------|-------------|
| Claude agent responds | 100% | No connection failures |
| Template parses | 100% | story-tmpl.yaml loads |
| FUSE mount operational | 100% | Mount/unmount succeeds |
| No data loss | 100% | Original content preserved |
| Transformation executes | 95%+ | Claude returns response |

### P1 Gate (Should Pass)

| Criterion | Threshold | Measurement |
|-----------|-----------|-------------|
| Full conformance rate | 70%+ | After transformation |
| Status normalization | 95%+ | Valid enum values |
| Section detection | 90%+ | Missing sections identified |
| Task formatting | 85%+ | Checkbox patterns correct |

### P2 Gate (Nice to Have)

| Criterion | Threshold | Measurement |
|-----------|-----------|-------------|
| Extra sections preserved | 80%+ | Story Context, etc. kept |
| Epic handling | 75%+ | Parent stories valid |
| Performance | <5s/file | Average transformation time |

---

## Risk Coverage

| Risk ID | Risk Description | Mitigating Test |
|---------|------------------|-----------------|
| RISK-001 | Claude API unavailability | MASS-UNIT-001, MASS-INT-010 |
| RISK-002 | FUSE mount failure | MASS-UNIT-003, MASS-E2E-001 |
| RISK-003 | Template parsing errors | MASS-UNIT-002 |
| RISK-004 | Content truncation | MASS-INT-009 |
| RISK-005 | Concurrent write corruption | MASS-E2E-005 |
| RISK-006 | Database corruption | MASS-E2E-006, MASS-E2E-008 |

---

## Recommended Execution Order

1. **P0 Unit tests** (fail fast on infrastructure issues)
2. **P0 Integration tests** (verify single-document flow)
3. **P0 E2E tests** (confirm FUSE pipeline)
4. **P1 Integration tests** (section-specific validation)
5. **P1 E2E tests** (scale testing)
6. **P2 tests** (edge cases and optimization)

---

## Test Execution Commands

### Quick Validation (10 files)

```bash
# Select 10 representative stories
SAMPLE_STORIES=(
  "TEA-RUST-014-library-api.md"        # Well-structured
  "TEA-CLI-005-interactive-hitl-mode.md"  # Epic
  "TD.2.add-future-import.md"          # Minimal
  "TEA-BUG-001-parallel-flow-result-serialization.md"  # Bug fix
  "YE.3.langgraph-interrupt-behavior.md"  # Non-standard naming
  "TEA-BUILTIN-008.2-schema-git-loading.md"  # Feature
  "DOC-002.2-node-specification.md"    # Documentation
  "TEA-RELEASE-005.4-platform-testing-validation.md"  # Release
  "TEA-PROLOG-003-neurosymbolic-examples-docs.md"  # Advanced
  "TEA-GAME-001.1-rust-game-engine-core.md"  # Game
)

for story in "${SAMPLE_STORIES[@]}"; do
  echo "Testing: $story"
  ./test-single.sh "/home/fabricio/src/the_edge_agent/docs/stories/$story"
done
```

### Full Corpus Test

```bash
# Run complete mass conformance test
./test-harness.sh 2>&1 | tee mass-conformance.log

# Analyze results
python3 conformance_validator.py /tmp/conformance-results/reports.jsonl
```

### CI/CD Integration

```yaml
# .github/workflows/conformance-test.yml
name: Mass Conformance Test

on:
  push:
    paths:
      - 'agents/**'
      - 'cli/src/handler.rs'
      - 'sdk/rust/src/graphdocs/**'

jobs:
  conformance:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build CLI
        run: cd cli && cargo build --release
      - name: Run conformance tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: ./test-harness.sh
      - name: Validate results
        run: python3 conformance_validator.py
```

---

## Quality Checklist

- [x] Every AC has test coverage
- [x] Test levels are appropriate (not over-testing)
- [x] No duplicate coverage across levels
- [x] Priorities align with business risk
- [x] Test IDs follow naming convention (MASS-{LEVEL}-{SEQ})
- [x] Scenarios are atomic and independent

---

## Appendix A: Sample Story Categories

### Category: Well-Structured (40%)
Stories with all required sections, proper formatting.
- `TEA-RUST-014-library-api.md`

### Category: Epic/Parent (10%)
Stories that spawn child stories, different structure.
- `TEA-CLI-005-interactive-hitl-mode.md`

### Category: Minimal/Bug Fix (30%)
Simple stories, often missing Dev Agent Record, QA Results.
- `TD.2.add-future-import.md`

### Category: Non-Standard Naming (15%)
Stories not following `{EPIC}-{NUM}` pattern.
- `YE.3.langgraph-interrupt-behavior.md`

### Category: Rich Documentation (5%)
Stories with extensive Dev Notes, multiple appendices.
- `TEA-DOCS-002.3-rag-memory-capability.md`

---

## Appendix B: Claude Agent Configuration

### document-transformer-claude.yaml

Key settings for mass testing:
- `max_tokens: 2048` - May need increase for large stories
- `temperature: 0.2` - Low for consistent output
- `shell_provider: claude` - Uses Claude CLI

### Recommended Overlay Adjustments

```yaml
# claude-mass-test-overlay.yaml
nodes:
  - name: transform_with_llm
    uses: llm.chat
    with:
      max_tokens: 4096  # Increased for large stories
      temperature: 0.1  # Even more consistent
```

---

## Appendix C: Expected Transformation Examples

### Input: Non-conformant Status

```markdown
## Status
**In Progress** ✓
```

### Expected Output: Conformant Status

```markdown
## Status
InProgress
```

---

### Input: Freeform Description

```markdown
## Description
This story adds dark mode to the app.
```

### Expected Output: Story Format

```markdown
## Story
**As a** user,
**I want** dark mode support in the application,
**so that** I can reduce eye strain in low-light conditions.
```

---

## Gate YAML Block

```yaml
test_design:
  scenarios_total: 24
  by_level:
    unit: 6
    integration: 10
    e2e: 8
  by_priority:
    p0: 8
    p1: 10
    p2: 6
  coverage_gaps: []
  target_corpus:
    path: /home/fabricio/src/the_edge_agent/docs/stories
    count: 345
    template: /home/fabricio/src/the_edge_agent/.bmad-core/templates/story-tmpl.yaml
  backend: claude-shell-provider
  mount_type: fuse
```

---

## Trace References

Test design matrix: docs/qa/assessments/MASS-CONFORMANCE-test-design-20260129.md
P0 tests identified: 8
Total corpus size: 345 stories
Backend: Claude YAML Agent via shell provider
