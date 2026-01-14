# STORY-3.1: Schema Code Graph

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-3.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 3 - Property Graphs (DuckPGQ) |
| **Status** | Done |
| **Priority** | Medium |
| **File** | `schema/duckagentfs.sql` |
| **Dependencies** | STORY-1.1 |

## User Story

**As a** developer
**I want** tables for the code dependency graph
**So that** I can analyze relationships between symbols

## Technical Description

The code graph stores information about code symbols (functions, classes, modules) and their relationships (calls, imports, extends). This enables:

- Impact analysis: What will break if I change this function?
- Dead code detection: What code is never called?
- Dependency visualization: How is the codebase connected?
- Refactoring support: Safe rename, safe move

## Acceptance Criteria

- [x] Table `code_symbols` (functions, classes, modules)
- [x] Table `code_dependencies` (calls, imports, extends)
- [x] Property Graph `code_graph` definition (commented)
- [x] Indexes for efficient graph traversal
- [x] Foreign key to fs_current for file association

## Technical Specification

### code_symbols Table

```sql
CREATE TABLE IF NOT EXISTS code_symbols (
    -- Unique identifier: 'file:symbol_name' or generated UUID
    id          VARCHAR PRIMARY KEY,

    -- File this symbol is defined in
    inode       UBIGINT NOT NULL,

    -- Symbol name (function name, class name, etc.)
    name        VARCHAR NOT NULL,

    -- Symbol type
    kind        VARCHAR NOT NULL,  -- 'function', 'class', 'module', 'variable', 'type', 'interface', 'method'

    -- Programming language
    language    VARCHAR,

    -- Source location
    start_line  UINTEGER,
    end_line    UINTEGER,

    -- Additional info
    signature   VARCHAR,           -- Function/method signature
    docstring   VARCHAR,           -- Documentation string
    visibility  VARCHAR DEFAULT 'public',  -- 'public', 'private', 'protected', 'internal'

    -- Extensible metadata
    metadata    JSON,

    -- Foreign key to filesystem
    FOREIGN KEY (inode) REFERENCES fs_current(inode)
);

-- Indexes for efficient lookups
CREATE INDEX IF NOT EXISTS idx_code_symbols_inode ON code_symbols(inode);
CREATE INDEX IF NOT EXISTS idx_code_symbols_name ON code_symbols(name);
CREATE INDEX IF NOT EXISTS idx_code_symbols_kind ON code_symbols(kind);
CREATE INDEX IF NOT EXISTS idx_code_symbols_language ON code_symbols(language);
```

### code_dependencies Table

```sql
CREATE TABLE IF NOT EXISTS code_dependencies (
    -- Source symbol (caller, importer, extender)
    source_id   VARCHAR NOT NULL,

    -- Target symbol (callee, imported, base class)
    target_id   VARCHAR NOT NULL,

    -- Dependency type
    dep_type    VARCHAR NOT NULL,  -- 'calls', 'imports', 'extends', 'implements', 'uses', 'references'

    -- Dependency strength/weight (for ranking)
    weight      DOUBLE DEFAULT 1.0,

    -- Extensible metadata
    metadata    JSON,

    -- Composite primary key
    PRIMARY KEY (source_id, target_id, dep_type),

    -- Foreign keys
    FOREIGN KEY (source_id) REFERENCES code_symbols(id),
    FOREIGN KEY (target_id) REFERENCES code_symbols(id)
);

-- Indexes for graph traversal
CREATE INDEX IF NOT EXISTS idx_code_deps_source ON code_dependencies(source_id);
CREATE INDEX IF NOT EXISTS idx_code_deps_target ON code_dependencies(target_id);
CREATE INDEX IF NOT EXISTS idx_code_deps_type ON code_dependencies(dep_type);
```

### Property Graph Definition

```sql
-- Requires: INSTALL duckpgq; LOAD duckpgq;

CREATE PROPERTY GRAPH code_graph
VERTEX TABLES (
    code_symbols
        PROPERTIES (id, name, kind, language, signature, visibility)
        LABEL symbol
)
EDGE TABLES (
    code_dependencies
        SOURCE KEY (source_id) REFERENCES code_symbols (id)
        DESTINATION KEY (target_id) REFERENCES code_symbols (id)
        PROPERTIES (dep_type, weight)
        LABEL depends_on
);
```

### Symbol Types

| Kind | Description | Example |
|------|-------------|---------|
| `function` | Standalone function | `fn process_data()` |
| `method` | Class/struct method | `impl Foo { fn bar() }` |
| `class` | Class definition | `class User` |
| `struct` | Struct definition | `struct Config` |
| `interface` | Interface/trait | `trait Display` |
| `module` | Module/namespace | `mod utils` |
| `variable` | Global/module variable | `const MAX_SIZE` |
| `type` | Type alias | `type UserId = u64` |

### Dependency Types

| dep_type | Description | Example |
|----------|-------------|---------|
| `calls` | Function/method call | `foo()` calls `bar()` |
| `imports` | Module import | `use std::io` |
| `extends` | Class inheritance | `class B extends A` |
| `implements` | Interface implementation | `impl Trait for Foo` |
| `uses` | Type usage | `fn foo() -> User` |
| `references` | Variable reference | `x = CONSTANT` |

## Example Data

```sql
-- Insert symbols
INSERT INTO code_symbols (id, inode, name, kind, language, start_line, end_line, signature)
VALUES
    ('src/main.rs:main', 100, 'main', 'function', 'rust', 1, 10, 'fn main()'),
    ('src/main.rs:process_data', 100, 'process_data', 'function', 'rust', 12, 30, 'fn process_data(data: &Data) -> Result<Output>'),
    ('src/lib.rs:Data', 101, 'Data', 'struct', 'rust', 1, 20, 'struct Data'),
    ('src/lib.rs:Output', 101, 'Output', 'struct', 'rust', 22, 40, 'struct Output'),
    ('src/lib.rs:process', 101, 'process', 'function', 'rust', 42, 60, 'pub fn process(d: Data) -> Output');

-- Insert dependencies
INSERT INTO code_dependencies (source_id, target_id, dep_type, weight)
VALUES
    ('src/main.rs:main', 'src/main.rs:process_data', 'calls', 1.0),
    ('src/main.rs:process_data', 'src/lib.rs:Data', 'uses', 0.5),
    ('src/main.rs:process_data', 'src/lib.rs:Output', 'uses', 0.5),
    ('src/main.rs:process_data', 'src/lib.rs:process', 'calls', 1.0);
```

## Graph Queries

### Find Callers (Who calls this function?)

```sql
-- Traditional SQL
SELECT s.name, s.kind, d.dep_type
FROM code_dependencies d
JOIN code_symbols s ON s.id = d.source_id
WHERE d.target_id = 'src/lib.rs:process'
  AND d.dep_type = 'calls';

-- DuckPGQ (when available)
-- MATCH (caller:symbol)-[d:depends_on]->(callee:symbol)
-- WHERE callee.id = 'src/lib.rs:process' AND d.dep_type = 'calls'
-- RETURN caller.name, caller.kind
```

### Find Callees (What does this function call?)

```sql
SELECT s.name, s.kind, d.dep_type
FROM code_dependencies d
JOIN code_symbols s ON s.id = d.target_id
WHERE d.source_id = 'src/main.rs:main'
  AND d.dep_type = 'calls';
```

### Find All Dependencies (transitive)

```sql
-- Recursive CTE for transitive closure
WITH RECURSIVE deps AS (
    -- Base case: direct dependencies
    SELECT target_id, 1 as depth
    FROM code_dependencies
    WHERE source_id = 'src/main.rs:main'

    UNION ALL

    -- Recursive case: dependencies of dependencies
    SELECT d.target_id, deps.depth + 1
    FROM code_dependencies d
    JOIN deps ON d.source_id = deps.target_id
    WHERE deps.depth < 10  -- Limit depth to prevent infinite loops
)
SELECT DISTINCT target_id, MIN(depth) as min_depth
FROM deps
GROUP BY target_id
ORDER BY min_depth;
```

### Impact Analysis (What breaks if I change this?)

```sql
-- Find all symbols that depend on target symbol (reverse transitive)
WITH RECURSIVE impact AS (
    SELECT source_id, 1 as depth
    FROM code_dependencies
    WHERE target_id = 'src/lib.rs:Data'

    UNION ALL

    SELECT d.source_id, impact.depth + 1
    FROM code_dependencies d
    JOIN impact ON d.target_id = impact.source_id
    WHERE impact.depth < 10
)
SELECT DISTINCT s.name, s.kind, MIN(i.depth) as distance
FROM impact i
JOIN code_symbols s ON s.id = i.source_id
GROUP BY s.id, s.name, s.kind
ORDER BY distance;
```

## Tests

### Test 1: Insert and Query Symbol
```sql
INSERT INTO code_symbols (id, inode, name, kind, language)
VALUES ('test:foo', 1, 'foo', 'function', 'rust');

SELECT * FROM code_symbols WHERE name = 'foo';
-- Expected: 1 row
```

### Test 2: Dependency Tracking
```sql
INSERT INTO code_symbols (id, inode, name, kind, language)
VALUES
    ('test:a', 1, 'a', 'function', 'rust'),
    ('test:b', 1, 'b', 'function', 'rust');

INSERT INTO code_dependencies (source_id, target_id, dep_type)
VALUES ('test:a', 'test:b', 'calls');

SELECT COUNT(*) FROM code_dependencies WHERE source_id = 'test:a';
-- Expected: 1
```

## Related Files

| File | Description |
|------|-------------|
| `schema/duckagentfs.sql` | Complete DDL |
| `sdk/rust/src/code_graph.rs` | Graph API (new) |
| `sdk/rust/src/analyzer/mod.rs` | Code analyzers (new) |

## Implementation Notes

1. **Symbol ID Format**: Use `{file_path}:{symbol_name}` for uniqueness. For overloaded methods, append signature hash.

2. **Incremental Updates**: When a file changes:
   - Delete all symbols where `inode = changed_file`
   - Re-analyze file and insert new symbols
   - Dependencies will cascade delete via foreign key

3. **Language Support**: Start with Rust (tree-sitter-rust), then add TypeScript, Python.

4. **Performance**: For large codebases:
   - Batch inserts
   - Use prepared statements
   - Consider partitioning by language
