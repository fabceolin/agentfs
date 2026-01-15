# Test Design: STORY-1.1 - Base Tables (GraphDocs)

**Date:** 2026-01-14
**Designer:** QA Agent (BMAD Process)
**Version:** 4.0 (QA Generated)
**Story Status:** Done

---

## Executive Summary

| Metric | Value |
|--------|-------|
| **Total Test Scenarios** | 52 |
| **Unit Tests** | 12 (23%) |
| **Integration Tests** | 32 (62%) |
| **E2E Tests** | 8 (15%) |
| **Priority Distribution** | P0: 20, P1: 22, P2: 10 |
| **Acceptance Criteria Coverage** | 100% (5/5 ACs) |
| **Risk Coverage** | 100% (4/4 identified risks) |

---

## Story Context

| Field | Value |
|-------|-------|
| **Story ID** | STORY-1.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 1 - Schema GraphDocs |
| **Priority** | High |
| **Target File** | `schema/duckagentfs.sql` |
| **Dependencies** | EPIC-DUCKAGENTFS-001 |

### User Story

> **As a** developer
> **I want** tables to store documents as graphs
> **So that** I have the necessary data structure for GraphDocs

### Acceptance Criteria

- [x] AC1: Table `gd_documents` (document metadata)
- [x] AC2: Table `gd_sections` (document sections)
- [x] AC3: Table `gd_variables` (variables for substitution)
- [x] AC4: Table `gd_edges` (relationships between sections)
- [x] AC5: Indexes for efficient queries

---

## Schema Implementation Analysis

### Actual Implementation (from `schema/duckagentfs.sql` lines 250-334)

#### Table: `gd_documents`

```sql
CREATE TABLE IF NOT EXISTS gd_documents (
    id              VARCHAR PRIMARY KEY,
    inode           UBIGINT UNIQUE,    -- Virtual inode in filesystem
    title           VARCHAR NOT NULL,
    base_template   VARCHAR,           -- Parent template for inheritance
    language        VARCHAR DEFAULT 'en',
    version         UINTEGER DEFAULT 1,
    created_at      TIMESTAMP DEFAULT current_timestamp,
    updated_at      TIMESTAMP DEFAULT current_timestamp,
    metadata        JSON,
    FOREIGN KEY (base_template) REFERENCES gd_documents(id)
);
```

#### Table: `gd_sections`

```sql
CREATE TABLE IF NOT EXISTS gd_sections (
    id              VARCHAR PRIMARY KEY,
    document_id     VARCHAR NOT NULL,
    parent_id       VARCHAR,           -- Parent section for hierarchy
    section_type    VARCHAR NOT NULL,  -- 'heading', 'paragraph', 'list', 'code', 'table'
    level           UINTEGER DEFAULT 1,-- Heading level (1-6) or nesting depth
    order_idx       UINTEGER NOT NULL, -- Order within parent
    content         VARCHAR,           -- Raw content with {{variable}} placeholders
    condition       VARCHAR,           -- Optional: variable name for conditional rendering
    is_inherited    BOOLEAN DEFAULT FALSE,
    source_section  VARCHAR,           -- Original section if inherited
    metadata        JSON,
    FOREIGN KEY (document_id) REFERENCES gd_documents(id),
    FOREIGN KEY (parent_id) REFERENCES gd_sections(id),
    FOREIGN KEY (source_section) REFERENCES gd_sections(id)
);
```

#### Table: `gd_variables`

```sql
CREATE TABLE IF NOT EXISTS gd_variables (
    id              VARCHAR PRIMARY KEY,
    document_id     VARCHAR NOT NULL,
    name            VARCHAR NOT NULL,
    value           JSON,              -- Variable value (any JSON type)
    var_type        VARCHAR DEFAULT 'string',  -- 'string', 'number', 'boolean', 'array', 'object'
    description     VARCHAR,
    is_inherited    BOOLEAN DEFAULT FALSE,
    source_doc      VARCHAR,           -- Document that defined this variable
    created_at      TIMESTAMP DEFAULT current_timestamp,
    updated_at      TIMESTAMP DEFAULT current_timestamp,
    UNIQUE (document_id, name),
    FOREIGN KEY (document_id) REFERENCES gd_documents(id),
    FOREIGN KEY (source_doc) REFERENCES gd_documents(id)
);
```

#### Table: `gd_edges`

```sql
CREATE TABLE IF NOT EXISTS gd_edges (
    source_id       VARCHAR NOT NULL,
    target_id       VARCHAR NOT NULL,
    edge_type       VARCHAR NOT NULL,  -- 'contains', 'references', 'extends', 'next'
    metadata        JSON,
    PRIMARY KEY (source_id, target_id, edge_type)
);
```

### Schema Differences: Story Spec vs Implementation

| Table/Feature | Story Spec | Actual Implementation | Impact |
|---------------|------------|----------------------|--------|
| `gd_documents.inode` | Not specified | UBIGINT UNIQUE | Test uniqueness |
| `gd_documents.description` | TEXT | Not present | Omit |
| `gd_documents.metadata` | Not specified | JSON | Include |
| `gd_sections.content` | TEXT NOT NULL | VARCHAR (nullable) | Test NULL |
| `gd_sections.CHECK constraints` | Yes | Not present | App validation |
| `gd_variables.value` | JSON NOT NULL | JSON (nullable) | Test NULL |
| `gd_variables.CHECK constraint` | Yes | Not present | App validation |
| `gd_variables.source_doc` | Not specified | FK to gd_documents | Test FK |
| `gd_edges.id` | VARCHAR PK | Composite PK | Different |
| `gd_edges.weight` | FLOAT DEFAULT 1.0 | Not present | Omit |

**Critical Finding:** Story spec CHECK constraints are NOT implemented:
- Application MUST validate `section_type` values
- Application MUST validate heading `level` bounds (1-6)
- Application MUST validate `var_type` values
- Application MUST validate `edge_type` values

---

## Test Scenarios by Acceptance Criteria

### AC1: Table `gd_documents` (12 tests)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-INT-001 | Integration | P0 | Verify gd_documents table exists with columns | id, inode, title, base_template, language, version, created_at, updated_at, metadata | Schema correctness |
| GD-1.1-INT-002 | Integration | P0 | Insert document with minimal fields | Defaults applied: language='en', version=1, timestamps set | Default behavior |
| GD-1.1-INT-003 | Integration | P0 | Verify id column is PRIMARY KEY | Duplicate IDs rejected with constraint error | PK constraint |
| GD-1.1-INT-004 | Integration | P0 | Verify title is NOT NULL | NULL title rejected | Required field |
| GD-1.1-INT-005 | Integration | P1 | Verify inode column is UNIQUE | Duplicate inodes rejected | FS integration |
| GD-1.1-INT-006 | Integration | P1 | Verify base_template FK references gd_documents(id) | Invalid base_template rejected | Self-ref integrity |
| GD-1.1-INT-007 | Integration | P1 | Insert document with NULL base_template | Insert succeeds | Optional inheritance |
| GD-1.1-INT-008 | Integration | P1 | Insert document with valid base_template | FK constraint satisfied | Inheritance works |
| GD-1.1-INT-009 | Integration | P2 | Verify VARCHAR PRIMARY KEY accepts alphanumeric IDs | IDs like 'doc-2024-001' accepted | ID format flexibility |
| GD-1.1-INT-010 | Integration | P2 | Insert document with metadata JSON | JSON stored and retrievable | JSON column |
| GD-1.1-UNIT-001 | Unit | P0 | Verify DDL syntax creates all columns | DDL parses without error | Schema validation |
| GD-1.1-UNIT-002 | Unit | P1 | Verify default values in schema | language='en', version=1 | Default specs |

### AC2: Table `gd_sections` (14 tests)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-INT-011 | Integration | P0 | Verify gd_sections table exists with columns | id, document_id, parent_id, section_type, level, order_idx, content, condition, is_inherited, source_section, metadata | Schema correctness |
| GD-1.1-INT-012 | Integration | P0 | Insert section with valid document_id | Insert succeeds | FK constraint |
| GD-1.1-INT-013 | Integration | P0 | Insert section with invalid document_id | FK violation error | Referential integrity |
| GD-1.1-INT-014 | Integration | P0 | Verify section_type is NOT NULL | NULL section_type rejected | Required field |
| GD-1.1-INT-015 | Integration | P0 | Verify order_idx is NOT NULL | NULL order_idx rejected | Required field |
| GD-1.1-INT-016 | Integration | P1 | Insert sections with valid section_type values | heading, paragraph, list, code, table accepted | Type storage |
| GD-1.1-INT-017 | Integration | P1 | Insert section with arbitrary section_type | Succeeds (no CHECK) | App validation awareness |
| GD-1.1-INT-018 | Integration | P1 | Verify parent_id self-reference FK | Section can reference another section as parent | Hierarchy support |
| GD-1.1-INT-019 | Integration | P1 | Verify source_section FK for inheritance | FK constraint enforced | Inheritance model |
| GD-1.1-INT-020 | Integration | P1 | Insert section with NULL content | Insert succeeds | Content nullable |
| GD-1.1-INT-021 | Integration | P2 | Insert heading with level values 0, 1, 6, 7 | All succeed (no CHECK) | Level validation awareness |
| GD-1.1-INT-022 | Integration | P2 | Insert section with condition value | Conditional rendering data stored | Feature support |
| GD-1.1-UNIT-003 | Unit | P0 | Verify section_type documented values | heading, paragraph, list, code, table, blockquote, hr | Input validation |
| GD-1.1-UNIT-004 | Unit | P0 | Verify heading level range 1-6 | Application must validate | Constraint logic |

### AC3: Table `gd_variables` (12 tests)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-INT-023 | Integration | P0 | Verify gd_variables table exists with columns | id, document_id, name, value, var_type, description, is_inherited, source_doc, created_at, updated_at | Schema correctness |
| GD-1.1-INT-024 | Integration | P0 | Insert variable with JSON value | JSON stored and retrieved correctly | JSON type support |
| GD-1.1-INT-025 | Integration | P0 | Verify UNIQUE(document_id, name) constraint | Duplicate name in same document rejected | Uniqueness |
| GD-1.1-INT-026 | Integration | P0 | Insert variable with same name in different documents | Both inserts succeed | Doc-scoped uniqueness |
| GD-1.1-INT-027 | Integration | P1 | Verify source_doc FK to gd_documents | Invalid source_doc rejected | Inheritance tracking |
| GD-1.1-INT-028 | Integration | P1 | Insert variable with complex nested JSON | JSON stored correctly | Complex JSON handling |
| GD-1.1-INT-029 | Integration | P1 | Verify name is NOT NULL | NULL name rejected | Required field |
| GD-1.1-INT-030 | Integration | P2 | Insert variable with var_type='number' but string JSON | Succeeds (no CHECK) | Type validation awareness |
| GD-1.1-INT-031 | Integration | P2 | Insert variable with NULL value | Insert succeeds | Value nullable |
| GD-1.1-INT-032 | Integration | P2 | Insert variable with description | Description stored | Optional field |
| GD-1.1-UNIT-005 | Unit | P1 | Verify var_type documented values | string, number, boolean, array, object | Type validation |
| GD-1.1-UNIT-006 | Unit | P1 | Verify default var_type is 'string' | Default applied | Default behavior |

### AC4: Table `gd_edges` (8 tests)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-INT-033 | Integration | P0 | Verify gd_edges table exists with columns | source_id, target_id, edge_type, metadata with composite PK | Schema correctness |
| GD-1.1-INT-034 | Integration | P0 | Insert edge between two sections | Edge stored correctly | Basic functionality |
| GD-1.1-INT-035 | Integration | P0 | Verify composite PK (source_id, target_id, edge_type) | Duplicate edge rejected | Uniqueness |
| GD-1.1-INT-036 | Integration | P0 | Insert same source/target with different edge_type | Both succeed | Composite PK behavior |
| GD-1.1-INT-037 | Integration | P1 | Insert edges with valid edge_type values | contains, references, extends, next accepted | Edge type storage |
| GD-1.1-INT-038 | Integration | P1 | Insert edge with metadata JSON | JSON stored correctly | Optional metadata |
| GD-1.1-INT-039 | Integration | P2 | Insert edge with arbitrary edge_type | Succeeds (no CHECK) | Type validation awareness |
| GD-1.1-UNIT-007 | Unit | P0 | Verify edge_type documented values | follows, contains, references, links_to | Edge type validation |

### AC5: Indexes for efficient queries (6 tests)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-UNIT-008 | Unit | P0 | Verify idx_gd_sections_document exists | Index on gd_sections(document_id) | Query performance |
| GD-1.1-UNIT-009 | Unit | P0 | Verify idx_gd_sections_parent exists | Index on gd_sections(parent_id) | Hierarchy performance |
| GD-1.1-UNIT-010 | Unit | P1 | Verify idx_gd_variables_document exists | Index on gd_variables(document_id) | Query performance |
| GD-1.1-UNIT-011 | Unit | P1 | Verify idx_gd_variables_name composite index | Index on gd_variables(document_id, name) | Lookup performance |
| GD-1.1-E2E-001 | E2E | P2 | Query sections by document_id with 10K sections | Query completes < 50ms | Performance validation |
| GD-1.1-E2E-002 | E2E | P2 | Query variables by (document_id, name) | Query completes < 10ms | Index effectiveness |

---

## Cross-Cutting Test Scenarios

### Template Inheritance (4 tests)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-E2E-003 | E2E | P0 | Create template document, create child with base_template | Inheritance chain established | Core feature |
| GD-1.1-E2E-004 | E2E | P1 | Create section in child with source_section referencing template | Section inheritance tracked | Section inheritance |
| GD-1.1-E2E-005 | E2E | P1 | Create variable in child with source_doc referencing template | Variable inheritance tracked | Variable inheritance |
| GD-1.1-E2E-006 | E2E | P1 | Three-level inheritance chain | All relationships maintained | Deep inheritance |

### Graph Traversal (2 tests)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-E2E-007 | E2E | P1 | Create sections with edges, traverse via edges table | Graph navigation works | Graph feature |
| GD-1.1-E2E-008 | E2E | P2 | Complex graph with multiple edge types | All edges queryable | Multi-type edges |

---

## Risk-Driven Test Scenarios

### Risk 1: Circular Reference Prevention (HIGH RISK)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-CIRC-001 | Integration | P0 | Attempt circular base_template (A→B→A) | Succeeds at DB level | Cycle detection not in DB |
| GD-1.1-CIRC-002 | Integration | P0 | Attempt circular parent_id (s1→s2→s1) | Succeeds at DB level | Hierarchy cycle not prevented |
| GD-1.1-CIRC-003 | Integration | P1 | Deep section hierarchy (15+ levels) | No performance degradation | Deep hierarchy support |

**Test Implementation:**

```sql
-- GD-1.1-CIRC-001: Circular base_template
INSERT INTO gd_documents (id, title) VALUES ('doc-a', 'Doc A');
INSERT INTO gd_documents (id, title, base_template) VALUES ('doc-b', 'Doc B', 'doc-a');
UPDATE gd_documents SET base_template = 'doc-b' WHERE id = 'doc-a';
-- Expected: UPDATE succeeds - APPLICATION MUST detect cycles

-- GD-1.1-CIRC-002: Circular parent_id
INSERT INTO gd_documents (id, title) VALUES ('circ-doc', 'Test');
INSERT INTO gd_sections (id, document_id, section_type, order_idx) VALUES ('s1', 'circ-doc', 'paragraph', 0);
INSERT INTO gd_sections (id, document_id, parent_id, section_type, order_idx) VALUES ('s2', 'circ-doc', 's1', 'paragraph', 1);
UPDATE gd_sections SET parent_id = 's2' WHERE id = 's1';
-- Expected: UPDATE succeeds - APPLICATION MUST detect cycles
```

### Risk 2: Orphaned Data Handling (MEDIUM RISK)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-ORPH-001 | Integration | P0 | Delete document with sections | FK violation OR orphaned sections | Cascade behavior |
| GD-1.1-ORPH-002 | Integration | P1 | Delete parent section | Child section's parent_id invalid | Hierarchy integrity |
| GD-1.1-ORPH-003 | Integration | P1 | Delete document with variables | FK violation OR orphaned variables | Cleanup |

**Test Implementation:**

```sql
-- GD-1.1-ORPH-001: Check CASCADE behavior
INSERT INTO gd_documents (id, title) VALUES ('orphan-test', 'Test');
INSERT INTO gd_sections (id, document_id, section_type, order_idx) VALUES ('os1', 'orphan-test', 'paragraph', 0);
DELETE FROM gd_documents WHERE id = 'orphan-test';
-- Check: Does gd_sections still contain 'os1'?
SELECT COUNT(*) FROM gd_sections WHERE id = 'os1';
-- Expected: 0 if CASCADE, FK error if NO CASCADE
```

### Risk 3: Cross-Document Edge Integrity (LOW RISK)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-EDGE-001 | Integration | P1 | Create edge between sections in different documents | Succeeds (no FK validation) | Cross-doc edge awareness |
| GD-1.1-EDGE-002 | Integration | P2 | Create self-referential edge (source = target) | Succeeds (no constraint) | Self-edge handling |

**Test Implementation:**

```sql
-- GD-1.1-EDGE-001: Cross-document edge
INSERT INTO gd_documents (id, title) VALUES ('doc1', 'Doc 1'), ('doc2', 'Doc 2');
INSERT INTO gd_sections (id, document_id, section_type, order_idx) VALUES ('d1s1', 'doc1', 'paragraph', 0);
INSERT INTO gd_sections (id, document_id, section_type, order_idx) VALUES ('d2s1', 'doc2', 'paragraph', 0);
INSERT INTO gd_edges (source_id, target_id, edge_type) VALUES ('d1s1', 'd2s1', 'references');
-- Expected: Succeeds - no FK constraint prevents this
```

### Risk 4: JSON Type Mismatch (LOW RISK)

| ID | Level | Priority | Test Scenario | Expected Result | Rationale |
|----|-------|----------|---------------|-----------------|-----------|
| GD-1.1-JSON-001 | Integration | P2 | var_type='number' with value='"text"' | Succeeds (no DB validation) | Type mismatch awareness |
| GD-1.1-JSON-002 | Integration | P2 | Deeply nested JSON (10+ levels) | Stored and retrieved correctly | Complex JSON handling |

---

## Test Data Requirements

### Minimal Dataset (Unit/Integration)

```sql
-- Base document
INSERT INTO gd_documents (id, title) VALUES ('test-doc', 'Test Document');

-- Sections with hierarchy
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES
    ('s1', 'test-doc', 'heading', 1, 0, '# Test'),
    ('s2', 'test-doc', 'paragraph', 0, 1, 'Hello world'),
    ('s3', 'test-doc', 'code', 0, 2, '```rust\nfn main() {}\n```');

-- Variables
INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES
    ('v1', 'test-doc', 'project_name', '"My Project"', 'string'),
    ('v2', 'test-doc', 'version', '1.0', 'number');

-- Edges
INSERT INTO gd_edges (source_id, target_id, edge_type) VALUES ('s1', 's2', 'next');
```

### Template Inheritance Dataset

```sql
-- Template
INSERT INTO gd_documents (id, title) VALUES ('template', 'Base Template');
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES
    ('t1', 'template', 'heading', 1, 0, '# {{title}}');
INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES
    ('tv1', 'template', 'author', '"Default Author"', 'string');

-- Child document
INSERT INTO gd_documents (id, title, base_template) VALUES ('child', 'Child Doc', 'template');
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content, is_inherited, source_section) VALUES
    ('c1', 'child', 'heading', 1, 0, '# My Custom Title', TRUE, 't1');
INSERT INTO gd_variables (id, document_id, name, value, var_type, is_inherited, source_doc) VALUES
    ('cv1', 'child', 'author', '"Custom Author"', 'string', TRUE, 'template');
```

### Edge Case Dataset

```sql
-- All section types (documented)
INSERT INTO gd_sections (id, document_id, section_type, order_idx) VALUES
    ('ec-heading', 'test-doc', 'heading', 10),
    ('ec-para', 'test-doc', 'paragraph', 11),
    ('ec-list', 'test-doc', 'list', 12),
    ('ec-code', 'test-doc', 'code', 13),
    ('ec-table', 'test-doc', 'table', 14),
    ('ec-blockquote', 'test-doc', 'blockquote', 15),
    ('ec-hr', 'test-doc', 'hr', 16);

-- All variable types
INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES
    ('vt-str', 'test-doc', 'v_string', '"hello"', 'string'),
    ('vt-num', 'test-doc', 'v_number', '42', 'number'),
    ('vt-bool', 'test-doc', 'v_boolean', 'true', 'boolean'),
    ('vt-arr', 'test-doc', 'v_array', '["a","b","c"]', 'array'),
    ('vt-obj', 'test-doc', 'v_object', '{"key":"value"}', 'object');

-- All edge types (from story spec)
INSERT INTO gd_edges (source_id, target_id, edge_type) VALUES
    ('s1', 's2', 'follows'),
    ('s1', 's3', 'contains'),
    ('s2', 's3', 'references'),
    ('s3', 's1', 'links_to');
```

---

## Risk Coverage Matrix

| Risk | Probability | Impact | Test IDs | Status |
|------|-------------|--------|----------|--------|
| Circular references | Medium | High | CIRC-001, CIRC-002, CIRC-003 | Covered |
| Orphaned data | Medium | Medium | ORPH-001, ORPH-002, ORPH-003 | Covered |
| Cross-document edges | Low | Low | EDGE-001, EDGE-002 | Covered |
| JSON type mismatch | Low | Low | JSON-001, JSON-002 | Covered |
| Missing CHECK constraints | Medium | Medium | INT-017, INT-021, INT-030, INT-039 | Documented |

---

## Test Execution Order

### Phase 1: Schema Validation (P0) - 18 tests
1. Table existence: INT-001, INT-011, INT-023, INT-033
2. Primary keys: INT-003, INT-035
3. Foreign keys: INT-006, INT-013
4. NOT NULL constraints: INT-004, INT-014, INT-015, INT-029
5. Defaults: INT-002
6. Basic operations: INT-012, INT-016, INT-024, INT-034
7. Uniqueness: INT-025, INT-026, INT-036

### Phase 2: Risk-Critical (P0) - 4 tests
8. Circular references: CIRC-001, CIRC-002
9. Orphan handling: ORPH-001
10. Template inheritance: E2E-003

### Phase 3: Unit Tests (P0-P1) - 11 tests
11. DDL validation: UNIT-001 through UNIT-011

### Phase 4: Extended Coverage (P1) - 14 tests
12. Remaining INT tests
13. E2E tests: E2E-004 through E2E-008
14. Risk tests: ORPH-002, ORPH-003, EDGE-001

### Phase 5: Edge Cases (P2) - 5 tests
15. Performance tests
16. Boundary tests
17. JSON edge cases

---

## Quality Gate YAML

```yaml
test_design:
  story_id: "STORY-1.1"
  epic: "EPIC-GRAPHDOCS-001"
  version: "4.0"
  date: "2026-01-14"
  status: "APPROVED"

  metrics:
    scenarios_total: 52
    by_level:
      unit: 12
      integration: 32
      e2e: 8
    by_priority:
      p0: 20
      p1: 22
      p2: 10

  coverage:
    acceptance_criteria:
      ac1_gd_documents: 12
      ac2_gd_sections: 14
      ac3_gd_variables: 12
      ac4_gd_edges: 8
      ac5_indexes: 6
    risk_scenarios:
      circular_references: 3
      orphaned_data: 3
      cross_document_edges: 2
      json_type_mismatch: 2
    cross_cutting:
      template_inheritance: 4
      graph_traversal: 2

  gaps:
    - "CHECK constraints not in schema - application validation required"
    - "CASCADE DELETE behavior needs runtime verification"
    - "No FK from gd_edges to gd_sections - cross-document edges possible"
    - "Circular reference prevention is application responsibility"

  schema_alignment: "Verified against schema/duckagentfs.sql lines 250-334"
```

---

## Trace References

```
Source Story:    docs/stories/graphdocs/STORY-1.1-base-tables.md
Schema File:     schema/duckagentfs.sql (lines 250-334)
Previous Design: docs/qa/assessments/graphdocs-STORY-1.1-test-design-20260114-final.md
Test Design:     docs/qa/assessments/graphdocs-STORY-1.1-test-design-20260114-qa.md

P0 Tests: 20
P1 Tests: 22
P2 Tests: 10
Total:    52

All ACs Covered:        YES
Schema Aligned:         YES
Risks Addressed:        YES (4/4)
```

---

## Quality Checklist

- [x] Every AC has test coverage
- [x] Test levels appropriate (integration-heavy for DB schema)
- [x] No duplicate coverage across levels
- [x] Priorities align with business risk
- [x] Test IDs follow naming convention (GD-1.1-{TYPE}-{SEQ})
- [x] Scenarios are atomic and independent
- [x] Template inheritance tested
- [x] Graph traversal validated
- [x] Schema differences from story spec documented
- [x] Circular reference risk addressed
- [x] Orphaned data risk addressed
- [x] Cross-document edge risk addressed
- [x] Missing CHECK constraints documented

---

## Implementation Notes

### Test Framework (Python + DuckDB)

```python
# pytest with DuckDB
import duckdb
import pytest

@pytest.fixture
def db():
    conn = duckdb.connect(':memory:')
    # Load schema
    with open('schema/duckagentfs.sql') as f:
        conn.execute(f.read())
    yield conn
    conn.close()

@pytest.mark.p0
def test_gd_documents_exists(db):
    """GD-1.1-INT-001: Verify gd_documents table exists"""
    result = db.execute("SELECT * FROM gd_documents LIMIT 0").description
    columns = [col[0] for col in result]
    assert 'id' in columns
    assert 'title' in columns
    assert 'base_template' in columns
    assert 'inode' in columns
    assert 'metadata' in columns

@pytest.mark.p0
def test_gd_documents_pk_constraint(db):
    """GD-1.1-INT-003: Verify id column is PRIMARY KEY"""
    db.execute("INSERT INTO gd_documents (id, title) VALUES ('test-id', 'Test')")
    with pytest.raises(Exception):
        db.execute("INSERT INTO gd_documents (id, title) VALUES ('test-id', 'Duplicate')")

@pytest.mark.p0
def test_gd_documents_title_not_null(db):
    """GD-1.1-INT-004: Verify title is NOT NULL"""
    with pytest.raises(Exception):
        db.execute("INSERT INTO gd_documents (id, title) VALUES ('test', NULL)")
```

### Cleanup Pattern

```sql
-- Test teardown
DELETE FROM gd_edges WHERE source_id LIKE 'test-%';
DELETE FROM gd_variables WHERE document_id LIKE 'test-%';
DELETE FROM gd_sections WHERE document_id LIKE 'test-%';
DELETE FROM gd_documents WHERE id LIKE 'test-%';
```

---

## Known Gaps for Future Stories

1. **Application-Level Validation**: Stories for implementing type validation in application code
2. **Cycle Detection**: Story for implementing circular reference detection
3. **Cascade Behavior**: Document actual CASCADE behavior or implement DELETE triggers
4. **Edge FK Constraints**: Consider adding optional FK from edges to sections

---

*Generated by QA Agent - BMAD Test Design Process v4.0*
*Comprehensive test design aligned with actual schema implementation*
