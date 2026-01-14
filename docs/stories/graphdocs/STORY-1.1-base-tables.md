# STORY-1.1: Base Tables

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-1.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 1 - Schema GraphDocs |
| **Status** | Done |
| **Priority** | High |
| **File** | `schema/duckagentfs.sql` |
| **Dependencies** | EPIC-DUCKAGENTFS-001 |

## User Story

**As a** developer
**I want** tables to store documents as graphs
**So that** I have the necessary data structure for GraphDocs

## Acceptance Criteria

- [x] Table `gd_documents` (document metadata)
- [x] Table `gd_sections` (document sections)
- [x] Table `gd_variables` (variables for substitution)
- [x] Table `gd_edges` (relationships between sections)
- [x] Indexes for efficient queries

## Technical Specification

### Table: gd_documents

Stores document metadata including title, language, and optional base template for inheritance.

```sql
CREATE TABLE gd_documents (
    id VARCHAR PRIMARY KEY,
    title VARCHAR NOT NULL,
    description TEXT,
    base_template VARCHAR REFERENCES gd_documents(id),
    language VARCHAR DEFAULT 'en',
    version INTEGER DEFAULT 1,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Index for template lookups
CREATE INDEX idx_gd_documents_base ON gd_documents(base_template);
```

### Table: gd_sections

Stores document sections with hierarchical structure support.

```sql
CREATE TABLE gd_sections (
    id VARCHAR PRIMARY KEY,
    document_id VARCHAR NOT NULL REFERENCES gd_documents(id) ON DELETE CASCADE,
    parent_id VARCHAR REFERENCES gd_sections(id),
    source_section VARCHAR REFERENCES gd_sections(id), -- For inheritance overrides
    section_type VARCHAR NOT NULL, -- heading, paragraph, list, code, table, blockquote
    level INTEGER DEFAULT 0, -- For headings (1-6)
    order_idx INTEGER NOT NULL,
    content TEXT NOT NULL,
    condition VARCHAR, -- Optional: variable name for conditional rendering
    is_inherited BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT valid_section_type CHECK (
        section_type IN ('heading', 'paragraph', 'list', 'code', 'table', 'blockquote', 'hr')
    ),
    CONSTRAINT valid_heading_level CHECK (
        section_type != 'heading' OR (level >= 1 AND level <= 6)
    )
);

-- Index for document sections lookup
CREATE INDEX idx_gd_sections_document ON gd_sections(document_id, order_idx);

-- Index for parent-child relationships
CREATE INDEX idx_gd_sections_parent ON gd_sections(parent_id);
```

### Table: gd_variables

Stores variables for template substitution.

```sql
CREATE TABLE gd_variables (
    id VARCHAR PRIMARY KEY,
    document_id VARCHAR NOT NULL REFERENCES gd_documents(id) ON DELETE CASCADE,
    name VARCHAR NOT NULL,
    value JSON NOT NULL, -- JSON for type flexibility
    var_type VARCHAR DEFAULT 'string', -- string, number, boolean, array, object
    description TEXT,
    is_inherited BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,

    UNIQUE(document_id, name),
    CONSTRAINT valid_var_type CHECK (
        var_type IN ('string', 'number', 'boolean', 'array', 'object')
    )
);

-- Index for variable lookups
CREATE INDEX idx_gd_variables_document ON gd_variables(document_id);
CREATE INDEX idx_gd_variables_name ON gd_variables(name);
```

### Table: gd_edges

Stores relationships between sections for graph traversal.

```sql
CREATE TABLE gd_edges (
    id VARCHAR PRIMARY KEY,
    source_id VARCHAR NOT NULL REFERENCES gd_sections(id) ON DELETE CASCADE,
    target_id VARCHAR NOT NULL REFERENCES gd_sections(id) ON DELETE CASCADE,
    edge_type VARCHAR NOT NULL, -- follows, contains, references
    weight FLOAT DEFAULT 1.0,
    metadata JSON,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,

    UNIQUE(source_id, target_id, edge_type),
    CONSTRAINT valid_edge_type CHECK (
        edge_type IN ('follows', 'contains', 'references', 'links_to')
    )
);

-- Indexes for graph traversal
CREATE INDEX idx_gd_edges_source ON gd_edges(source_id);
CREATE INDEX idx_gd_edges_target ON gd_edges(target_id);
CREATE INDEX idx_gd_edges_type ON gd_edges(edge_type);
```

### Data Model Relationships

```
gd_documents
    │
    ├── 1:N ──> gd_sections
    │               │
    │               ├── parent_id ──> gd_sections (self-ref, hierarchy)
    │               └── source_section ──> gd_sections (inheritance)
    │
    ├── 1:N ──> gd_variables
    │
    └── base_template ──> gd_documents (self-ref, inheritance)

gd_sections <──> gd_edges <──> gd_sections (graph relationships)
```

## Tests

### Test 1: Create Document with Sections
```sql
-- Create document
INSERT INTO gd_documents (id, title) VALUES ('test-doc', 'Test Document');

-- Add sections
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES
    ('s1', 'test-doc', 'heading', 1, 0, '# Test'),
    ('s2', 'test-doc', 'paragraph', 0, 1, 'Hello world'),
    ('s3', 'test-doc', 'code', 0, 2, '```rust\nfn main() {}\n```');

-- Verify
SELECT COUNT(*) FROM gd_sections WHERE document_id = 'test-doc';
-- Expected: 3
```

### Test 2: Variable Substitution Data
```sql
-- Add variables
INSERT INTO gd_variables (id, document_id, name, value, var_type)
VALUES
    ('v1', 'test-doc', 'project_name', '"My Project"', 'string'),
    ('v2', 'test-doc', 'version', '1.0', 'number'),
    ('v3', 'test-doc', 'features', '["a", "b", "c"]', 'array');

-- Query variables for rendering
SELECT name, value, var_type
FROM gd_variables
WHERE document_id = 'test-doc';
```

### Test 3: Template Inheritance
```sql
-- Create template
INSERT INTO gd_documents (id, title) VALUES ('template', 'Base Template');
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES ('t1', 'template', 'heading', 1, 0, '# {{title}}');

-- Create child document
INSERT INTO gd_documents (id, title, base_template)
VALUES ('child', 'Child Doc', 'template');

-- Verify inheritance
SELECT d.id, d.title, d.base_template
FROM gd_documents d
WHERE d.id = 'child';
```

## Related Files

| File | Description |
|------|-------------|
| `schema/duckagentfs.sql` | DDL definitions |
| `sdk/rust/src/graphdocs/mod.rs` | Rust module (future) |

## Implementation Notes

1. **JSON for Variables**: Using JSON type allows flexible value types without separate tables
2. **Cascade Deletes**: Sections and variables are deleted when document is deleted
3. **Self-Referential**: Both documents and sections have self-references for hierarchy
4. **Unique Constraints**: Prevent duplicate variable names per document
