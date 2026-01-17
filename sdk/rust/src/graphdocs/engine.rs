//! GraphDocs rendering engine
//!
//! Provides template-based Markdown generation from the GraphDocs graph structure
//! stored in DuckDB. Supports template inheritance, variable substitution,
//! section ordering, and time-travel rendering via an append-only journal.

use crate::error::{Error, Result};
use crate::filesystem::duckagentfs::DuckConnectionPool;
use chrono::{DateTime, Utc};
use duckdb::params;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

/// GraphDocs rendering engine.
///
/// Renders documents stored in the GraphDocs graph structure to Markdown,
/// handling template inheritance, variable substitution, and section ordering.
///
/// # Example
///
/// ```rust,ignore
/// use graphdocs::engine::GraphDocsEngine;
///
/// let engine = GraphDocsEngine::new(pool);
///
/// // Set variables
/// engine.set_variable("my-doc", "project_name", json!("My Project")).await?;
///
/// // Render
/// let markdown = engine.render("my-doc").await?;
/// ```
pub struct GraphDocsEngine {
    pool: DuckConnectionPool,
}

/// Rendered document result with metadata.
#[derive(Debug, Clone)]
pub struct RenderedDocument {
    /// The rendered Markdown content.
    pub markdown: String,
    /// List of variable names that were used during rendering.
    pub variables_used: Vec<String>,
    /// List of variable names referenced but not defined.
    pub missing_variables: Vec<String>,
}

/// Section ready for rendering (internal).
#[derive(Debug, Clone)]
struct ResolvedSection {
    section_type: String,
    level: Option<u8>,
    content: String,
    order_idx: i32,
    #[allow(dead_code)]
    is_inherited: bool,
}

/// Document metadata (internal).
#[derive(Debug)]
struct Document {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    title: String,
    #[allow(dead_code)]
    base_template: Option<String>,
}

/// A journal event representing a mutation to a GraphDocs record.
#[derive(Debug, Clone)]
pub struct DocumentEvent {
    /// Unique event identifier.
    pub event_id: i64,
    /// Type of event: 'create', 'update', or 'delete'.
    pub event_type: String,
    /// Table that was modified: 'gd_documents', 'gd_sections', 'gd_variables', 'gd_edges'.
    pub table_name: String,
    /// Timestamp when the event occurred.
    pub event_time: DateTime<Utc>,
}

impl GraphDocsEngine {
    /// Create a new GraphDocsEngine with the given connection pool.
    pub fn new(pool: DuckConnectionPool) -> Self {
        Self { pool }
    }

    /// Render a document to Markdown.
    ///
    /// # Arguments
    ///
    /// * `doc_id` - The document ID to render
    ///
    /// # Returns
    ///
    /// The rendered Markdown string.
    pub async fn render(&self, doc_id: &str) -> Result<String> {
        let result = self.render_full(doc_id).await?;
        Ok(result.markdown)
    }

    /// Render a document with full details.
    ///
    /// Returns the rendered Markdown along with metadata about which variables
    /// were used and which were missing.
    ///
    /// # Arguments
    ///
    /// * `doc_id` - The document ID to render
    ///
    /// # Returns
    ///
    /// A `RenderedDocument` containing the Markdown and variable usage info.
    pub async fn render_full(&self, doc_id: &str) -> Result<RenderedDocument> {
        let pool = self.pool.clone();
        let doc_id = doc_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            // 1. Load document metadata (validates it exists)
            let _doc = Self::load_document_sync(&conn, &doc_id)?;

            // 2. Resolve inheritance chain
            let inheritance_chain = Self::resolve_inheritance_sync(&conn, &doc_id)?;

            // 3. Collect all sections (with inheritance)
            let sections = Self::collect_sections_sync(&conn, &inheritance_chain)?;

            // 4. Collect all variables (with inheritance)
            let variables = Self::collect_variables_sync(&conn, &inheritance_chain)?;

            // 5. Render sections to Markdown
            let (markdown, vars_used, missing) = Self::render_sections(&sections, &variables);

            Ok(RenderedDocument {
                markdown,
                variables_used: vars_used,
                missing_variables: missing,
            })
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    /// Render a document at a specific event_id (time-travel).
    ///
    /// Returns the rendered Markdown as it existed at the given event.
    ///
    /// # Arguments
    ///
    /// * `doc_id` - The document ID to render
    /// * `event_id` - The event ID to render at (use `current_event_id()` for latest)
    pub async fn render_at(&self, doc_id: &str, event_id: i64) -> Result<String> {
        let result = self.render_at_full(doc_id, event_id).await?;
        Ok(result.markdown)
    }

    /// Render a document at a specific event_id with full details.
    ///
    /// Returns the rendered Markdown along with metadata about which variables
    /// were used and which were missing, all as of the specified event.
    ///
    /// # Arguments
    ///
    /// * `doc_id` - The document ID to render
    /// * `event_id` - The event ID to render at
    pub async fn render_at_full(
        &self,
        doc_id: &str,
        event_id: i64,
    ) -> Result<RenderedDocument> {
        let pool = self.pool.clone();
        let doc_id = doc_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            // 1. Load document at event_id
            let _doc = Self::load_document_at_sync(&conn, &doc_id, event_id)?;

            // 2. Resolve inheritance chain at event_id
            let inheritance_chain = Self::resolve_inheritance_at_sync(&conn, &doc_id, event_id)?;

            // 3. Collect sections at event_id
            let sections = Self::collect_sections_at_sync(&conn, &inheritance_chain, event_id)?;

            // 4. Collect variables at event_id
            let variables = Self::collect_variables_at_sync(&conn, &inheritance_chain, event_id)?;

            // 5. Render sections to Markdown
            let (markdown, vars_used, missing) = Self::render_sections(&sections, &variables);

            Ok(RenderedDocument {
                markdown,
                variables_used: vars_used,
                missing_variables: missing,
            })
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    /// Get the current (latest) event ID from the journal.
    ///
    /// Returns 0 if no events exist yet.
    pub async fn current_event_id(&self) -> Result<i64> {
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;
            let event_id: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(event_id), 0) FROM gd_journal",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| Error::Custom(format!("Failed to get current event_id: {}", e)))?;
            Ok(event_id)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    /// List recent events for a document.
    ///
    /// Returns events related to the document and its sections/variables,
    /// ordered by event_id descending (most recent first).
    ///
    /// # Arguments
    ///
    /// * `doc_id` - The document ID to get events for
    /// * `limit` - Maximum number of events to return
    pub async fn list_events(&self, doc_id: &str, limit: usize) -> Result<Vec<DocumentEvent>> {
        let pool = self.pool.clone();
        let doc_id = doc_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            // Query for events related to this document
            // - direct document events (record_id = doc_id)
            // - section/variable events where new_data->>'document_id' = doc_id
            let mut stmt = conn
                .prepare(
                    r#"
                SELECT event_id, event_type, table_name, event_time
                FROM gd_journal
                WHERE record_id = ?
                   OR new_data->>'document_id' = ?
                   OR old_data->>'document_id' = ?
                ORDER BY event_id DESC
                LIMIT ?
            "#,
                )
                .map_err(|e| Error::Custom(format!("Failed to prepare events query: {}", e)))?;

            let events = stmt
                .query_map(params![doc_id, doc_id, doc_id, limit as i64], |row| {
                    let event_id: i64 = row.get(0)?;
                    let event_type: String = row.get(1)?;
                    let table_name: String = row.get(2)?;
                    // DuckDB returns TIMESTAMP as a string, parse it
                    let event_time_str: String = row.get(3)?;
                    let event_time = chrono::NaiveDateTime::parse_from_str(
                        &event_time_str,
                        "%Y-%m-%d %H:%M:%S",
                    )
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(
                            &event_time_str,
                            "%Y-%m-%d %H:%M:%S%.f",
                        )
                    })
                    .map(|dt| dt.and_utc())
                    .unwrap_or_else(|_| Utc::now());

                    Ok(DocumentEvent {
                        event_id,
                        event_type,
                        table_name,
                        event_time,
                    })
                })
                .map_err(|e| Error::Custom(format!("Failed to query events: {}", e)))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Custom(format!("Failed to read event row: {}", e)))?;

            Ok(events)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    /// Get all variables for a document (including inherited).
    ///
    /// # Arguments
    ///
    /// * `doc_id` - The document ID
    ///
    /// # Returns
    ///
    /// A HashMap of variable names to their JSON values.
    pub async fn get_variables(&self, doc_id: &str) -> Result<HashMap<String, Value>> {
        let pool = self.pool.clone();
        let doc_id = doc_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;
            let chain = Self::resolve_inheritance_sync(&conn, &doc_id)?;
            Self::collect_variables_sync(&conn, &chain)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    /// Set a variable value for a document.
    ///
    /// Creates the variable if it doesn't exist, otherwise updates it.
    /// Records a journal entry for time-travel support.
    ///
    /// # Arguments
    ///
    /// * `doc_id` - The document ID
    /// * `name` - The variable name
    /// * `value` - The JSON value to set
    pub async fn set_variable(&self, doc_id: &str, name: &str, value: Value) -> Result<()> {
        let pool = self.pool.clone();
        let doc_id = doc_id.to_string();
        let name = name.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_write_connection()?;

            // Check if variable exists and get old data
            let existing: Option<(String, String, String)> = conn
                .query_row(
                    "SELECT id, value, var_type FROM gd_variables WHERE document_id = ? AND name = ?",
                    params![doc_id, name],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .ok();

            let var_type = match &value {
                Value::String(_) => "string",
                Value::Number(_) => "number",
                Value::Bool(_) => "boolean",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
                Value::Null => "string",
            };

            match existing {
                Some((id, old_value, old_var_type)) => {
                    // Build old_data JSON for journal
                    let old_data = serde_json::json!({
                        "id": id,
                        "document_id": doc_id,
                        "name": name,
                        "value": old_value,
                        "var_type": old_var_type
                    });

                    // Update existing
                    conn.execute(
                        "UPDATE gd_variables SET value = ?, var_type = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                        params![value.to_string(), var_type, id],
                    )
                    .map_err(|e| Error::Custom(format!("Failed to update variable: {}", e)))?;

                    // Build new_data JSON for journal
                    let new_data = serde_json::json!({
                        "id": id,
                        "document_id": doc_id,
                        "name": name,
                        "value": value.to_string(),
                        "var_type": var_type
                    });

                    // Record journal entry
                    Self::record_journal_entry_sync(
                        &conn,
                        "update",
                        "gd_variables",
                        &id,
                        Some(&old_data),
                        Some(&new_data),
                    )?;
                }
                None => {
                    // Insert new
                    let id = uuid::Uuid::new_v4().to_string();
                    conn.execute(
                        r#"INSERT INTO gd_variables (id, document_id, name, value, var_type)
                           VALUES (?, ?, ?, ?, ?)"#,
                        params![id, doc_id, name, value.to_string(), var_type],
                    )
                    .map_err(|e| Error::Custom(format!("Failed to insert variable: {}", e)))?;

                    // Build new_data JSON for journal
                    let new_data = serde_json::json!({
                        "id": id,
                        "document_id": doc_id,
                        "name": name,
                        "value": value.to_string(),
                        "var_type": var_type
                    });

                    // Record journal entry
                    Self::record_journal_entry_sync(
                        &conn,
                        "create",
                        "gd_variables",
                        &id,
                        None,
                        Some(&new_data),
                    )?;
                }
            }

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    // =========================================================================
    // Private synchronous methods (called inside spawn_blocking)
    // =========================================================================

    /// Load document metadata (synchronous).
    fn load_document_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        doc_id: &str,
    ) -> Result<Document> {
        conn.query_row(
            "SELECT id, title, base_template FROM gd_documents WHERE id = ?",
            [doc_id],
            |row| {
                Ok(Document {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    base_template: row.get(2)?,
                })
            },
        )
        .map_err(|_| Error::Custom(format!("Document not found: {}", doc_id)))
    }

    /// Resolve template inheritance chain (synchronous).
    ///
    /// Returns a vector of document IDs from base template to child,
    /// with the base template first and the target document last.
    fn resolve_inheritance_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        doc_id: &str,
    ) -> Result<Vec<String>> {
        let mut chain = vec![doc_id.to_string()];
        let mut current = doc_id.to_string();
        let mut depth = 0;
        const MAX_DEPTH: usize = 10;

        loop {
            let base: Option<String> = conn
                .query_row(
                    "SELECT base_template FROM gd_documents WHERE id = ?",
                    [&current],
                    |r| r.get(0),
                )
                .ok()
                .flatten();

            match base {
                Some(base_id) => {
                    if chain.contains(&base_id) {
                        return Err(Error::Custom(format!(
                            "Circular inheritance detected: {} -> {}",
                            current, base_id
                        )));
                    }
                    if depth >= MAX_DEPTH {
                        return Err(Error::Custom(format!(
                            "Inheritance depth exceeded maximum of {}",
                            MAX_DEPTH
                        )));
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

    /// Collect sections from all documents in inheritance chain (synchronous).
    ///
    /// Child sections override parent sections with the same source_section ID.
    fn collect_sections_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        chain: &[String],
    ) -> Result<Vec<ResolvedSection>> {
        let mut sections: HashMap<String, ResolvedSection> = HashMap::new();

        // Process from base to child (child overrides)
        for (chain_idx, doc_id) in chain.iter().enumerate() {
            let mut stmt = conn
                .prepare(
                    r#"SELECT id, section_type, level, order_idx, content, source_section
                       FROM gd_sections
                       WHERE document_id = ?
                       ORDER BY order_idx"#,
                )
                .map_err(|e| Error::Custom(format!("Failed to prepare sections query: {}", e)))?;

            let rows = stmt
                .query_map([doc_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,         // id
                        row.get::<_, String>(1)?,         // section_type
                        row.get::<_, Option<i32>>(2)?,    // level
                        row.get::<_, i32>(3)?,            // order_idx
                        row.get::<_, Option<String>>(4)?, // content
                        row.get::<_, Option<String>>(5)?, // source_section
                    ))
                })
                .map_err(|e| Error::Custom(format!("Failed to query sections: {}", e)))?;

            for row_result in rows {
                let (id, section_type, level, order_idx, content, source_section) = row_result
                    .map_err(|e| Error::Custom(format!("Failed to read section row: {}", e)))?;

                let key = source_section.unwrap_or_else(|| id.clone());
                let is_inherited = chain_idx < chain.len() - 1;

                sections.insert(
                    key,
                    ResolvedSection {
                        section_type,
                        level: level.map(|l| l as u8),
                        content: content.unwrap_or_default(),
                        order_idx,
                        is_inherited,
                    },
                );
            }
        }

        // Sort by order_idx
        let mut result: Vec<_> = sections.into_values().collect();
        result.sort_by_key(|s| s.order_idx);
        Ok(result)
    }

    /// Collect variables from all documents in inheritance chain (synchronous).
    ///
    /// Child variables override parent variables with the same name.
    fn collect_variables_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        chain: &[String],
    ) -> Result<HashMap<String, Value>> {
        let mut variables: HashMap<String, Value> = HashMap::new();

        // Process from base to child (child overrides)
        for doc_id in chain {
            let mut stmt = conn
                .prepare("SELECT name, value FROM gd_variables WHERE document_id = ?")
                .map_err(|e| Error::Custom(format!("Failed to prepare variables query: {}", e)))?;

            let rows = stmt
                .query_map([doc_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .map_err(|e| Error::Custom(format!("Failed to query variables: {}", e)))?;

            for row_result in rows {
                let (name, value_str) = row_result
                    .map_err(|e| Error::Custom(format!("Failed to read variable row: {}", e)))?;

                let value: Value = value_str
                    .as_ref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| value_str.map(Value::String).unwrap_or(Value::Null));

                variables.insert(name, value);
            }
        }

        Ok(variables)
    }

    /// Render sections to Markdown with variable substitution.
    fn render_sections(
        sections: &[ResolvedSection],
        variables: &HashMap<String, Value>,
    ) -> (String, Vec<String>, Vec<String>) {
        let mut output = String::new();
        let mut vars_used = Vec::new();
        let mut missing = Vec::new();

        for section in sections {
            let rendered = Self::render_section(section, variables, &mut vars_used, &mut missing);
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

    /// Render a single section with variable substitution.
    fn render_section(
        section: &ResolvedSection,
        variables: &HashMap<String, Value>,
        vars_used: &mut Vec<String>,
        missing: &mut Vec<String>,
    ) -> String {
        let mut content = section.content.clone();

        // Find and substitute variables: {{variable_name}}
        let re = Regex::new(r"\{\{(\w+)\}\}").expect("Invalid regex");
        for cap in re.captures_iter(&section.content) {
            let var_name = &cap[1];
            vars_used.push(var_name.to_string());

            if let Some(value) = variables.get(var_name) {
                let replacement = value_to_markdown_string(value);
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
            "blockquote" => content
                .lines()
                .map(|line| format!("> {}", line))
                .collect::<Vec<_>>()
                .join("\n"),
            "hr" => "---".to_string(),
            _ => content,
        }
    }

    // =========================================================================
    // Time-Travel Synchronous Methods (query from gd_journal)
    // =========================================================================

    /// Load document at a specific event_id from journal.
    fn load_document_at_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        doc_id: &str,
        event_id: i64,
    ) -> Result<Document> {
        // Query journal for the document state at or before event_id
        conn.query_row(
            r#"
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
            "#,
            params![doc_id, event_id],
            |row| {
                Ok(Document {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    base_template: row.get(2)?,
                })
            },
        )
        .map_err(|_| {
            Error::Custom(format!(
                "Document '{}' not found at event {}",
                doc_id, event_id
            ))
        })
    }

    /// Resolve inheritance chain at a specific event_id.
    fn resolve_inheritance_at_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        doc_id: &str,
        event_id: i64,
    ) -> Result<Vec<String>> {
        let mut chain = vec![doc_id.to_string()];
        let mut current = doc_id.to_string();
        let mut depth = 0;
        const MAX_DEPTH: usize = 10;

        loop {
            let base: Option<String> = conn
                .query_row(
                    r#"
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
                    "#,
                    params![&current, event_id],
                    |r| r.get(0),
                )
                .ok()
                .flatten();

            match base {
                Some(base_id) if !base_id.is_empty() => {
                    if chain.contains(&base_id) {
                        return Err(Error::Custom(format!(
                            "Circular inheritance detected: {} -> {}",
                            current, base_id
                        )));
                    }
                    if depth >= MAX_DEPTH {
                        return Err(Error::Custom(format!(
                            "Inheritance depth exceeded maximum of {}",
                            MAX_DEPTH
                        )));
                    }
                    chain.push(base_id.clone());
                    current = base_id;
                    depth += 1;
                }
                _ => break,
            }
        }

        // Reverse so base templates come first
        chain.reverse();
        Ok(chain)
    }

    /// Collect sections at a specific event_id from journal.
    fn collect_sections_at_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        chain: &[String],
        event_id: i64,
    ) -> Result<Vec<ResolvedSection>> {
        let mut sections: HashMap<String, ResolvedSection> = HashMap::new();

        for (chain_idx, doc_id) in chain.iter().enumerate() {
            // Query sections from journal at event_id
            let mut stmt = conn
                .prepare(
                    r#"
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
                    "#,
                )
                .map_err(|e| {
                    Error::Custom(format!("Failed to prepare sections_at query: {}", e))
                })?;

            let rows = stmt
                .query_map(params![doc_id, event_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i32>>(2)?,
                        row.get::<_, i32>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                })
                .map_err(|e| Error::Custom(format!("Failed to query sections_at: {}", e)))?;

            for row_result in rows {
                let (id, section_type, level, order_idx, content, source_section) = row_result
                    .map_err(|e| Error::Custom(format!("Failed to read section_at row: {}", e)))?;

                let key = source_section.unwrap_or_else(|| id.clone());
                let is_inherited = chain_idx < chain.len() - 1;

                sections.insert(
                    key,
                    ResolvedSection {
                        section_type,
                        level: level.map(|l| l as u8),
                        content: content.unwrap_or_default(),
                        order_idx,
                        is_inherited,
                    },
                );
            }
        }

        let mut result: Vec<_> = sections.into_values().collect();
        result.sort_by_key(|s| s.order_idx);
        Ok(result)
    }

    /// Collect variables at a specific event_id from journal.
    fn collect_variables_at_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        chain: &[String],
        event_id: i64,
    ) -> Result<HashMap<String, Value>> {
        let mut variables: HashMap<String, Value> = HashMap::new();

        for doc_id in chain {
            let mut stmt = conn
                .prepare(
                    r#"
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
                    "#,
                )
                .map_err(|e| {
                    Error::Custom(format!("Failed to prepare variables_at query: {}", e))
                })?;

            let rows = stmt
                .query_map(params![doc_id, event_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| Error::Custom(format!("Failed to query variables_at: {}", e)))?;

            for row_result in rows {
                let (name, value_str) = row_result
                    .map_err(|e| Error::Custom(format!("Failed to read variable_at row: {}", e)))?;

                let value: Value = serde_json::from_str(&value_str)
                    .unwrap_or_else(|_| Value::String(value_str));
                variables.insert(name, value);
            }
        }

        Ok(variables)
    }

    // =========================================================================
    // Journaling Helper Methods
    // =========================================================================

    /// Record a journal entry for a mutation.
    ///
    /// This is called internally when documents, sections, or variables are modified.
    fn record_journal_entry_sync(
        conn: &std::sync::MutexGuard<'_, duckdb::Connection>,
        event_type: &str,
        table_name: &str,
        record_id: &str,
        old_data: Option<&Value>,
        new_data: Option<&Value>,
    ) -> Result<i64> {
        let old_json = old_data.map(|v| v.to_string());
        let new_json = new_data.map(|v| v.to_string());

        conn.execute(
            r#"
            INSERT INTO gd_journal (event_type, table_name, record_id, old_data, new_data)
            VALUES (?, ?, ?, ?, ?)
            "#,
            params![event_type, table_name, record_id, old_json, new_json],
        )
        .map_err(|e| Error::Custom(format!("Failed to record journal entry: {}", e)))?;

        // Get the event_id that was just inserted
        let event_id: i64 = conn
            .query_row("SELECT currval('gd_event_seq')", [], |r| r.get(0))
            .map_err(|e| Error::Custom(format!("Failed to get journal event_id: {}", e)))?;

        Ok(event_id)
    }
}

/// Convert a JSON value to a Markdown-friendly string.
fn value_to_markdown_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(arr) => arr
            .iter()
            .map(|v| format!("- {}", value_to_simple_string(v)))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => value.to_string(),
        Value::Null => String::new(),
    }
}

/// Convert a JSON value to a simple string (for list items).
fn value_to_simple_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::duckagentfs::DuckConnectionPool;

    /// Create engine with schema for testing.
    async fn create_test_engine() -> GraphDocsEngine {
        // Create pool and load schema manually
        let pool = DuckConnectionPool::new(":memory:").expect("Failed to create pool");
        {
            let conn = pool
                .get_write_connection()
                .expect("Failed to get connection");
            conn.execute_batch(include_str!("../../../../schema/duckagentfs.sql"))
                .expect("Failed to load schema");
        }
        GraphDocsEngine::new(pool)
    }

    #[tokio::test]
    async fn test_simple_render() {
        let engine = create_test_engine().await;

        // Setup test data
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test Document')",
                [],
            )
            .expect("Failed to insert document");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'test', 'heading', 1, 0, 'Hello World')",
                [],
            ).expect("Failed to insert section");
        }

        // Render
        let md = engine.render("test").await.expect("Failed to render");
        assert!(
            md.contains("# Hello World"),
            "Expected heading, got: {}",
            md
        );
    }

    #[tokio::test]
    async fn test_variable_substitution() {
        let engine = create_test_engine().await;

        // Setup test data with variable placeholder
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            )
            .expect("Failed to insert document");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'test', 'paragraph', NULL, 0, 'Hello {{name}}!')",
                [],
            ).expect("Failed to insert section");
        }

        // Set variable
        engine
            .set_variable("test", "name", serde_json::json!("World"))
            .await
            .expect("Failed to set variable");

        // Render
        let result = engine.render_full("test").await.expect("Failed to render");
        assert!(
            result.markdown.contains("Hello World!"),
            "Expected substitution, got: {}",
            result.markdown
        );
        assert!(result.variables_used.contains(&"name".to_string()));
        assert!(result.missing_variables.is_empty());
    }

    #[tokio::test]
    async fn test_missing_variable_detection() {
        let engine = create_test_engine().await;

        // Setup test data with variable placeholder but no variable defined
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            )
            .expect("Failed to insert document");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'test', 'paragraph', NULL, 0, 'Hello {{missing}}!')",
                [],
            ).expect("Failed to insert section");
        }

        // Render without setting the variable
        let result = engine.render_full("test").await.expect("Failed to render");
        assert!(result.missing_variables.contains(&"missing".to_string()));
        // The placeholder should remain in output
        assert!(
            result.markdown.contains("{{missing}}"),
            "Expected placeholder to remain, got: {}",
            result.markdown
        );
    }

    #[tokio::test]
    async fn test_template_inheritance() {
        let engine = create_test_engine().await;

        // Setup base template and child document
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");

            // Base template
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('base', 'Base Template')",
                [],
            )
            .expect("Failed to insert base document");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('base-s1', 'base', 'heading', 1, 0, 'Header')",
                [],
            ).expect("Failed to insert base section");
            conn.execute(
                "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES ('base-v1', 'base', 'version', '\"1.0\"', 'string')",
                [],
            ).expect("Failed to insert base variable");

            // Child document inheriting from base
            conn.execute(
                "INSERT INTO gd_documents (id, title, base_template) VALUES ('child', 'Child Doc', 'base')",
                [],
            ).expect("Failed to insert child document");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('child-s1', 'child', 'paragraph', NULL, 1, 'Version: {{version}}')",
                [],
            ).expect("Failed to insert child section");
        }

        // Render child - should include inherited variable
        let result = engine.render_full("child").await.expect("Failed to render");
        assert!(
            result.markdown.contains("# Header"),
            "Expected inherited header"
        );
        assert!(
            result.markdown.contains("Version: 1.0"),
            "Expected inherited variable substitution"
        );
    }

    #[tokio::test]
    async fn test_variable_override() {
        let engine = create_test_engine().await;

        // Setup base and child with same variable name
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");

            // Base template
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('base', 'Base')",
                [],
            )
            .expect("Failed to insert base document");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'base', 'paragraph', NULL, 0, '{{greeting}}')",
                [],
            ).expect("Failed to insert section");
            conn.execute(
                "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES ('v1', 'base', 'greeting', '\"Hello from base\"', 'string')",
                [],
            ).expect("Failed to insert base variable");

            // Child overrides variable
            conn.execute(
                "INSERT INTO gd_documents (id, title, base_template) VALUES ('child', 'Child', 'base')",
                [],
            ).expect("Failed to insert child document");
            conn.execute(
                "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES ('v2', 'child', 'greeting', '\"Hello from child\"', 'string')",
                [],
            ).expect("Failed to insert child variable");
        }

        // Render child - should use child's variable value
        let result = engine.render_full("child").await.expect("Failed to render");
        assert!(
            result.markdown.contains("Hello from child"),
            "Expected child variable to override, got: {}",
            result.markdown
        );
        assert!(!result.markdown.contains("Hello from base"));
    }

    #[tokio::test]
    async fn test_section_ordering() {
        let engine = create_test_engine().await;

        // Setup sections with different order_idx values
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            )
            .expect("Failed to insert document");
            // Insert out of order
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s3', 'test', 'paragraph', NULL, 20, 'Third')",
                [],
            ).expect("Failed to insert section 3");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'test', 'paragraph', NULL, 0, 'First')",
                [],
            ).expect("Failed to insert section 1");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s2', 'test', 'paragraph', NULL, 10, 'Second')",
                [],
            ).expect("Failed to insert section 2");
        }

        // Render - sections should be in order
        let md = engine.render("test").await.expect("Failed to render");
        let first_pos = md.find("First").expect("First not found");
        let second_pos = md.find("Second").expect("Second not found");
        let third_pos = md.find("Third").expect("Third not found");
        assert!(first_pos < second_pos, "First should come before Second");
        assert!(second_pos < third_pos, "Second should come before Third");
    }

    #[tokio::test]
    async fn test_circular_inheritance_detection() {
        // For this test, we create a pool and load a modified schema without FK constraint
        let pool = DuckConnectionPool::new(":memory:").expect("Failed to create pool");
        {
            let conn = pool
                .get_write_connection()
                .expect("Failed to get connection");
            // Create gd_documents without FK constraint for testing cycles
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS gd_documents (
                    id              VARCHAR PRIMARY KEY,
                    inode           UBIGINT UNIQUE,
                    title           VARCHAR NOT NULL,
                    base_template   VARCHAR,
                    language        VARCHAR DEFAULT 'en',
                    version         UINTEGER DEFAULT 1,
                    created_at      TIMESTAMP DEFAULT current_timestamp,
                    updated_at      TIMESTAMP DEFAULT current_timestamp,
                    metadata        JSON
                );
                "#,
            )
            .expect("Failed to create test schema");

            // Insert documents with cycle: A -> B -> A
            conn.execute(
                "INSERT INTO gd_documents (id, title, base_template) VALUES ('doc-b', 'B', 'doc-a')",
                [],
            ).expect("Failed to insert doc B");
            conn.execute(
                "INSERT INTO gd_documents (id, title, base_template) VALUES ('doc-a', 'A', 'doc-b')",
                [],
            ).expect("Failed to insert doc A");
        }

        let engine = GraphDocsEngine::new(pool);

        // Render should fail with circular inheritance error
        let result = engine.render("doc-a").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Circular inheritance"),
            "Expected circular inheritance error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_document_not_found() {
        let engine = create_test_engine().await;

        // Try to render non-existent document
        let result = engine.render("nonexistent").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "Expected not found error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_get_variables() {
        let engine = create_test_engine().await;

        // Setup document with variables
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            )
            .expect("Failed to insert document");
            conn.execute(
                "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES ('v1', 'test', 'name', '\"Alice\"', 'string')",
                [],
            ).expect("Failed to insert variable");
            conn.execute(
                "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES ('v2', 'test', 'count', '42', 'number')",
                [],
            ).expect("Failed to insert variable");
        }

        // Get variables
        let vars = engine
            .get_variables("test")
            .await
            .expect("Failed to get variables");
        assert_eq!(vars.get("name"), Some(&serde_json::json!("Alice")));
        assert_eq!(vars.get("count"), Some(&serde_json::json!(42)));
    }

    #[tokio::test]
    async fn test_section_types() {
        let engine = create_test_engine().await;

        // Setup document with various section types
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            )
            .expect("Failed to insert document");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'test', 'heading', 2, 0, 'Subheading')",
                [],
            ).expect("Failed to insert heading");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s2', 'test', 'code', NULL, 1, 'let x = 1;')",
                [],
            ).expect("Failed to insert code");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s3', 'test', 'blockquote', NULL, 2, 'A quote')",
                [],
            ).expect("Failed to insert blockquote");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s4', 'test', 'hr', NULL, 3, '')",
                [],
            ).expect("Failed to insert hr");
        }

        // Render
        let md = engine.render("test").await.expect("Failed to render");
        assert!(md.contains("## Subheading"), "Expected level 2 heading");
        assert!(md.contains("```\nlet x = 1;\n```"), "Expected code block");
        assert!(md.contains("> A quote"), "Expected blockquote");
        assert!(md.contains("---"), "Expected horizontal rule");
    }

    #[tokio::test]
    async fn test_array_variable() {
        let engine = create_test_engine().await;

        // Setup document with array variable
        {
            let conn = engine
                .pool
                .get_write_connection()
                .expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            )
            .expect("Failed to insert document");
            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'test', 'paragraph', NULL, 0, '{{items}}')",
                [],
            ).expect("Failed to insert section");
        }

        // Set array variable
        engine
            .set_variable(
                "test",
                "items",
                serde_json::json!(["Apple", "Banana", "Cherry"]),
            )
            .await
            .expect("Failed to set variable");

        // Render
        let md = engine.render("test").await.expect("Failed to render");
        assert!(md.contains("- Apple"), "Expected list item Apple");
        assert!(md.contains("- Banana"), "Expected list item Banana");
        assert!(md.contains("- Cherry"), "Expected list item Cherry");
    }

    // =========================================================================
    // Time-Travel Tests
    // =========================================================================

    #[tokio::test]
    async fn test_current_event_id_empty() {
        let engine = create_test_engine().await;

        // With no journal entries, should return 0
        let event_id = engine.current_event_id().await.expect("Failed to get event_id");
        assert_eq!(event_id, 0);
    }

    #[tokio::test]
    async fn test_set_variable_creates_journal_entry() {
        let engine = create_test_engine().await;

        // Setup document
        {
            let conn = engine.pool.get_write_connection().expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            ).expect("Failed to insert document");
        }

        // Initial event_id should be 0
        let initial_event_id = engine.current_event_id().await.expect("Failed to get event_id");
        assert_eq!(initial_event_id, 0);

        // Set a variable - should create journal entry
        engine
            .set_variable("test", "name", serde_json::json!("Alice"))
            .await
            .expect("Failed to set variable");

        // event_id should have increased
        let event_id_after = engine.current_event_id().await.expect("Failed to get event_id");
        assert!(event_id_after > initial_event_id, "Event ID should increase after set_variable");
    }

    #[tokio::test]
    async fn test_render_at_returns_historical_state() {
        let engine = create_test_engine().await;

        // Setup document with section and variable
        {
            let conn = engine.pool.get_write_connection().expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            ).expect("Failed to insert document");

            // Record document creation in journal
            conn.execute(
                r#"INSERT INTO gd_journal (event_type, table_name, record_id, new_data)
                   VALUES ('create', 'gd_documents', 'test', '{"id":"test","title":"Test"}')"#,
                [],
            ).expect("Failed to record document journal");

            conn.execute(
                "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'test', 'paragraph', NULL, 0, 'Hello {{name}}!')",
                [],
            ).expect("Failed to insert section");

            // Record section creation in journal
            conn.execute(
                r#"INSERT INTO gd_journal (event_type, table_name, record_id, new_data)
                   VALUES ('create', 'gd_sections', 's1', '{"id":"s1","document_id":"test","section_type":"paragraph","level":null,"order_idx":0,"content":"Hello {{name}}!"}')"#,
                [],
            ).expect("Failed to record section journal");
        }

        // Get event_id after initial setup
        let event_after_setup = engine.current_event_id().await.expect("Failed to get event_id");

        // Set variable to "Alice"
        engine
            .set_variable("test", "name", serde_json::json!("Alice"))
            .await
            .expect("Failed to set variable to Alice");

        let event_after_alice = engine.current_event_id().await.expect("Failed to get event_id");

        // Set variable to "Bob"
        engine
            .set_variable("test", "name", serde_json::json!("Bob"))
            .await
            .expect("Failed to set variable to Bob");

        let event_after_bob = engine.current_event_id().await.expect("Failed to get event_id");

        // Current render should show "Bob"
        let current_md = engine.render("test").await.expect("Failed to render current");
        assert!(
            current_md.contains("Hello Bob!"),
            "Current render should show Bob, got: {}",
            current_md
        );

        // Render at event_after_alice should show "Alice"
        let alice_md = engine
            .render_at("test", event_after_alice)
            .await
            .expect("Failed to render at Alice event");
        assert!(
            alice_md.contains("Hello Alice!"),
            "Render at Alice event should show Alice, got: {}",
            alice_md
        );

        // Render at event_after_bob should show "Bob"
        let bob_md = engine
            .render_at("test", event_after_bob)
            .await
            .expect("Failed to render at Bob event");
        assert!(
            bob_md.contains("Hello Bob!"),
            "Render at Bob event should show Bob, got: {}",
            bob_md
        );
    }

    #[tokio::test]
    async fn test_list_events_returns_document_events() {
        let engine = create_test_engine().await;

        // Setup document
        {
            let conn = engine.pool.get_write_connection().expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            ).expect("Failed to insert document");
        }

        // Initially no events
        let initial_events = engine.list_events("test", 10).await.expect("Failed to list events");
        assert!(initial_events.is_empty(), "Should have no events initially");

        // Set some variables
        engine.set_variable("test", "var1", serde_json::json!("value1")).await.unwrap();
        engine.set_variable("test", "var2", serde_json::json!("value2")).await.unwrap();
        engine.set_variable("test", "var1", serde_json::json!("updated")).await.unwrap();

        // List events
        let events = engine.list_events("test", 10).await.expect("Failed to list events");
        assert_eq!(events.len(), 3, "Should have 3 events");

        // Events should be in descending order (most recent first)
        assert!(
            events[0].event_id > events[1].event_id,
            "Events should be in descending order"
        );

        // Check event types
        assert_eq!(events[0].event_type, "update"); // var1 updated
        assert_eq!(events[1].event_type, "create"); // var2 created
        assert_eq!(events[2].event_type, "create"); // var1 created
    }

    #[tokio::test]
    async fn test_list_events_respects_limit() {
        let engine = create_test_engine().await;

        // Setup document
        {
            let conn = engine.pool.get_write_connection().expect("Failed to get connection");
            conn.execute(
                "INSERT INTO gd_documents (id, title) VALUES ('test', 'Test')",
                [],
            ).expect("Failed to insert document");
        }

        // Create many events
        for i in 0..10 {
            engine
                .set_variable("test", &format!("var{}", i), serde_json::json!(i))
                .await
                .unwrap();
        }

        // List with limit of 5
        let events = engine.list_events("test", 5).await.expect("Failed to list events");
        assert_eq!(events.len(), 5, "Should respect limit");
    }
}
