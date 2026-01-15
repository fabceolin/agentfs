# STORY-3.2: Template Inheritance

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-3.2 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 3 - Rendering Engine |
| **Status** | Ready for Development |
| **Priority** | Medium |
| **File** | `sdk/rust/src/graphdocs/engine.rs` |
| **Dependencies** | STORY-3.1 |

## User Story

**As a** document author
**I want** to create documents that inherit from templates
**So that** I can reuse common structure

## Acceptance Criteria

- [ ] Child document inherits sections from parent
- [ ] Sections can be overridden via `source_section`
- [ ] Inherited variables can be overridden
- [ ] Unlimited inheritance chain (with max depth)
- [ ] Cycle detection

## Technical Specification

### Inheritance Model

```
                    ┌─────────────────┐
                    │  Base Template  │
                    │  - Section A    │
                    │  - Section B    │
                    │  - Section C    │
                    │  - var: title   │
                    └────────┬────────┘
                             │ inherits
                    ┌────────▼────────┐
                    │ Intermediate    │
                    │  - Section B*   │ ◄── override
                    │  - Section D    │ ◄── new
                    │  - var: title*  │ ◄── override
                    └────────┬────────┘
                             │ inherits
                    ┌────────▼────────┐
                    │  Final Doc      │
                    │  - Section A    │ ◄── inherited from base
                    │  - Section B*   │ ◄── inherited from intermediate
                    │  - Section C    │ ◄── inherited from base
                    │  - Section D    │ ◄── inherited from intermediate
                    │  - Section E    │ ◄── new
                    └─────────────────┘
```

### Section Override Mechanism

```sql
-- Base template with sections
INSERT INTO gd_documents (id, title) VALUES ('base', 'Base Template');
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES
    ('base-intro', 'base', 'heading', 1, 0, '# {{project_name}}'),
    ('base-desc', 'base', 'paragraph', 0, 1, '{{description}}'),
    ('base-install', 'base', 'heading', 2, 2, '## Installation');

-- Child document overriding a section
INSERT INTO gd_documents (id, title, base_template)
VALUES ('child', 'Child Doc', 'base');

-- Override the description section
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content, source_section)
VALUES ('child-desc', 'child', 'paragraph', 0, 1, 'Custom description here', 'base-desc');
-- source_section points to the section being overridden

-- Add a new section (not overriding anything)
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES ('child-extra', 'child', 'heading', 2, 3, '## Extra Section');
```

### Variable Inheritance

```sql
-- Base template variables
INSERT INTO gd_variables (id, document_id, name, value, var_type)
VALUES
    ('v1', 'base', 'project_name', '"Untitled"', 'string'),
    ('v2', 'base', 'description', '"No description"', 'string'),
    ('v3', 'base', 'version', '"0.0.0"', 'string');

-- Child overrides some variables
INSERT INTO gd_variables (id, document_id, name, value, var_type)
VALUES
    ('v4', 'child', 'project_name', '"My Project"', 'string'),
    -- description inherits from base
    ('v5', 'child', 'version', '"1.0.0"', 'string');
```

### Inheritance Resolution Implementation

```rust
// sdk/rust/src/graphdocs/inheritance.rs

/// Resolve the full inheritance chain for a document
pub struct InheritanceResolver<'a> {
    conn: &'a DuckConnection,
    max_depth: usize,
}

impl<'a> InheritanceResolver<'a> {
    pub fn new(conn: &'a DuckConnection) -> Self {
        Self { conn, max_depth: 10 }
    }

    /// Get inheritance chain from root to leaf
    pub fn resolve_chain(&self, doc_id: &str) -> Result<InheritanceChain> {
        let mut chain = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut current = Some(doc_id.to_string());

        while let Some(id) = current {
            // Cycle detection
            if !visited.insert(id.clone()) {
                return Err(InheritanceError::CycleDetected(id));
            }

            // Depth check
            if chain.len() >= self.max_depth {
                return Err(InheritanceError::MaxDepthExceeded(self.max_depth));
            }

            // Load document
            let doc = self.load_document(&id)?;
            chain.push(doc.clone());
            current = doc.base_template;
        }

        // Reverse so base is first
        chain.reverse();

        Ok(InheritanceChain { documents: chain })
    }

    fn load_document(&self, id: &str) -> Result<DocumentMeta> {
        self.conn.query_row(
            "SELECT id, title, base_template FROM gd_documents WHERE id = ?",
            [id],
            |row| Ok(DocumentMeta {
                id: row.get(0)?,
                title: row.get(1)?,
                base_template: row.get(2)?,
            }),
        ).map_err(|_| InheritanceError::DocumentNotFound(id.to_string()))
    }
}

#[derive(Debug)]
pub struct InheritanceChain {
    pub documents: Vec<DocumentMeta>,
}

impl InheritanceChain {
    /// Get document IDs from base to leaf
    pub fn doc_ids(&self) -> Vec<&str> {
        self.documents.iter().map(|d| d.id.as_str()).collect()
    }

    /// Get the root (base) document
    pub fn root(&self) -> Option<&DocumentMeta> {
        self.documents.first()
    }

    /// Get the leaf (final) document
    pub fn leaf(&self) -> Option<&DocumentMeta> {
        self.documents.last()
    }

    /// Get depth of inheritance
    pub fn depth(&self) -> usize {
        self.documents.len()
    }
}

#[derive(Debug, Clone)]
pub struct DocumentMeta {
    pub id: String,
    pub title: String,
    pub base_template: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum InheritanceError {
    #[error("Document not found: {0}")]
    DocumentNotFound(String),
    #[error("Circular inheritance detected at: {0}")]
    CycleDetected(String),
    #[error("Maximum inheritance depth ({0}) exceeded")]
    MaxDepthExceeded(usize),
}
```

### Section Merging

```rust
/// Merge sections from inheritance chain
pub struct SectionMerger<'a> {
    conn: &'a DuckConnection,
}

impl<'a> SectionMerger<'a> {
    pub fn new(conn: &'a DuckConnection) -> Self {
        Self { conn }
    }

    /// Collect and merge sections from entire inheritance chain
    pub fn merge(&self, chain: &InheritanceChain) -> Result<Vec<MergedSection>> {
        // Map: section_id -> MergedSection
        // When a child has source_section, it overrides that section
        let mut sections: HashMap<String, MergedSection> = HashMap::new();

        for doc in &chain.documents {
            let doc_sections = self.load_sections(&doc.id)?;

            for section in doc_sections {
                // Determine the key (what this section represents)
                let key = section.source_section
                    .clone()
                    .unwrap_or(section.id.clone());

                let is_final = doc.id == chain.leaf().map(|d| d.id.as_str()).unwrap_or("");

                sections.insert(key, MergedSection {
                    id: section.id,
                    section_type: section.section_type,
                    level: section.level,
                    order_idx: section.order_idx,
                    content: section.content,
                    from_document: doc.id.clone(),
                    is_inherited: !is_final,
                    source_section: section.source_section,
                });
            }
        }

        // Sort by order_idx
        let mut result: Vec<_> = sections.into_values().collect();
        result.sort_by_key(|s| s.order_idx);
        Ok(result)
    }

    fn load_sections(&self, doc_id: &str) -> Result<Vec<Section>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, section_type, level, order_idx, content, source_section
               FROM gd_sections
               WHERE document_id = ?
               ORDER BY order_idx"#,
        )?;

        let sections = stmt.query_map([doc_id], |row| {
            Ok(Section {
                id: row.get(0)?,
                section_type: row.get(1)?,
                level: row.get(2)?,
                order_idx: row.get(3)?,
                content: row.get(4)?,
                source_section: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

        Ok(sections)
    }
}

#[derive(Debug)]
pub struct MergedSection {
    pub id: String,
    pub section_type: String,
    pub level: Option<i32>,
    pub order_idx: i32,
    pub content: String,
    pub from_document: String,
    pub is_inherited: bool,
    pub source_section: Option<String>,
}

#[derive(Debug)]
struct Section {
    id: String,
    section_type: String,
    level: Option<i32>,
    order_idx: i32,
    content: String,
    source_section: Option<String>,
}
```

### Variable Merging

```rust
/// Merge variables from inheritance chain
pub struct VariableMerger<'a> {
    conn: &'a DuckConnection,
}

impl<'a> VariableMerger<'a> {
    pub fn new(conn: &'a DuckConnection) -> Self {
        Self { conn }
    }

    /// Collect and merge variables from entire inheritance chain
    pub fn merge(&self, chain: &InheritanceChain) -> Result<HashMap<String, MergedVariable>> {
        let mut variables: HashMap<String, MergedVariable> = HashMap::new();

        for doc in &chain.documents {
            let doc_vars = self.load_variables(&doc.id)?;

            for (name, value, var_type) in doc_vars {
                let is_final = doc.id == chain.leaf().map(|d| d.id.as_str()).unwrap_or("");

                variables.insert(name.clone(), MergedVariable {
                    name,
                    value,
                    var_type,
                    from_document: doc.id.clone(),
                    is_inherited: !is_final,
                });
            }
        }

        Ok(variables)
    }

    fn load_variables(&self, doc_id: &str) -> Result<Vec<(String, Value, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, value, var_type FROM gd_variables WHERE document_id = ?",
        )?;

        let vars = stmt.query_map([doc_id], |row| {
            let name: String = row.get(0)?;
            let value_str: String = row.get(1)?;
            let var_type: String = row.get(2)?;
            let value: Value = serde_json::from_str(&value_str)
                .unwrap_or(Value::String(value_str));
            Ok((name, value, var_type))
        })?
        .collect::<Result<Vec<_>, _>>()?;

        Ok(vars)
    }
}

#[derive(Debug)]
pub struct MergedVariable {
    pub name: String,
    pub value: Value,
    pub var_type: String,
    pub from_document: String,
    pub is_inherited: bool,
}
```

## Example

### Setup
```sql
-- README template
INSERT INTO gd_documents (id, title) VALUES ('readme-tmpl', 'README Template');
INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
VALUES
    ('t1', 'readme-tmpl', 'heading', 1, 0, '# {{project_name}}'),
    ('t2', 'readme-tmpl', 'paragraph', 0, 1, '{{description}}'),
    ('t3', 'readme-tmpl', 'heading', 2, 2, '## Installation'),
    ('t4', 'readme-tmpl', 'code', 0, 3, '```bash\n{{install_cmd}}\n```');
INSERT INTO gd_variables (id, document_id, name, value)
VALUES
    ('tv1', 'readme-tmpl', 'project_name', '"Project"'),
    ('tv2', 'readme-tmpl', 'description', '"A project"'),
    ('tv3', 'readme-tmpl', 'install_cmd', '"npm install"');

-- My project README inheriting from template
INSERT INTO gd_documents (id, title, base_template)
VALUES ('my-readme', 'My README', 'readme-tmpl');
-- Override just the variables
INSERT INTO gd_variables (id, document_id, name, value)
VALUES
    ('mv1', 'my-readme', 'project_name', '"AgentFS"'),
    ('mv2', 'my-readme', 'description', '"A filesystem for AI agents"'),
    ('mv3', 'my-readme', 'install_cmd', '"cargo install agentfs"');
```

### Rendered Output
```markdown
# AgentFS

A filesystem for AI agents

## Installation

```bash
cargo install agentfs
```
```

## Tests

### Test 1: Simple Inheritance
```rust
#[tokio::test]
async fn test_simple_inheritance() {
    let pool = setup_test_pool().await;
    let conn = pool.get_write_connection().await.unwrap();

    // Create base with section
    conn.execute("INSERT INTO gd_documents (id, title) VALUES ('base', 'Base')", []).unwrap();
    conn.execute(
        "INSERT INTO gd_sections (id, document_id, section_type, order_idx, content) VALUES ('s1', 'base', 'paragraph', 0, 'Base content')",
        [],
    ).unwrap();

    // Create child inheriting base
    conn.execute(
        "INSERT INTO gd_documents (id, title, base_template) VALUES ('child', 'Child', 'base')",
        [],
    ).unwrap();

    let engine = GraphDocsEngine::new(pool);
    let md = engine.render("child").await.unwrap();

    assert!(md.contains("Base content"));
}
```

### Test 2: Section Override
```rust
#[tokio::test]
async fn test_section_override() {
    // ... setup base with section 's1' containing "Original" ...

    // Child overrides s1
    conn.execute(
        "INSERT INTO gd_sections (id, document_id, section_type, order_idx, content, source_section) VALUES ('c1', 'child', 'paragraph', 0, 'Overridden', 's1')",
        [],
    ).unwrap();

    let md = engine.render("child").await.unwrap();

    assert!(!md.contains("Original"));
    assert!(md.contains("Overridden"));
}
```

### Test 3: Cycle Detection
```rust
#[tokio::test]
async fn test_cycle_detection() {
    let conn = pool.get_write_connection().await.unwrap();

    // Create cycle: A -> B -> A
    conn.execute("INSERT INTO gd_documents (id, title, base_template) VALUES ('a', 'A', 'b')", []).unwrap();
    conn.execute("INSERT INTO gd_documents (id, title, base_template) VALUES ('b', 'B', 'a')", []).unwrap();

    let resolver = InheritanceResolver::new(&conn);
    let result = resolver.resolve_chain("a");

    assert!(matches!(result, Err(InheritanceError::CycleDetected(_))));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/engine.rs` | Main engine |
| `sdk/rust/src/graphdocs/inheritance.rs` | Inheritance resolver |
| `schema/duckagentfs.sql` | Schema with base_template |
