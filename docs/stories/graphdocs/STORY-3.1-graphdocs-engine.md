# STORY-3.1: GraphDocsEngine

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-3.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 3 - Rendering Engine |
| **Status** | Done |
| **Priority** | High |
| **File** | `sdk/rust/src/graphdocs/engine.rs` |
| **Dependencies** | STORY-1.1, STORY-1.2 |

## User Story

**As a** developer
**I want** a rendering engine
**So that** I can generate Markdown from the graph

## Acceptance Criteria

- [x] Resolve template inheritance
- [x] Load variables (with inheritance)
- [x] Order sections by `order_idx`
- [x] Substitute `{{variable}}` with values
- [x] Generate formatted Markdown

## Technical Specification

### Engine Structure

```rust
// sdk/rust/src/graphdocs/engine.rs

use std::collections::HashMap;
use serde_json::Value;

/// GraphDocs rendering engine
pub struct GraphDocsEngine {
    pool: DuckConnectionPool,
}

/// Rendered document result
#[derive(Debug)]
pub struct RenderedDocument {
    pub markdown: String,
    pub variables_used: Vec<String>,
    pub missing_variables: Vec<String>,
}

/// Section ready for rendering
#[derive(Debug, Clone)]
struct ResolvedSection {
    section_type: String,
    level: Option<u8>,
    content: String,
    order_idx: i32,
    is_inherited: bool,
}

impl GraphDocsEngine {
    pub fn new(pool: DuckConnectionPool) -> Self {
        Self { pool }
    }

    /// Render a document to Markdown
    pub async fn render(&self, doc_id: &str) -> Result<String> {
        let result = self.render_full(doc_id).await?;
        Ok(result.markdown)
    }

    /// Render with full details
    pub async fn render_full(&self, doc_id: &str) -> Result<RenderedDocument> {
        let conn = self.pool.get_read_connection().await?;

        // 1. Load document metadata
        let doc = self.load_document(&conn, doc_id).await?;

        // 2. Resolve inheritance chain
        let inheritance_chain = self.resolve_inheritance(&conn, doc_id).await?;

        // 3. Collect all sections (with inheritance)
        let sections = self.collect_sections(&conn, &inheritance_chain).await?;

        // 4. Collect all variables (with inheritance)
        let variables = self.collect_variables(&conn, &inheritance_chain).await?;

        // 5. Render sections to Markdown
        let (markdown, vars_used, missing) = self.render_sections(&sections, &variables);

        Ok(RenderedDocument {
            markdown,
            variables_used: vars_used,
            missing_variables: missing,
        })
    }

    /// Render at a specific event_id (time-travel)
    pub async fn render_at(&self, doc_id: &str, event_id: i64) -> Result<String> {
        // Use time-travel view
        let conn = self.pool.get_read_connection().await?;

        // This would use a filtered view or CTE
        // For now, placeholder - full implementation in STORY-3.3
        todo!("Time-travel rendering")
    }

    /// Get all variables for a document
    pub async fn get_variables(&self, doc_id: &str) -> Result<HashMap<String, Value>> {
        let conn = self.pool.get_read_connection().await?;
        let chain = self.resolve_inheritance(&conn, doc_id).await?;
        self.collect_variables(&conn, &chain).await
    }

    /// Set a variable value
    pub async fn set_variable(
        &self,
        doc_id: &str,
        name: &str,
        value: Value,
    ) -> Result<()> {
        let conn = self.pool.get_write_connection().await?;

        // Check if variable exists
        let var_id: Option<String> = conn.query_row(
            "SELECT id FROM gd_variables WHERE document_id = ? AND name = ?",
            params![doc_id, name],
            |r| r.get(0),
        ).optional()?;

        match var_id {
            Some(id) => {
                // Update existing
                conn.execute(
                    "UPDATE gd_variables SET value = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                    params![value.to_string(), id],
                )?;
            }
            None => {
                // Insert new
                let id = uuid::Uuid::new_v4().to_string();
                let var_type = match &value {
                    Value::String(_) => "string",
                    Value::Number(_) => "number",
                    Value::Bool(_) => "boolean",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                    Value::Null => "string",
                };
                conn.execute(
                    r#"INSERT INTO gd_variables (id, document_id, name, value, var_type)
                       VALUES (?, ?, ?, ?, ?)"#,
                    params![id, doc_id, name, value.to_string(), var_type],
                )?;
            }
        }

        Ok(())
    }

    // === Private Methods ===

    async fn load_document(&self, conn: &DuckConnection, doc_id: &str) -> Result<Document> {
        conn.query_row(
            "SELECT id, title, base_template FROM gd_documents WHERE id = ?",
            [doc_id],
            |row| Ok(Document {
                id: row.get(0)?,
                title: row.get(1)?,
                base_template: row.get(2)?,
            }),
        ).map_err(|_| anyhow::anyhow!("Document not found: {}", doc_id))
    }

    async fn resolve_inheritance(
        &self,
        conn: &DuckConnection,
        doc_id: &str,
    ) -> Result<Vec<String>> {
        let mut chain = vec![doc_id.to_string()];
        let mut current = doc_id.to_string();
        let mut depth = 0;
        const MAX_DEPTH: usize = 10;

        loop {
            let base: Option<String> = conn.query_row(
                "SELECT base_template FROM gd_documents WHERE id = ?",
                [&current],
                |r| r.get(0),
            ).optional()?.flatten();

            match base {
                Some(base_id) => {
                    if chain.contains(&base_id) {
                        return Err(anyhow::anyhow!(
                            "Circular inheritance detected: {} -> {}",
                            current, base_id
                        ));
                    }
                    if depth >= MAX_DEPTH {
                        return Err(anyhow::anyhow!(
                            "Inheritance depth exceeded maximum of {}",
                            MAX_DEPTH
                        ));
                    }
                    chain.push(base_id.clone());
                    current = base_id;
                    depth += 1;
                }
                None => break,
            }
        }

        // Reverse so base templates come first
        chain.reverse();
        Ok(chain)
    }

    async fn collect_sections(
        &self,
        conn: &DuckConnection,
        chain: &[String],
    ) -> Result<Vec<ResolvedSection>> {
        let mut sections: HashMap<String, ResolvedSection> = HashMap::new();

        // Process from base to child (child overrides)
        for doc_id in chain {
            let mut stmt = conn.prepare(
                r#"SELECT id, section_type, level, order_idx, content, source_section
                   FROM gd_sections
                   WHERE document_id = ?
                   ORDER BY order_idx"#,
            )?;

            let rows = stmt.query_map([doc_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,      // id
                    row.get::<_, String>(1)?,      // section_type
                    row.get::<_, Option<i32>>(2)?, // level
                    row.get::<_, i32>(3)?,         // order_idx
                    row.get::<_, String>(4)?,      // content
                    row.get::<_, Option<String>>(5)?, // source_section
                ))
            })?;

            for row in rows {
                let (id, section_type, level, order_idx, content, source_section) = row?;

                let key = source_section.unwrap_or(id);
                let is_inherited = doc_id != chain.last().unwrap();

                sections.insert(key, ResolvedSection {
                    section_type,
                    level: level.map(|l| l as u8),
                    content,
                    order_idx,
                    is_inherited,
                });
            }
        }

        // Sort by order_idx
        let mut result: Vec<_> = sections.into_values().collect();
        result.sort_by_key(|s| s.order_idx);
        Ok(result)
    }

    async fn collect_variables(
        &self,
        conn: &DuckConnection,
        chain: &[String],
    ) -> Result<HashMap<String, Value>> {
        let mut variables: HashMap<String, Value> = HashMap::new();

        // Process from base to child (child overrides)
        for doc_id in chain {
            let mut stmt = conn.prepare(
                "SELECT name, value FROM gd_variables WHERE document_id = ?",
            )?;

            let rows = stmt.query_map([doc_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                ))
            })?;

            for row in rows {
                let (name, value_str) = row?;
                let value: Value = serde_json::from_str(&value_str)
                    .unwrap_or(Value::String(value_str));
                variables.insert(name, value);
            }
        }

        Ok(variables)
    }

    fn render_sections(
        &self,
        sections: &[ResolvedSection],
        variables: &HashMap<String, Value>,
    ) -> (String, Vec<String>, Vec<String>) {
        let mut output = String::new();
        let mut vars_used = Vec::new();
        let mut missing = Vec::new();

        for section in sections {
            let rendered = self.render_section(section, variables, &mut vars_used, &mut missing);
            output.push_str(&rendered);
            output.push_str("\n\n");
        }

        // Deduplicate
        vars_used.sort();
        vars_used.dedup();
        missing.sort();
        missing.dedup();

        (output.trim().to_string(), vars_used, missing)
    }

    fn render_section(
        &self,
        section: &ResolvedSection,
        variables: &HashMap<String, Value>,
        vars_used: &mut Vec<String>,
        missing: &mut Vec<String>,
    ) -> String {
        let mut content = section.content.clone();

        // Find and substitute variables
        let re = regex::Regex::new(r"\{\{(\w+)\}\}").unwrap();
        for cap in re.captures_iter(&section.content) {
            let var_name = &cap[1];
            vars_used.push(var_name.to_string());

            if let Some(value) = variables.get(var_name) {
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Array(arr) => arr.iter()
                        .map(|v| format!("- {}", value_to_string(v)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Value::Object(_) => value.to_string(),
                    Value::Null => "".to_string(),
                };
                content = content.replace(&cap[0], &replacement);
            } else {
                missing.push(var_name.to_string());
            }
        }

        // Format based on section type
        match section.section_type.as_str() {
            "heading" => {
                let prefix = "#".repeat(section.level.unwrap_or(1) as usize);
                if !content.starts_with('#') {
                    format!("{} {}", prefix, content)
                } else {
                    content
                }
            }
            "code" => {
                if !content.starts_with("```") {
                    format!("```\n{}\n```", content)
                } else {
                    content
                }
            }
            "list" => content,
            "blockquote" => {
                content.lines()
                    .map(|line| format!("> {}", line))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            "hr" => "---".to_string(),
            _ => content,
        }
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

struct Document {
    id: String,
    title: String,
    base_template: Option<String>,
}
```

### Usage Example

```rust
use graphdocs::engine::GraphDocsEngine;

let engine = GraphDocsEngine::new(pool);

// Set variables
engine.set_variable("my-doc", "project_name", json!("My Project")).await?;
engine.set_variable("my-doc", "version", json!("1.0.0")).await?;

// Render
let markdown = engine.render("my-doc").await?;
println!("{}", markdown);

// Or get full details
let result = engine.render_full("my-doc").await?;
println!("Variables used: {:?}", result.variables_used);
if !result.missing_variables.is_empty() {
    println!("Warning: missing variables: {:?}", result.missing_variables);
}
```

## Tests

### Test 1: Simple Render
```rust
#[tokio::test]
async fn test_simple_render() {
    let pool = setup_test_pool().await;
    let engine = GraphDocsEngine::new(pool.clone());

    // Setup
    let conn = pool.get_write_connection().await.unwrap();
    conn.execute("INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')", []).unwrap();
    conn.execute(
        "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'test', 'heading', 1, 0, '# Hello')",
        [],
    ).unwrap();

    // Render
    let md = engine.render("test").await.unwrap();
    assert!(md.contains("# Hello"));
}
```

### Test 2: Variable Substitution
```rust
#[tokio::test]
async fn test_variable_substitution() {
    let pool = setup_test_pool().await;
    let engine = GraphDocsEngine::new(pool.clone());

    // Setup doc with variable
    setup_doc_with_var(&pool, "test", "{{name}}").await;

    // Set variable
    engine.set_variable("test", "name", json!("World")).await.unwrap();

    // Render
    let result = engine.render_full("test").await.unwrap();
    assert!(result.markdown.contains("World"));
    assert!(result.variables_used.contains(&"name".to_string()));
    assert!(result.missing_variables.is_empty());
}
```

### Test 3: Missing Variable Detection
```rust
#[tokio::test]
async fn test_missing_variables() {
    let pool = setup_test_pool().await;
    let engine = GraphDocsEngine::new(pool.clone());

    // Setup doc with variable but don't set it
    setup_doc_with_var(&pool, "test", "{{missing}}").await;

    // Render
    let result = engine.render_full("test").await.unwrap();
    assert!(result.missing_variables.contains(&"missing".to_string()));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/engine.rs` | Engine implementation |
| `sdk/rust/src/graphdocs/mod.rs` | Module exports |
| `schema/duckagentfs.sql` | Database schema |

## Implementation Notes

1. **Inheritance Order**: Base templates processed first, children override
2. **Cycle Detection**: Prevents infinite loops in template inheritance
3. **Variable Types**: JSON values support strings, numbers, booleans, arrays
4. **Missing Variables**: Detected but not fatal - allows partial rendering

---

## Dev Agent Record

### Agent Model Used
Claude Opus 4.5 (claude-opus-4-5-20251101)

### Debug Log References
N/A - No blocking issues encountered

### Completion Notes
- Implemented `GraphDocsEngine` with full template inheritance support
- Adapted story's async API design to work with DuckDB's synchronous connection pool using `spawn_blocking`
- Added circular inheritance detection with MAX_DEPTH (10) limit
- Variable substitution supports all JSON types: string, number, boolean, array, object
- Arrays render as Markdown list items
- Section types supported: heading, paragraph, code, blockquote, list, hr
- All 11 tests pass covering: simple render, variable substitution, missing variables, template inheritance, variable override, section ordering, circular inheritance detection, document not found, get_variables, section types, array variables

### File List
| File | Action |
|------|--------|
| `sdk/rust/src/graphdocs/engine.rs` | Created |
| `sdk/rust/src/graphdocs/mod.rs` | Modified (added engine module export) |

### Change Log
| Date | Change |
|------|--------|
| 2026-01-16 | Initial implementation of GraphDocsEngine with all acceptance criteria met |

---

## QA Results

### Review Date: 2026-01-16

### Reviewed By: Quinn (Test Architect)

### Code Quality Assessment

**Overall: EXCELLENT** - Clean, well-documented Rust implementation with comprehensive test coverage.

**Strengths:**
- Excellent documentation with module-level `//!` docs and `///` doc comments on all public APIs
- Proper async/sync pattern using `spawn_blocking` to bridge DuckDB's synchronous API
- Clean separation of concerns between public async methods and private sync helpers
- Comprehensive error handling with descriptive error messages
- Well-structured test suite with isolated in-memory databases

**Design Decisions (Appropriate):**
- Adapted story's async API design to work with DuckDB's synchronous nature
- Used `#[allow(dead_code)]` for `is_inherited` field - reserved for future use
- `render_at()` returns clear error indicating STORY-3.3 dependency

### Refactoring Performed

None required - code quality is production-ready.

### Compliance Check

- Coding Standards: ✓ Follows Rust 2021 edition, proper naming conventions, `rustfmt` compliant
- Project Structure: ✓ Module placed in `sdk/rust/src/graphdocs/engine.rs`, exports in `mod.rs`
- Testing Strategy: ✓ Unit tests inline with `#[cfg(test)]`, 11 comprehensive tests
- All ACs Met: ✓ All 5 acceptance criteria verified with test coverage

### Improvements Checklist

- [x] All acceptance criteria implemented with tests
- [x] Documentation complete with examples
- [x] Error handling comprehensive
- [x] Parameterized SQL queries (no injection vulnerabilities)
- [x] Connection pooling used correctly
- [ ] **Future**: Consider caching compiled Regex in `render_section()` (minor optimization)
- [ ] **Future**: `render_at()` implementation deferred to STORY-3.3

### Security Review

**Status: PASS**

- ✓ Parameterized queries used throughout (`params![]` macro)
- ✓ No raw SQL string concatenation
- ✓ Document IDs validated through database lookups
- ✓ Circular inheritance detection prevents DoS via infinite loops
- ✓ MAX_DEPTH limit prevents stack overflow in deep inheritance chains

### Performance Considerations

**Status: PASS**

- ✓ Uses `spawn_blocking` to avoid blocking async runtime on DB operations
- ✓ Connection pooling via `DuckConnectionPool`
- ✓ Sections collected once and sorted in memory (efficient for typical document sizes)
- ⚠ Regex compiled per `render_section()` call - acceptable for current use, but could be cached if rendering large documents frequently

### Files Modified During Review

None - no refactoring required.

### Gate Status

Gate: **PASS** → `docs/qa/gates/3.1-graphdocs-engine.yml`

### Recommended Status

**✓ Ready for Done** - All acceptance criteria met, comprehensive test coverage, clean code quality. Story owner can proceed to mark as Done.
