# STORY-3.3: Time-Travel Rendering

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-3.3 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 3 - Rendering Engine |
| **Status** | Ready for Development |
| **Priority** | Medium |
| **File** | `sdk/rust/src/graphdocs/engine.rs` |
| **Dependencies** | STORY-3.1, EPIC-DUCKAGENTFS-001 (STORY-1.4) |

## User Story

**As a** document author
**I want** to render previous versions of documents
**So that** I can view document history

## Acceptance Criteria

- [ ] `render_at(doc_id, event_id)` renders historical version
- [ ] Uses fs_journal filtered by event_id
- [ ] CLI: `agentfs graphdocs render <doc> --at <event_id>`

## Technical Specification

### Time-Travel Schema Extension

GraphDocs tables need to integrate with the append-only journal model from DuckAgentFS.

```sql
-- GraphDocs journal for time-travel
-- Each mutation to gd_* tables creates a journal entry

CREATE TABLE gd_journal (
    event_id BIGINT PRIMARY KEY DEFAULT nextval('gd_event_seq'),
    event_type VARCHAR NOT NULL, -- create, update, delete
    table_name VARCHAR NOT NULL, -- gd_documents, gd_sections, gd_variables, gd_edges
    record_id VARCHAR NOT NULL,
    old_data JSON,
    new_data JSON,
    event_time TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE SEQUENCE gd_event_seq START 1;

CREATE INDEX idx_gd_journal_record ON gd_journal(table_name, record_id);
CREATE INDEX idx_gd_journal_time ON gd_journal(event_time);
```

### Historical Views

```sql
-- View: Documents at a point in time
CREATE VIEW gd_documents_at AS
WITH latest_events AS (
    SELECT
        record_id,
        event_type,
        new_data,
        event_id,
        ROW_NUMBER() OVER (
            PARTITION BY record_id
            ORDER BY event_id DESC
        ) AS rn
    FROM gd_journal
    WHERE table_name = 'gd_documents'
)
SELECT
    new_data->>'id' AS id,
    new_data->>'title' AS title,
    new_data->>'base_template' AS base_template,
    new_data->>'language' AS language,
    CAST(new_data->>'version' AS INTEGER) AS version,
    event_id AS last_event_id
FROM latest_events
WHERE rn = 1 AND event_type != 'delete';

-- Similar views for gd_sections_at, gd_variables_at, gd_edges_at
```

### Time-Travel Engine Methods

```rust
// sdk/rust/src/graphdocs/engine.rs

impl GraphDocsEngine {
    /// Render document at a specific point in time
    pub async fn render_at(&self, doc_id: &str, event_id: i64) -> Result<String> {
        let result = self.render_at_full(doc_id, event_id).await?;
        Ok(result.markdown)
    }

    /// Render with full details at a specific event
    pub async fn render_at_full(
        &self,
        doc_id: &str,
        event_id: i64,
    ) -> Result<RenderedDocument> {
        let conn = self.pool.get_read_connection().await?;

        // 1. Load document at event_id
        let doc = self.load_document_at(&conn, doc_id, event_id).await?;

        // 2. Resolve inheritance chain at event_id
        let chain = self.resolve_inheritance_at(&conn, doc_id, event_id).await?;

        // 3. Collect sections at event_id
        let sections = self.collect_sections_at(&conn, &chain, event_id).await?;

        // 4. Collect variables at event_id
        let variables = self.collect_variables_at(&conn, &chain, event_id).await?;

        // 5. Render
        let (markdown, vars_used, missing) = self.render_sections(&sections, &variables);

        Ok(RenderedDocument {
            markdown,
            variables_used: vars_used,
            missing_variables: missing,
        })
    }

    /// Get current event ID
    pub async fn current_event_id(&self) -> Result<i64> {
        let conn = self.pool.get_read_connection().await?;
        conn.query_row(
            "SELECT COALESCE(MAX(event_id), 0) FROM gd_journal",
            [],
            |r| r.get(0),
        ).map_err(Into::into)
    }

    /// List recent events for a document
    pub async fn list_events(
        &self,
        doc_id: &str,
        limit: usize,
    ) -> Result<Vec<DocumentEvent>> {
        let conn = self.pool.get_read_connection().await?;

        let mut stmt = conn.prepare(r#"
            SELECT event_id, event_type, table_name, event_time
            FROM gd_journal
            WHERE record_id = ? OR record_id LIKE ?
            ORDER BY event_id DESC
            LIMIT ?
        "#)?;

        let events = stmt.query_map(
            params![doc_id, format!("{}/%", doc_id), limit as i64],
            |row| Ok(DocumentEvent {
                event_id: row.get(0)?,
                event_type: row.get(1)?,
                table_name: row.get(2)?,
                event_time: row.get(3)?,
            }),
        )?
        .collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    // === Private Time-Travel Methods ===

    async fn load_document_at(
        &self,
        conn: &DuckConnection,
        doc_id: &str,
        event_id: i64,
    ) -> Result<Document> {
        conn.query_row(r#"
            WITH doc_events AS (
                SELECT
                    new_data,
                    event_type,
                    ROW_NUMBER() OVER (ORDER BY event_id DESC) AS rn
                FROM gd_journal
                WHERE table_name = 'gd_documents'
                  AND record_id = ?
                  AND event_id <= ?
            )
            SELECT
                new_data->>'id',
                new_data->>'title',
                new_data->>'base_template'
            FROM doc_events
            WHERE rn = 1 AND event_type != 'delete'
        "#, params![doc_id, event_id], |row| Ok(Document {
            id: row.get(0)?,
            title: row.get(1)?,
            base_template: row.get(2)?,
        })).map_err(|_| anyhow::anyhow!(
            "Document '{}' not found at event {}",
            doc_id, event_id
        ))
    }

    async fn resolve_inheritance_at(
        &self,
        conn: &DuckConnection,
        doc_id: &str,
        event_id: i64,
    ) -> Result<Vec<String>> {
        let mut chain = vec![doc_id.to_string()];
        let mut current = doc_id.to_string();
        let mut depth = 0;

        loop {
            let base: Option<String> = conn.query_row(r#"
                WITH doc_events AS (
                    SELECT
                        new_data,
                        event_type,
                        ROW_NUMBER() OVER (ORDER BY event_id DESC) AS rn
                    FROM gd_journal
                    WHERE table_name = 'gd_documents'
                      AND record_id = ?
                      AND event_id <= ?
                )
                SELECT new_data->>'base_template'
                FROM doc_events
                WHERE rn = 1 AND event_type != 'delete'
            "#, params![&current, event_id], |r| r.get(0))
                .optional()?
                .flatten();

            match base {
                Some(base_id) if !base_id.is_empty() => {
                    if chain.contains(&base_id) {
                        return Err(anyhow::anyhow!("Circular inheritance"));
                    }
                    if depth >= 10 {
                        return Err(anyhow::anyhow!("Max depth exceeded"));
                    }
                    chain.push(base_id.clone());
                    current = base_id;
                    depth += 1;
                }
                _ => break,
            }
        }

        chain.reverse();
        Ok(chain)
    }

    async fn collect_sections_at(
        &self,
        conn: &DuckConnection,
        chain: &[String],
        event_id: i64,
    ) -> Result<Vec<ResolvedSection>> {
        let mut sections = HashMap::new();

        for doc_id in chain {
            let mut stmt = conn.prepare(r#"
                WITH section_events AS (
                    SELECT
                        new_data,
                        event_type,
                        ROW_NUMBER() OVER (
                            PARTITION BY record_id
                            ORDER BY event_id DESC
                        ) AS rn
                    FROM gd_journal
                    WHERE table_name = 'gd_sections'
                      AND new_data->>'document_id' = ?
                      AND event_id <= ?
                )
                SELECT
                    new_data->>'id',
                    new_data->>'section_type',
                    CAST(new_data->>'level' AS INTEGER),
                    CAST(new_data->>'order_idx' AS INTEGER),
                    new_data->>'content',
                    new_data->>'source_section'
                FROM section_events
                WHERE rn = 1 AND event_type != 'delete'
            "#)?;

            let rows = stmt.query_map(params![doc_id, event_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i32>>(2)?,
                    row.get::<_, i32>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
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

        let mut result: Vec<_> = sections.into_values().collect();
        result.sort_by_key(|s| s.order_idx);
        Ok(result)
    }

    async fn collect_variables_at(
        &self,
        conn: &DuckConnection,
        chain: &[String],
        event_id: i64,
    ) -> Result<HashMap<String, Value>> {
        let mut variables = HashMap::new();

        for doc_id in chain {
            let mut stmt = conn.prepare(r#"
                WITH var_events AS (
                    SELECT
                        new_data,
                        event_type,
                        ROW_NUMBER() OVER (
                            PARTITION BY record_id
                            ORDER BY event_id DESC
                        ) AS rn
                    FROM gd_journal
                    WHERE table_name = 'gd_variables'
                      AND new_data->>'document_id' = ?
                      AND event_id <= ?
                )
                SELECT
                    new_data->>'name',
                    new_data->>'value'
                FROM var_events
                WHERE rn = 1 AND event_type != 'delete'
            "#)?;

            let rows = stmt.query_map(params![doc_id, event_id], |row| {
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
}

#[derive(Debug)]
pub struct DocumentEvent {
    pub event_id: i64,
    pub event_type: String,
    pub table_name: String,
    pub event_time: chrono::DateTime<chrono::Utc>,
}
```

### CLI Integration

```rust
// cli/src/cmd/graphdocs.rs

#[derive(Args)]
pub struct RenderArgs {
    /// Document ID to render
    pub doc_id: String,

    /// Render at specific event ID (time-travel)
    #[clap(long)]
    pub at: Option<i64>,

    /// Output file (stdout if not specified)
    #[clap(long, short)]
    pub output: Option<PathBuf>,
}

pub async fn handle_render(fs: &DuckAgentFS, args: RenderArgs) -> Result<()> {
    let engine = GraphDocsEngine::new(fs.pool.clone());

    let markdown = match args.at {
        Some(event_id) => {
            println!("Rendering {} at event {}", args.doc_id, event_id);
            engine.render_at(&args.doc_id, event_id).await?
        }
        None => {
            engine.render(&args.doc_id).await?
        }
    };

    match args.output {
        Some(path) => {
            std::fs::write(&path, &markdown)?;
            println!("Written to {:?}", path);
        }
        None => {
            println!("{}", markdown);
        }
    }

    Ok(())
}

#[derive(Args)]
pub struct HistoryArgs {
    /// Document ID
    pub doc_id: String,

    /// Number of events to show
    #[clap(long, default_value = "20")]
    pub limit: usize,
}

pub async fn handle_history(fs: &DuckAgentFS, args: HistoryArgs) -> Result<()> {
    let engine = GraphDocsEngine::new(fs.pool.clone());
    let events = engine.list_events(&args.doc_id, args.limit).await?;

    println!("{:<10} {:<10} {:<15} {:<25}",
        "EVENT_ID", "TYPE", "TABLE", "TIME");
    println!("{}", "-".repeat(60));

    for event in events {
        println!("{:<10} {:<10} {:<15} {:<25}",
            event.event_id,
            event.event_type,
            event.table_name,
            event.event_time.format("%Y-%m-%d %H:%M:%S"),
        );
    }

    Ok(())
}
```

### CLI Usage

```bash
# Render current version
agentfs graphdocs render my-doc

# Render at specific event
agentfs graphdocs render my-doc --at 150

# Save to file
agentfs graphdocs render my-doc --at 150 --output old-version.md

# View document history
agentfs graphdocs history my-doc
agentfs graphdocs history my-doc --limit 50
```

### Output Example

```
$ agentfs graphdocs history my-doc

EVENT_ID   TYPE       TABLE           TIME
------------------------------------------------------------
200        update     gd_variables    2024-01-15 10:30:00
195        update     gd_sections     2024-01-15 10:25:00
190        create     gd_sections     2024-01-15 10:20:00
185        create     gd_variables    2024-01-15 10:15:00
180        create     gd_documents    2024-01-15 10:10:00
```

## Tests

### Test 1: Render at Past Event
```rust
#[tokio::test]
async fn test_render_at_past() {
    let pool = setup_test_pool().await;
    let engine = GraphDocsEngine::new(pool.clone());

    // Create document
    create_test_doc(&pool, "test", "Version 1").await;
    let event1 = engine.current_event_id().await.unwrap();

    // Update
    update_test_doc(&pool, "test", "Version 2").await;

    // Render at past event should show Version 1
    let md = engine.render_at("test", event1).await.unwrap();
    assert!(md.contains("Version 1"));
    assert!(!md.contains("Version 2"));

    // Render current should show Version 2
    let md_current = engine.render("test").await.unwrap();
    assert!(md_current.contains("Version 2"));
}
```

### Test 2: Variable History
```rust
#[tokio::test]
async fn test_variable_history() {
    let pool = setup_test_pool().await;
    let engine = GraphDocsEngine::new(pool.clone());

    // Create doc with variable
    create_doc_with_var(&pool, "test", "name", "Alice").await;
    let event1 = engine.current_event_id().await.unwrap();

    // Update variable
    engine.set_variable("test", "name", json!("Bob")).await.unwrap();

    // Render at past should use "Alice"
    let md_past = engine.render_at("test", event1).await.unwrap();
    assert!(md_past.contains("Alice"));

    // Render current should use "Bob"
    let md_current = engine.render("test").await.unwrap();
    assert!(md_current.contains("Bob"));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/engine.rs` | Engine with time-travel |
| `schema/duckagentfs.sql` | gd_journal table |
| `cli/src/cmd/graphdocs.rs` | CLI commands |
