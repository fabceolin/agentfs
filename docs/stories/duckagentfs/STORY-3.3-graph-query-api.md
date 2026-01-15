# STORY-3.3: Graph Query API

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-3.3 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 3 - Property Graphs (DuckPGQ) |
| **Status** | Ready for Development |
| **Priority** | Medium |
| **File** | `sdk/rust/src/code_graph.rs` (new) |
| **Dependencies** | STORY-3.1, STORY-3.2 |

## User Story

**As a** developer
**I want** APIs to query the code graph
**So that** I can navigate dependencies

## Acceptance Criteria

- [ ] `get_callers(symbol)`: who calls this symbol
- [ ] `get_callees(symbol)`: who this symbol calls
- [ ] `get_dependencies(file)`: dependencies of a file
- [ ] `get_impact(symbol)`: impact analysis for changes
- [ ] CLI commands for graph queries

## Technical Specification

### CodeGraph API

```rust
use crate::filesystem::DuckAgentFS;

pub struct CodeGraph<'a> {
    fs: &'a DuckAgentFS,
}

impl<'a> CodeGraph<'a> {
    pub fn new(fs: &'a DuckAgentFS) -> Self {
        Self { fs }
    }

    /// Find all symbols that call the target symbol
    pub async fn get_callers(&self, symbol_id: &str) -> Result<Vec<SymbolInfo>> {
        let conn = self.fs.pool.get_connection().await?;

        let callers = conn.query_map(
            r#"
            SELECT s.id, s.name, s.kind, s.language, t.path
            FROM code_dependencies d
            JOIN code_symbols s ON s.id = d.source_id
            JOIN fs_tree t ON t.inode = s.inode
            WHERE d.target_id = ?
              AND d.dep_type = 'calls'
            ORDER BY s.name
            "#,
            params![symbol_id],
            |row| Ok(SymbolInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                language: row.get(3)?,
                path: row.get(4)?,
            })
        )?;

        Ok(callers)
    }

    /// Find all symbols that this symbol calls
    pub async fn get_callees(&self, symbol_id: &str) -> Result<Vec<SymbolInfo>> {
        let conn = self.fs.pool.get_connection().await?;

        let callees = conn.query_map(
            r#"
            SELECT s.id, s.name, s.kind, s.language, t.path
            FROM code_dependencies d
            JOIN code_symbols s ON s.id = d.target_id
            JOIN fs_tree t ON t.inode = s.inode
            WHERE d.source_id = ?
              AND d.dep_type = 'calls'
            ORDER BY s.name
            "#,
            params![symbol_id],
            |row| Ok(SymbolInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                language: row.get(3)?,
                path: row.get(4)?,
            })
        )?;

        Ok(callees)
    }

    /// Get all dependencies of a file
    pub async fn get_file_dependencies(&self, path: &str) -> Result<FileDependencies> {
        let conn = self.fs.pool.get_connection().await?;

        let inode = self.fs.resolve_path(path, true).await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        // Get symbols defined in this file
        let symbols: Vec<SymbolInfo> = conn.query_map(
            "SELECT id, name, kind, language FROM code_symbols WHERE inode = ?",
            params![inode],
            |row| Ok(SymbolInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                language: row.get(3)?,
                path: path.to_string(),
            })
        )?;

        // Get external dependencies (what this file depends on)
        let depends_on: Vec<DependencyInfo> = conn.query_map(
            r#"
            SELECT DISTINCT
                t.path as target_path,
                s.name as target_name,
                d.dep_type
            FROM code_symbols src
            JOIN code_dependencies d ON d.source_id = src.id
            JOIN code_symbols s ON s.id = d.target_id
            JOIN fs_tree t ON t.inode = s.inode
            WHERE src.inode = ?
              AND s.inode != ?
            ORDER BY t.path, s.name
            "#,
            params![inode, inode],
            |row| Ok(DependencyInfo {
                path: row.get(0)?,
                symbol: row.get(1)?,
                dep_type: row.get(2)?,
            })
        )?;

        // Get dependents (what depends on this file)
        let dependents: Vec<DependencyInfo> = conn.query_map(
            r#"
            SELECT DISTINCT
                t.path as source_path,
                src.name as source_name,
                d.dep_type
            FROM code_symbols target
            JOIN code_dependencies d ON d.target_id = target.id
            JOIN code_symbols src ON src.id = d.source_id
            JOIN fs_tree t ON t.inode = src.inode
            WHERE target.inode = ?
              AND src.inode != ?
            ORDER BY t.path, src.name
            "#,
            params![inode, inode],
            |row| Ok(DependencyInfo {
                path: row.get(0)?,
                symbol: row.get(1)?,
                dep_type: row.get(2)?,
            })
        )?;

        Ok(FileDependencies {
            path: path.to_string(),
            symbols,
            depends_on,
            dependents,
        })
    }

    /// Impact analysis: what would be affected if symbol changes
    pub async fn get_impact(&self, symbol_id: &str, max_depth: usize) -> Result<ImpactAnalysis> {
        let conn = self.fs.pool.get_connection().await?;

        // Recursive query for transitive dependents
        let affected: Vec<ImpactedSymbol> = conn.query_map(
            r#"
            WITH RECURSIVE impact AS (
                -- Direct dependents
                SELECT source_id, 1 as depth
                FROM code_dependencies
                WHERE target_id = ?

                UNION ALL

                -- Transitive dependents
                SELECT d.source_id, i.depth + 1
                FROM code_dependencies d
                JOIN impact i ON d.target_id = i.source_id
                WHERE i.depth < ?
            )
            SELECT DISTINCT
                s.id,
                s.name,
                s.kind,
                t.path,
                MIN(i.depth) as distance
            FROM impact i
            JOIN code_symbols s ON s.id = i.source_id
            JOIN fs_tree t ON t.inode = s.inode
            GROUP BY s.id, s.name, s.kind, t.path
            ORDER BY distance, t.path
            "#,
            params![symbol_id, max_depth],
            |row| Ok(ImpactedSymbol {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                path: row.get(3)?,
                distance: row.get(4)?,
            })
        )?;

        // Count affected files
        let affected_files: HashSet<String> = affected.iter()
            .map(|s| s.path.clone())
            .collect();

        Ok(ImpactAnalysis {
            target_symbol: symbol_id.to_string(),
            affected_symbols: affected,
            affected_file_count: affected_files.len(),
            max_depth_reached: max_depth,
        })
    }

    /// Find dead code (symbols never referenced)
    pub async fn find_dead_code(&self) -> Result<Vec<SymbolInfo>> {
        let conn = self.fs.pool.get_connection().await?;

        let dead: Vec<SymbolInfo> = conn.query_map(
            r#"
            SELECT s.id, s.name, s.kind, s.language, t.path
            FROM code_symbols s
            JOIN fs_tree t ON t.inode = s.inode
            LEFT JOIN code_dependencies d ON d.target_id = s.id
            WHERE d.source_id IS NULL
              AND s.kind IN ('function', 'method')
              AND s.visibility = 'private'
            ORDER BY t.path, s.name
            "#,
            [],
            |row| Ok(SymbolInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                language: row.get(3)?,
                path: row.get(4)?,
            })
        )?;

        Ok(dead)
    }

    /// Find cycles in the dependency graph
    pub async fn find_cycles(&self) -> Result<Vec<Vec<String>>> {
        // This is complex - need Tarjan's or similar algorithm
        // DuckPGQ might have built-in cycle detection
        todo!()
    }
}
```

### Data Types

```rust
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub language: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct DependencyInfo {
    pub path: String,
    pub symbol: String,
    pub dep_type: String,
}

#[derive(Debug, Clone)]
pub struct FileDependencies {
    pub path: String,
    pub symbols: Vec<SymbolInfo>,
    pub depends_on: Vec<DependencyInfo>,
    pub dependents: Vec<DependencyInfo>,
}

#[derive(Debug, Clone)]
pub struct ImpactedSymbol {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub distance: i32,
}

#[derive(Debug, Clone)]
pub struct ImpactAnalysis {
    pub target_symbol: String,
    pub affected_symbols: Vec<ImpactedSymbol>,
    pub affected_file_count: usize,
    pub max_depth_reached: usize,
}
```

### CLI Commands

```bash
# Find callers of a function
agentfs graph callers my-agent "src/lib.rs:process"

# Find callees of a function
agentfs graph callees my-agent "src/main.rs:main"

# Show file dependencies
agentfs graph deps my-agent /src/main.rs

# Impact analysis
agentfs graph impact my-agent "src/lib.rs:Data" --depth 5

# Find dead code
agentfs graph dead-code my-agent

# Visualize (output DOT format)
agentfs graph visualize my-agent --format dot > graph.dot
dot -Tpng graph.dot -o graph.png
```

## Tests

### Test 1: Get Callers
```rust
#[tokio::test]
async fn test_get_callers() {
    let fs = setup_test_fs_with_graph().await;
    let graph = CodeGraph::new(&fs);

    // main calls helper
    let callers = graph.get_callers("test:helper").await.unwrap();

    assert!(!callers.is_empty());
    assert!(callers.iter().any(|c| c.name == "main"));
}
```

### Test 2: Impact Analysis
```rust
#[tokio::test]
async fn test_impact_analysis() {
    let fs = setup_test_fs_with_graph().await;
    let graph = CodeGraph::new(&fs);

    // If we change Data struct, what's affected?
    let impact = graph.get_impact("test:Data", 5).await.unwrap();

    assert!(!impact.affected_symbols.is_empty());
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/code_graph.rs` | CodeGraph API |
| `cli/src/cmd/graph.rs` | CLI commands |
| `schema/duckagentfs.sql` | Graph tables |

## Implementation Notes

1. **Performance**: For large codebases, consider:
   - Caching frequent queries
   - Limiting recursion depth
   - Using DuckPGQ native graph algorithms when available

2. **DuckPGQ Queries**: When DuckPGQ is loaded:
   ```sql
   -- Path finding
   MATCH p = (a:symbol)-[:depends_on*1..5]->(b:symbol)
   WHERE a.id = 'src/main.rs:main'
   RETURN p

   -- Shortest path
   MATCH (a:symbol), (b:symbol),
         p = shortestPath((a)-[:depends_on*]->(b))
   WHERE a.id = 'src/main.rs:main' AND b.id = 'src/lib.rs:helper'
   RETURN p
   ```

3. **Visualization**: Consider outputting:
   - DOT format for Graphviz
   - JSON for web visualization (D3.js, Cytoscape)
   - Mermaid for documentation
