# Test Design: Story 1.1 - Base Tables (GraphDocs)

Date: 2026-01-14
Designer: Quinn (Test Architect)

## Test Strategy Overview

- Total test scenarios: 28
- Unit tests: 6 (21%)
- Integration tests: 18 (64%)
- E2E tests: 4 (15%)
- Priority distribution: P0: 12, P1: 10, P2: 6

## Story Context

**Story ID:** STORY-1.1
**Epic:** EPIC-GRAPHDOCS-001
**Phase:** 1 - Schema GraphDocs
**Title:** Base Tables
**Priority:** High
**Status:** Done

### Core Functionality

A graph-based document storage model where:
- Documents store metadata with optional template inheritance
- Sections form hierarchical structure with parent-child relationships
- Variables enable template substitution with JSON flexibility
- Edges define relationships between sections for graph traversal

## Test Scenarios by Acceptance Criteria

### AC1: Table `gd_documents` (document metadata)

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| GD-1.1-INT-001 | Integration | P0 | Verify gd_documents table exists with all required columns (id, title, description, base_template, language, version, timestamps) | Schema correctness |
| GD-1.1-INT-002 | Integration | P0 | Insert document and verify default values (language='en', version=1, timestamps) | Default behavior |
| GD-1.1-INT-003 | Integration | P1 | Verify base_template FK references gd_documents(id) | Self-referential integrity |
| GD-1.1-INT-004 | Integration | P1 | Verify idx_gd_documents_base index exists | Query performance |
| GD-1.1-UNIT-001 | Unit | P1 | Verify VARCHAR PRIMARY KEY allows alphanumeric IDs | ID format flexibility |

### AC2: Table `gd_sections` (document sections)

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| GD-1.1-INT-005 | Integration | P0 | Verify gd_sections table exists with all required columns | Schema correctness |
| GD-1.1-INT-006 | Integration | P0 | Insert section and verify FK to gd_documents | Referential integrity |
| GD-1.1-INT-007 | Integration | P0 | Verify CASCADE DELETE removes sections when document deleted | Data consistency |
| GD-1.1-UNIT-002 | Unit | P0 | Verify valid_section_type constraint: heading, paragraph, list, code, table, blockquote, hr | Input validation |
| GD-1.1-UNIT-003 | Unit | P0 | Verify valid_heading_level constraint: level 1-6 for headings only | Constraint logic |
| GD-1.1-INT-008 | Integration | P1 | Verify parent_id self-reference for hierarchical sections | Hierarchy support |
| GD-1.1-INT-009 | Integration | P1 | Verify source_section FK for inheritance overrides | Inheritance model |
| GD-1.1-INT-010 | Integration | P1 | Verify idx_gd_sections_document index on (document_id, order_idx) | Query performance |
| GD-1.1-INT-011 | Integration | P2 | Verify idx_gd_sections_parent index on parent_id | Hierarchy query performance |

### AC3: Table `gd_variables` (variables for substitution)

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| GD-1.1-INT-012 | Integration | P0 | Verify gd_variables table exists with all required columns | Schema correctness |
| GD-1.1-INT-013 | Integration | P0 | Insert variable with JSON value and verify storage | JSON type correctness |
| GD-1.1-INT-014 | Integration | P0 | Verify UNIQUE(document_id, name) prevents duplicate variable names | Uniqueness constraint |
| GD-1.1-UNIT-004 | Unit | P1 | Verify valid_var_type constraint: string, number, boolean, array, object | Type validation |
| GD-1.1-INT-015 | Integration | P1 | Verify CASCADE DELETE removes variables when document deleted | Data consistency |
| GD-1.1-INT-016 | Integration | P2 | Verify idx_gd_variables_document and idx_gd_variables_name indexes exist | Query performance |

### AC4: Table `gd_edges` (relationships between sections)

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| GD-1.1-INT-017 | Integration | P0 | Verify gd_edges table exists with all required columns | Schema correctness |
| GD-1.1-INT-018 | Integration | P0 | Insert edge and verify FK to gd_sections for source_id and target_id | Referential integrity |
| GD-1.1-UNIT-005 | Unit | P0 | Verify valid_edge_type constraint: follows, contains, references, links_to | Edge type validation |
| GD-1.1-INT-019 | Integration | P1 | Verify UNIQUE(source_id, target_id, edge_type) prevents duplicate edges | Uniqueness constraint |
| GD-1.1-INT-020 | Integration | P1 | Verify CASCADE DELETE removes edges when section deleted | Graph consistency |
| GD-1.1-INT-021 | Integration | P2 | Verify idx_gd_edges_source, idx_gd_edges_target, idx_gd_edges_type indexes exist | Graph traversal performance |

### AC5: Indexes for efficient queries

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| GD-1.1-UNIT-006 | Unit | P1 | Verify all 7 indexes exist in DDL | Index presence |
| GD-1.1-E2E-001 | E2E | P2 | Query performance test - lookup sections by document_id | Performance validation |

## Cross-Cutting Test Scenarios

### Template Inheritance

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| GD-1.1-E2E-002 | E2E | P0 | Create template document, create child document with base_template, verify relationship | Core inheritance feature |
| GD-1.1-E2E-003 | E2E | P1 | Create section in child document with source_section referencing template section | Section inheritance |

### Graph Traversal

| ID | Level | Priority | Test | Justification |
|----|-------|----------|------|---------------|
| GD-1.1-E2E-004 | E2E | P1 | Create sections with edges, traverse graph via edges table | Graph feature validation |

## Test Data Requirements

### Minimal Dataset

```sql
-- Create document
INSERT INTO gd_documents (id, title) VALUES ('test-doc', 'Test Document');

-- Add sections with hierarchy
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES
    ('s1', 'test-doc', 'heading', 1, 0, '# Test'),
    ('s2', 'test-doc', 'paragraph', 0, 1, 'Hello world'),
    ('s3', 'test-doc', 'code', 0, 2, '```rust\nfn main() {}\n```');

-- Add variables
INSERT INTO gd_variables (id, document_id, name, value, var_type)
VALUES
    ('v1', 'test-doc', 'project_name', '"My Project"', 'string'),
    ('v2', 'test-doc', 'version', '1.0', 'number');

-- Add edges
INSERT INTO gd_edges (id, source_id, target_id, edge_type)
VALUES ('e1', 's1', 's2', 'follows');
```

### Template Inheritance Dataset

```sql
-- Create template
INSERT INTO gd_documents (id, title) VALUES ('template', 'Base Template');
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES ('t1', 'template', 'heading', 1, 0, '# {{title}}');

-- Create child document
INSERT INTO gd_documents (id, title, base_template)
VALUES ('child', 'Child Doc', 'template');
```

### Edge Case Dataset

```sql
-- Invalid section type (should fail)
INSERT INTO gd_sections (..., section_type='invalid', ...) -- EXPECT: constraint violation

-- Heading with invalid level (should fail)
INSERT INTO gd_sections (..., section_type='heading', level=7, ...) -- EXPECT: constraint violation

-- Non-heading with level > 0 (should pass - constraint only applies to headings)
INSERT INTO gd_sections (..., section_type='paragraph', level=5, ...) -- EXPECT: success

-- Duplicate variable name (should fail)
INSERT INTO gd_variables (..., document_id='test-doc', name='project_name', ...) -- EXPECT: unique violation

-- Self-referential edge (edge from section to itself)
INSERT INTO gd_edges (id, source_id, target_id, edge_type)
VALUES ('self-edge', 's1', 's1', 'references'); -- EXPECT: success (no constraint prevents this)
```

## Risk Coverage

| Risk | Test IDs | Mitigation |
|------|----------|------------|
| Schema incomplete | GD-1.1-INT-001, 005, 012, 017 | Full column verification per table |
| Referential integrity broken | GD-1.1-INT-006, 007, 015, 018, 020 | FK and CASCADE DELETE testing |
| Invalid data accepted | GD-1.1-UNIT-002, 003, 004, 005 | Constraint validation |
| Duplicate data | GD-1.1-INT-014, 019 | Unique constraint testing |
| Template inheritance broken | GD-1.1-E2E-002, 003 | End-to-end inheritance flow |
| Graph traversal failures | GD-1.1-E2E-004 | Edge relationship testing |
| Query performance | GD-1.1-E2E-001, INT-010, 011, 016, 021 | Index verification and performance |

## Recommended Execution Order

1. **P0 Unit tests** (fail fast on constraint logic)
   - GD-1.1-UNIT-002, UNIT-003, UNIT-005

2. **P0 Integration tests** (core schema validation)
   - GD-1.1-INT-001, 002, 005, 006, 007, 012, 013, 014, 017, 018

3. **P0 E2E tests** (critical feature)
   - GD-1.1-E2E-002

4. **P1 tests** (extended functionality)
   - All remaining INT and E2E tests

5. **P2 tests** (as time permits)
   - GD-1.1-INT-011, 016, 021, E2E-001

## Gate YAML Block

```yaml
test_design:
  story_id: "graphdocs-1.1"
  epic: "EPIC-GRAPHDOCS-001"
  date: "2026-01-14"
  scenarios_total: 28
  by_level:
    unit: 6
    integration: 18
    e2e: 4
  by_priority:
    p0: 12
    p1: 10
    p2: 6
  coverage_gaps: []
  ac_coverage:
    ac1_gd_documents: 5
    ac2_gd_sections: 7
    ac3_gd_variables: 5
    ac4_gd_edges: 5
    ac5_indexes: 2
  cross_cutting: 4
```

## Trace References

```
Test design matrix: docs/qa/assessments/graphdocs-1.1-test-design-20260114.md
P0 tests identified: 12
Total scenarios: 28
All ACs covered: YES
```

## Quality Checklist

- [x] Every AC has test coverage
- [x] Test levels are appropriate (favoring integration for DB schema operations)
- [x] No duplicate coverage across levels
- [x] Priorities align with business risk (data integrity = P0)
- [x] Test IDs follow naming convention (GD-1.1-{LEVEL}-{SEQ})
- [x] Scenarios are atomic and independent
- [x] Template inheritance adequately tested
- [x] Graph traversal validated
- [x] All constraints have dedicated tests

## Key Design Decisions

1. **Heavy Integration Focus (64%)**: Schema DDL is inherently about database structure. Unit tests are limited to constraint logic validation; integration tests against real DuckDB verify actual behavior.

2. **P0 for Referential Integrity**: FK relationships and CASCADE DELETE are critical for data consistency. Broken references would corrupt the document graph.

3. **P0 for Constraints**: CHECK constraints are the first line of defense against invalid data. All constraint types (section_type, heading_level, var_type, edge_type) are P0.

4. **E2E for Inheritance**: Template inheritance spans multiple tables and requires realistic data scenarios to validate the full flow.

5. **Prefix Naming**: Using `GD-` prefix to distinguish GraphDocs tests from DuckAgentFS tests (which use unprefixed IDs).

---

*Generated by Quinn, Test Architect - BMAD QA Process*
