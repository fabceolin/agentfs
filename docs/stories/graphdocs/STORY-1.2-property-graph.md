# STORY-1.2: Property Graph Definition

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.2 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 1 - Schema GraphDocs |
| **Status** | Done |
| **Priority** | High |
| **File** | `schema/duckagentfs.sql` |
| **Dependencies** | STORY-1.1 |

## User Story

**As a** developer
**I want** a property graph definition for GraphDocs
**So that** I can use DuckPGQ for graph queries

## Acceptance Criteria

- [x] CREATE PROPERTY GRAPH graphdocs (commented in schema)
- [x] Vertices: documents, sections, variables
- [x] Edges: has_section, has_variable, edge

## Technical Specification

### Property Graph Definition

DuckPGQ uses SQL/PGQ syntax to define property graphs over existing tables.

```sql
-- Property Graph Definition (requires DuckPGQ extension)
-- LOAD 'duckpgq';

CREATE PROPERTY GRAPH graphdocs
VERTEX TABLES (
    gd_documents PROPERTIES (id, title, description, language, version) LABEL document,
    gd_sections PROPERTIES (id, section_type, level, order_idx, content) LABEL section,
    gd_variables PROPERTIES (id, name, value, var_type) LABEL variable
)
EDGE TABLES (
    -- Document -> Section edges (implicit from foreign key)
    gd_sections AS has_section
        SOURCE KEY (document_id) REFERENCES gd_documents (id)
        DESTINATION KEY (id) REFERENCES gd_sections (id)
        PROPERTIES (order_idx, is_inherited)
        LABEL has_section,

    -- Document -> Variable edges (implicit from foreign key)
    gd_variables AS has_variable
        SOURCE KEY (document_id) REFERENCES gd_documents (id)
        DESTINATION KEY (id) REFERENCES gd_variables (id)
        PROPERTIES (is_inherited)
        LABEL has_variable,

    -- Section -> Section edges (from gd_edges)
    gd_edges AS section_edge
        SOURCE KEY (source_id) REFERENCES gd_sections (id)
        DESTINATION KEY (target_id) REFERENCES gd_sections (id)
        PROPERTIES (edge_type, weight, metadata)
        LABEL edge,

    -- Document -> Document edges (template inheritance)
    gd_documents AS inherits_from
        SOURCE KEY (id) REFERENCES gd_documents (id)
        DESTINATION KEY (base_template) REFERENCES gd_documents (id)
        LABEL inherits
);
```

### Graph Structure

```
                    +---------------+
                    |   document    |
                    +-------+-------+
                            |
          +-----------------+------------------+
          |                 |                  |
          v                 v                  v
    +----------+      +----------+      +----------+
    | section  |      | section  |      | variable |
    +----+-----+      +----+-----+      +----------+
         |                 |
         +---- edge -------+
```

### Example Queries

#### Query 1: Get all sections of a document
```sql
-- Using SQL/PGQ MATCH syntax
FROM GRAPH_TABLE (graphdocs
    MATCH (d:document WHERE d.id = 'my-doc')-[e:has_section]->(s:section)
    COLUMNS (s.id, s.section_type, s.content, e.order_idx)
)
ORDER BY order_idx;
```

#### Query 2: Get document inheritance chain
```sql
-- Find all ancestors of a document
FROM GRAPH_TABLE (graphdocs
    MATCH (d:document WHERE d.id = 'child-doc')-[:inherits*1..5]->(ancestor:document)
    COLUMNS (ancestor.id AS ancestor_id, ancestor.title)
);
```

#### Query 3: Find connected sections
```sql
-- Find sections that follow a given section
FROM GRAPH_TABLE (graphdocs
    MATCH (s1:section WHERE s1.id = 'section-1')-[e:edge WHERE e.edge_type = 'follows']->(s2:section)
    COLUMNS (s2.id, s2.section_type, s2.content)
);
```

#### Query 4: Get document with all related nodes
```sql
-- Get complete document graph
FROM GRAPH_TABLE (graphdocs
    MATCH (d:document WHERE d.id = 'my-doc')
          -[hs:has_section]->(s:section),
          (d)-[hv:has_variable]->(v:variable)
    COLUMNS (
        d.title AS doc_title,
        s.id AS section_id,
        s.content,
        v.name AS var_name,
        v.value AS var_value
    )
);
```

### Fallback Queries (Without DuckPGQ)

If DuckPGQ is not available, equivalent queries using standard SQL:

```sql
-- Get all sections (standard SQL)
SELECT s.id, s.section_type, s.content, s.order_idx
FROM gd_sections s
WHERE s.document_id = 'my-doc'
ORDER BY s.order_idx;

-- Get inheritance chain (recursive CTE)
WITH RECURSIVE ancestors AS (
    SELECT id, title, base_template, 0 AS depth
    FROM gd_documents
    WHERE id = 'child-doc'

    UNION ALL

    SELECT d.id, d.title, d.base_template, a.depth + 1
    FROM gd_documents d
    JOIN ancestors a ON d.id = a.base_template
    WHERE a.depth < 10 -- Prevent infinite recursion
)
SELECT id, title, depth
FROM ancestors
WHERE depth > 0
ORDER BY depth;

-- Get connected sections
SELECT s2.id, s2.section_type, s2.content
FROM gd_sections s1
JOIN gd_edges e ON e.source_id = s1.id
JOIN gd_sections s2 ON e.target_id = s2.id
WHERE s1.id = 'section-1' AND e.edge_type = 'follows';
```

## Tests

### Test 1: Graph Query Returns Sections
```sql
-- Setup
INSERT INTO gd_documents (id, title) VALUES ('test', 'Test');
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES ('s1', 'test', 'heading', 1, 0, '# Test');

-- Query (standard SQL fallback)
SELECT s.* FROM gd_sections s WHERE s.document_id = 'test';
-- Expected: 1 row
```

### Test 2: Inheritance Chain
```sql
-- Setup chain: child -> parent -> grandparent
INSERT INTO gd_documents (id, title) VALUES ('gp', 'Grandparent');
INSERT INTO gd_documents (id, title, base_template) VALUES ('p', 'Parent', 'gp');
INSERT INTO gd_documents (id, title, base_template) VALUES ('c', 'Child', 'p');

-- Query chain
WITH RECURSIVE chain AS (
    SELECT id, base_template, 0 AS depth FROM gd_documents WHERE id = 'c'
    UNION ALL
    SELECT d.id, d.base_template, c.depth + 1
    FROM gd_documents d JOIN chain c ON d.id = c.base_template
)
SELECT * FROM chain;
-- Expected: 3 rows (c, p, gp)
```

### Test 3: Edge Traversal
```sql
-- Setup
INSERT INTO gd_documents (id, title) VALUES ('doc', 'Doc');
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES
    ('a', 'doc', 'heading', 1, 0, '# A'),
    ('b', 'doc', 'paragraph', 0, 1, 'B');
INSERT INTO gd_edges (id, source_id, target_id, edge_type)
VALUES ('e1', 'a', 'b', 'follows');

-- Traverse
SELECT t.content
FROM gd_sections s
JOIN gd_edges e ON s.id = e.source_id
JOIN gd_sections t ON e.target_id = t.id
WHERE s.id = 'a';
-- Expected: 'B'
```

## Related Files

| File | Description |
|------|-------------|
| `schema/duckagentfs.sql` | Property graph definition |
| `sdk/rust/src/graphdocs/query.rs` | Graph query helpers (future) |

## Implementation Notes

1. **DuckPGQ Dependency**: Property graph syntax requires the DuckPGQ extension
2. **Fallback Strategy**: Standard SQL queries provide equivalent functionality
3. **Recursive CTEs**: Used for inheritance chain traversal
4. **Depth Limits**: Prevent infinite recursion in self-referential queries
