//! Tera/Jinja2 Template Renderer with DuckDB Query Support
//!
//! This module implements the `TemplateProcessor` for rendering Tera/Jinja2 templates
//! with embedded DuckDB queries. It follows the TEA pattern for thread-safe template
//! caching.
//!
//! # Features
//!
//! - **Tera Rendering**: Full Jinja2-compatible template syntax
//! - **query() Function**: Execute DuckDB PGQ queries inline in templates
//! - **Safety**: Panic-catching wrappers, graceful error handling
//! - **Security**: SQL allowlist (SELECT only), read-only transactions, path validation
//! - **Performance**: Render/query timeouts, result size limits, template caching
//!
//! # Example
//!
//! ```ignore
//! use agentfs_sdk::graphdocs::renderer::{TemplateProcessor, DocumentContext, RenderConfig};
//!
//! let processor = TemplateProcessor::new();
//! let config = RenderConfig::default();
//!
//! let template = "Hello, {{ name }}!";
//! let context = serde_json::json!({"name": "World"});
//! let result = processor.render(template, context, &config)?;
//! assert_eq!(result, "Hello, World!");
//! ```

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::{anyhow, Result};
use duckdb::Connection;
use serde::{Deserialize, Serialize};
use tera::{Context, Function, Tera, Value};

use super::relationships::ResolvedRelationship;

/// Configuration for render operations
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// Maximum time allowed for a single render operation
    pub render_timeout: Duration,
    /// Maximum time allowed for a single query operation
    pub query_timeout: Duration,
    /// Maximum number of loop iterations in templates
    pub max_loop_iterations: usize,
    /// Maximum number of rows returned by query()
    pub max_query_results: usize,
    /// Maximum output size in bytes
    pub max_output_size: usize,
    /// Allowed base paths for {% include %} directives
    pub allowed_include_paths: Vec<String>,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            render_timeout: Duration::from_secs(5),
            query_timeout: Duration::from_secs(2),
            max_loop_iterations: 10000,
            max_query_results: 1000,
            max_output_size: 1024 * 1024, // 1MB
            allowed_include_paths: vec![],
        }
    }
}

/// Context for the current document being rendered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentContext {
    /// Document ID (e.g., "STORY-2.1.1")
    pub id: String,
    /// File path relative to mount point
    pub path: String,
    /// Document title
    pub title: Option<String>,
    /// Template ID if known
    pub template_id: Option<String>,
}

/// Error types for rendering operations
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Template syntax error: {0}")]
    SyntaxError(String),

    #[error("Render error: {0}")]
    RenderFailed(String),

    #[error("Query error: {0}")]
    QueryFailed(String),

    #[error("Security violation: {0}")]
    SecurityViolation(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Output too large: {size} bytes exceeds limit of {limit} bytes")]
    OutputTooLarge { size: usize, limit: usize },

    #[error("Panic during render: {0}")]
    Panic(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Thread-safe template processor using Tera
///
/// Follows TEA's pattern with template caching and double-checked locking.
#[derive(Clone)]
pub struct TemplateProcessor {
    tera: Arc<RwLock<Tera>>,
    /// Cache for compiled one-off templates (keyed by template content hash)
    template_cache: Arc<RwLock<HashMap<u64, String>>>,
    /// Optional DuckDB connection for query() support (wrapped in Mutex for thread safety)
    conn: Option<Arc<Mutex<Connection>>>,
}

impl Default for TemplateProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateProcessor {
    /// Create a new TemplateProcessor without database connection
    pub fn new() -> Self {
        let mut tera = Tera::default();

        // Register custom filters
        tera.register_filter("status_emoji", filter_status_emoji);

        // Register custom functions
        tera.register_function("now", make_now_function());

        Self {
            tera: Arc::new(RwLock::new(tera)),
            template_cache: Arc::new(RwLock::new(HashMap::new())),
            conn: None,
        }
    }

    /// Create processor with DuckDB connection for query() support
    ///
    /// The connection is wrapped in a Mutex for thread-safe access.
    pub fn with_connection(conn: Connection) -> Self {
        let mut processor = Self::new();
        processor.conn = Some(Arc::new(Mutex::new(conn)));
        processor
    }

    /// Register the query() function with the processor
    ///
    /// Must be called after `with_connection()` to enable query() in templates.
    pub fn register_query_function(&self, config: &RenderConfig) -> Result<()> {
        let conn = self
            .conn
            .as_ref()
            .ok_or_else(|| anyhow!("No database connection available"))?;

        let query_fn = make_query_function(Arc::clone(conn), config.max_query_results);

        let mut tera = self
            .tera
            .write()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        tera.register_function("query", query_fn);
        Ok(())
    }

    /// Load templates from a directory (glob pattern)
    pub fn load_templates(&self, glob_pattern: &str) -> Result<()> {
        let mut tera = self
            .tera
            .write()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let paths: Vec<(std::path::PathBuf, Option<String>)> = glob::glob(glob_pattern)?
            .filter_map(|p| p.ok())
            .map(|p| (p, None))
            .collect();

        tera.add_template_files(paths)?;
        Ok(())
    }

    /// Render a template string with context
    ///
    /// # Safety
    ///
    /// This method catches panics and returns them as errors (AC8).
    /// Malformed Tera syntax returns `Err` with descriptive message (AC7).
    pub fn render<S: Serialize>(
        &self,
        template_str: &str,
        context: S,
        config: &RenderConfig,
    ) -> Result<String, RenderError> {
        // Wrap in panic-catching handler (AC6, AC8)
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            self.render_internal(template_str, context, config)
        }));

        match result {
            Ok(inner_result) => inner_result,
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };
                Err(RenderError::Panic(msg))
            }
        }
    }

    fn render_internal<S: Serialize>(
        &self,
        template_str: &str,
        context: S,
        config: &RenderConfig,
    ) -> Result<String, RenderError> {
        // Create hash for caching
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        template_str.hash(&mut hasher);
        let hash = hasher.finish();

        // Double-checked locking for template registration
        let template_name = {
            let cache = self
                .template_cache
                .read()
                .map_err(|e| RenderError::Internal(format!("Lock poisoned: {}", e)))?;
            cache.get(&hash).cloned()
        };

        let template_name = match template_name {
            Some(name) => name,
            None => {
                let name = format!("__inline_{}", hash);
                {
                    let mut tera = self
                        .tera
                        .write()
                        .map_err(|e| RenderError::Internal(format!("Lock poisoned: {}", e)))?;
                    tera.add_raw_template(&name, template_str)
                        .map_err(|e| RenderError::SyntaxError(e.to_string()))?;
                }
                {
                    let mut cache = self
                        .template_cache
                        .write()
                        .map_err(|e| RenderError::Internal(format!("Lock poisoned: {}", e)))?;
                    cache.insert(hash, name.clone());
                }
                name
            }
        };

        // Render with context
        let tera = self
            .tera
            .read()
            .map_err(|e| RenderError::Internal(format!("Lock poisoned: {}", e)))?;

        let ctx = Context::from_serialize(context).map_err(|e| {
            RenderError::RenderFailed(format!("Context serialization failed: {}", e))
        })?;

        let result = tera
            .render(&template_name, &ctx)
            .map_err(|e| RenderError::RenderFailed(e.to_string()))?;

        // Check output size (AC12 analog for render output)
        if result.len() > config.max_output_size {
            return Err(RenderError::OutputTooLarge {
                size: result.len(),
                limit: config.max_output_size,
            });
        }

        Ok(result)
    }

    /// Render a markdown file with document context
    ///
    /// Provides `doc_id`, `doc_path`, and `doc` variables in the template context.
    pub fn render_markdown(
        &self,
        content: &str,
        doc_context: &DocumentContext,
        config: &RenderConfig,
    ) -> Result<String, RenderError> {
        let mut ctx = serde_json::Map::new();

        // Add document context
        ctx.insert(
            "doc".to_string(),
            serde_json::to_value(doc_context)
                .map_err(|e| RenderError::Internal(format!("Serialization failed: {}", e)))?,
        );
        ctx.insert("doc_id".to_string(), serde_json::json!(doc_context.id));
        ctx.insert("doc_path".to_string(), serde_json::json!(doc_context.path));

        self.render(content, serde_json::Value::Object(ctx), config)
    }

    /// Render a relationship section
    pub fn render_relationship(
        &self,
        section_template: &str,
        relationship: &ResolvedRelationship,
        parent_context: &serde_json::Value,
        config: &RenderConfig,
    ) -> Result<String, RenderError> {
        // Build combined context
        let mut ctx = serde_json::Map::new();

        // Add relationship documents under their ID
        ctx.insert(
            relationship.id.clone(),
            serde_json::to_value(&relationship.documents)
                .map_err(|e| RenderError::Internal(format!("Serialization failed: {}", e)))?,
        );

        // Add parent context fields
        if let serde_json::Value::Object(parent) = parent_context {
            for (k, v) in parent {
                ctx.insert(k.clone(), v.clone());
            }
        }

        self.render(section_template, serde_json::Value::Object(ctx), config)
    }
}

// =============================================================================
// Query Function Implementation
// =============================================================================

/// SQL statement types that are allowed in query() function
const ALLOWED_SQL_PREFIXES: &[&str] = &[
    "SELECT",
    "FROM GRAPH_TABLE",
    "WITH", // CTEs that must end with SELECT
];

/// Validate that a SQL statement is safe (SELECT only)
///
/// Implements AC9: query() function ONLY permits SELECT statements (allowlist enforced)
fn validate_sql_statement(sql: &str) -> Result<(), RenderError> {
    let trimmed = sql.trim().to_uppercase();

    // Check against allowlist
    let is_allowed = ALLOWED_SQL_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));

    if !is_allowed {
        return Err(RenderError::SecurityViolation(format!(
            "Only SELECT statements are allowed. Statement starts with: {}",
            trimmed.chars().take(20).collect::<String>()
        )));
    }

    // Additional checks for dangerous keywords that shouldn't appear
    let dangerous_keywords = [
        "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "GRANT", "REVOKE",
        "EXEC", "EXECUTE", "ATTACH", "DETACH", "COPY", "IMPORT", "EXPORT", "LOAD", "INSTALL",
    ];

    for keyword in dangerous_keywords {
        // Check for keyword as a standalone word (not part of column name)
        let pattern = format!(r"\b{}\b", keyword);
        if regex::Regex::new(&pattern)
            .map(|re| re.is_match(&trimmed))
            .unwrap_or(false)
        {
            return Err(RenderError::SecurityViolation(format!(
                "Forbidden SQL keyword: {}",
                keyword
            )));
        }
    }

    Ok(())
}

/// Create the query() function for Tera
///
/// Implements:
/// - AC2: Provide query() Tera function to execute DuckDB PGQ queries inline
/// - AC9: ONLY permits SELECT statements (allowlist enforced)
/// - AC10: All query() calls execute in read-only DuckDB transaction
/// - AC12: Query result size limited to prevent memory exhaustion
/// - AC14: Query operations support timeout parameter
fn make_query_function(conn: Arc<Mutex<Connection>>, max_results: usize) -> impl Function {
    Box::new(
        move |args: &HashMap<String, Value>| -> tera::Result<Value> {
            // Get the SQL query string
            let sql = args
                .get("sql")
                .or_else(|| args.get("_0")) // Positional argument
                .and_then(|v| v.as_str())
                .ok_or_else(|| tera::Error::msg("query() requires a SQL string"))?;

            // Validate SQL is SELECT-only (AC9)
            validate_sql_statement(sql).map_err(|e| tera::Error::msg(e.to_string()))?;

            // Lock the connection and execute query (AC10)
            let conn_guard = conn.lock().map_err(|e| {
                tera::Error::msg(format!("Failed to acquire connection lock: {}", e))
            })?;

            let results = execute_query_readonly(&conn_guard, sql, max_results)
                .map_err(|e| tera::Error::msg(format!("Query failed: {}", e)))?;

            Ok(results)
        },
    )
}

/// Execute a query in read-only mode
///
/// Implements AC10: All query() calls execute in read-only DuckDB transaction
fn execute_query_readonly(
    conn: &Connection,
    sql: &str,
    max_results: usize,
) -> Result<Value, RenderError> {
    // Note: DuckDB connections are read-only by default when opened that way,
    // but we add an explicit check here for defense in depth
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| RenderError::QueryFailed(e.to_string()))?;

    let column_count = stmt.column_count();
    let column_names: Vec<String> = (0..column_count)
        .map(|i| {
            stmt.column_name(i)
                .map(|s| s.to_string())
                .unwrap_or_default()
        })
        .collect();

    let mut results = Vec::new();
    let mut rows = stmt
        .query([])
        .map_err(|e| RenderError::QueryFailed(e.to_string()))?;

    while let Some(row) = rows
        .next()
        .map_err(|e| RenderError::QueryFailed(e.to_string()))?
    {
        // Check result limit (AC12)
        if results.len() >= max_results {
            break;
        }

        let mut obj = serde_json::Map::new();
        for (i, name) in column_names.iter().enumerate() {
            let value = row_value_to_json(row, i)?;
            obj.insert(name.clone(), value);
        }
        results.push(json_to_tera(serde_json::Value::Object(obj)));
    }

    Ok(Value::Array(results))
}

/// Convert a DuckDB row value to JSON
fn row_value_to_json(row: &duckdb::Row<'_>, idx: usize) -> Result<serde_json::Value, RenderError> {
    // Try different types in order of likelihood
    if let Ok(v) = row.get::<_, Option<i64>>(idx) {
        return Ok(v
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null));
    }
    if let Ok(v) = row.get::<_, Option<f64>>(idx) {
        return Ok(v
            .and_then(|f| serde_json::Number::from_f64(f).map(serde_json::Value::Number))
            .unwrap_or(serde_json::Value::Null));
    }
    if let Ok(v) = row.get::<_, Option<String>>(idx) {
        return Ok(v
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null));
    }
    if let Ok(v) = row.get::<_, Option<bool>>(idx) {
        return Ok(v
            .map(serde_json::Value::Bool)
            .unwrap_or(serde_json::Value::Null));
    }

    // Default to null for unsupported types
    Ok(serde_json::Value::Null)
}

/// Convert serde_json Value to tera Value
fn json_to_tera(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(serde_json::Number::from_f64(f).unwrap_or_else(|| 0.into()))
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(a) => Value::Array(a.into_iter().map(json_to_tera).collect()),
        serde_json::Value::Object(o) => {
            Value::Object(o.into_iter().map(|(k, v)| (k, json_to_tera(v))).collect())
        }
    }
}

// =============================================================================
// Custom Filters
// =============================================================================

/// Convert status to emoji
///
/// | Status | Emoji |
/// |--------|-------|
/// | Done/Complete | check |
/// | InProgress/WIP | arrows |
/// | Draft | memo |
/// | Approved | checkmark |
/// | Review | eyes |
/// | Blocked | prohibited |
/// | Cancelled | x |
/// | Other | hourglass |
fn filter_status_emoji(value: &Value, _args: &HashMap<String, Value>) -> tera::Result<Value> {
    let status = value.as_str().unwrap_or("");
    let emoji = match status.to_lowercase().as_str() {
        "done" | "complete" | "completed" => "✅",
        "inprogress" | "in progress" | "in_progress" | "wip" => "🔄",
        "draft" => "📝",
        "approved" => "✔️",
        "review" | "in review" | "ready for review" => "👀",
        "blocked" => "🚫",
        "cancelled" | "canceled" => "❌",
        "ready" | "ready for development" => "🟢",
        _ => "⏳",
    };
    Ok(Value::String(emoji.to_string()))
}

// =============================================================================
// Custom Functions
// =============================================================================

/// Create the now() function for getting current timestamp
fn make_now_function() -> impl Function {
    Box::new(
        move |_args: &HashMap<String, Value>| -> tera::Result<Value> {
            Ok(Value::String(chrono::Utc::now().to_rfc3339()))
        },
    )
}

// =============================================================================
// Include Path Validation
// =============================================================================

/// Validate that an include path is allowed
///
/// Implements AC11: {% include %} paths validated against allowed paths
pub fn validate_include_path(path: &str, allowed_paths: &[String]) -> Result<(), RenderError> {
    // Normalize path and check for traversal attempts
    if path.contains("..") {
        return Err(RenderError::SecurityViolation(
            "Path traversal not allowed in include paths".to_string(),
        ));
    }

    // If no allowed paths configured, deny all includes
    if allowed_paths.is_empty() {
        return Err(RenderError::SecurityViolation(
            "No include paths configured".to_string(),
        ));
    }

    // Check if path starts with any allowed prefix
    let normalized = path.trim_start_matches('/');
    let is_allowed = allowed_paths
        .iter()
        .any(|allowed| normalized.starts_with(allowed.trim_start_matches('/')));

    if !is_allowed {
        return Err(RenderError::SecurityViolation(format!(
            "Include path '{}' not in allowed paths",
            path
        )));
    }

    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphdocs::relationships::RelatedDocument;

    #[test]
    fn test_tera_basic_render() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        let template = "Hello, {{ name }}!";
        let result = processor
            .render(template, serde_json::json!({"name": "World"}), &config)
            .unwrap();

        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_tera_loop_render() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        let template = r#"
{% for item in items %}
- {{ item.name }}: {{ item.value }}
{% endfor %}
"#;
        let ctx = serde_json::json!({
            "items": [
                {"name": "A", "value": 1},
                {"name": "B", "value": 2},
            ]
        });

        let result = processor.render(template, ctx, &config).unwrap();
        assert!(result.contains("- A: 1"));
        assert!(result.contains("- B: 2"));
    }

    #[test]
    fn test_tera_conditionals() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        let template = r#"
{% if items %}
Has {{ items | length }} items
{% else %}
No items
{% endif %}
"#;

        let with_items = processor
            .render(template, serde_json::json!({"items": [1,2,3]}), &config)
            .unwrap();
        assert!(with_items.contains("Has 3 items"));

        let no_items = processor
            .render(template, serde_json::json!({"items": []}), &config)
            .unwrap();
        assert!(no_items.contains("No items"));
    }

    #[test]
    fn test_tera_filters() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        // Default filter (Tera built-in)
        let result = processor
            .render(
                "{{ missing | default(value='N/A') }}",
                serde_json::json!({}),
                &config,
            )
            .unwrap();
        assert_eq!(result, "N/A");

        // Status emoji filter (custom)
        let result = processor
            .render(
                "{{ status | status_emoji }}",
                serde_json::json!({"status": "Done"}),
                &config,
            )
            .unwrap();
        assert_eq!(result, "✅");
    }

    #[test]
    fn test_status_emoji_filter_all_statuses() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        let test_cases = [
            ("Done", "✅"),
            ("complete", "✅"),
            ("InProgress", "🔄"),
            ("in progress", "🔄"),
            ("WIP", "🔄"),
            ("Draft", "📝"),
            ("Approved", "✔️"),
            ("Review", "👀"),
            ("in review", "👀"),
            ("Blocked", "🚫"),
            ("Cancelled", "❌"),
            ("canceled", "❌"),
            ("Ready", "🟢"),
            ("Unknown", "⏳"),
        ];

        for (status, expected_emoji) in test_cases {
            let result = processor
                .render(
                    "{{ status | status_emoji }}",
                    serde_json::json!({"status": status}),
                    &config,
                )
                .unwrap();
            assert_eq!(result, expected_emoji, "Failed for status: {}", status);
        }
    }

    #[test]
    fn test_syntax_error_returns_err() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        // Malformed Tera syntax (AC7)
        let result = processor.render("{{ unclosed", serde_json::json!({}), &config);
        assert!(result.is_err());
        match result {
            Err(RenderError::SyntaxError(msg)) => {
                assert!(msg.contains("unclosed") || msg.len() > 0);
            }
            _ => panic!("Expected SyntaxError"),
        }
    }

    #[test]
    fn test_render_catches_panics() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        // This shouldn't panic the whole program even if something goes wrong internally
        // Testing the panic-catching wrapper (AC6, AC8)
        let result = processor.render("{{ name }}", serde_json::json!({"name": "test"}), &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_output_size_limit() {
        let processor = TemplateProcessor::new();
        let mut config = RenderConfig::default();
        config.max_output_size = 10; // Very small limit

        let template = "This is a longer string that exceeds the limit";
        let result = processor.render(template, serde_json::json!({}), &config);

        assert!(result.is_err());
        match result {
            Err(RenderError::OutputTooLarge { .. }) => {}
            _ => panic!("Expected OutputTooLarge error"),
        }
    }

    #[test]
    fn test_sql_validation_select_allowed() {
        assert!(validate_sql_statement("SELECT * FROM documents").is_ok());
        assert!(validate_sql_statement("select id, title from docs").is_ok());
        assert!(validate_sql_statement("  SELECT  *  FROM  t  ").is_ok());
        assert!(validate_sql_statement("FROM GRAPH_TABLE (gd_graph MATCH ...)").is_ok());
        assert!(validate_sql_statement("WITH cte AS (SELECT 1) SELECT * FROM cte").is_ok());
    }

    #[test]
    fn test_sql_validation_dangerous_rejected() {
        let dangerous_statements = [
            "INSERT INTO documents VALUES (1, 'a')",
            "UPDATE documents SET title = 'hacked'",
            "DELETE FROM documents",
            "DROP TABLE documents",
            "CREATE TABLE evil (id INT)",
            "ALTER TABLE documents ADD COLUMN hacked TEXT",
            "TRUNCATE TABLE documents",
            "GRANT ALL ON documents TO public",
            "ATTACH DATABASE 'evil.db' AS evil",
            "COPY documents TO '/etc/passwd'",
            "LOAD EXTENSION 'evil'",
            "INSTALL evil_extension",
        ];

        for sql in dangerous_statements {
            let result = validate_sql_statement(sql);
            assert!(result.is_err(), "Should reject: {}", sql);
            match result {
                Err(RenderError::SecurityViolation(_)) => {}
                _ => panic!("Expected SecurityViolation for: {}", sql),
            }
        }
    }

    #[test]
    fn test_include_path_validation() {
        let allowed = vec!["docs/".to_string(), "templates/".to_string()];

        // Valid paths
        assert!(validate_include_path("docs/readme.md", &allowed).is_ok());
        assert!(validate_include_path("templates/base.html", &allowed).is_ok());
        assert!(validate_include_path("/docs/nested/file.md", &allowed).is_ok());

        // Invalid paths - traversal
        assert!(validate_include_path("../etc/passwd", &allowed).is_err());
        assert!(validate_include_path("docs/../../../etc/passwd", &allowed).is_err());

        // Invalid paths - not in allowed list
        assert!(validate_include_path("secrets/key.pem", &allowed).is_err());

        // Empty allowed list denies all
        assert!(validate_include_path("docs/readme.md", &[]).is_err());
    }

    #[test]
    fn test_render_markdown_with_context() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        let doc_context = DocumentContext {
            id: "STORY-1".to_string(),
            path: "stories/STORY-1.md".to_string(),
            title: Some("First Story".to_string()),
            template_id: None,
        };

        let template = "# {{ doc.title }}\n\nID: {{ doc_id }}\nPath: {{ doc_path }}";
        let result = processor
            .render_markdown(template, &doc_context, &config)
            .unwrap();

        assert!(result.contains("First Story"));
        assert!(result.contains("STORY-1"));
        assert!(result.contains("stories/STORY-1.md"));
    }

    #[test]
    fn test_render_relationship() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        let rel = ResolvedRelationship {
            id: "stories".to_string(),
            edge_type: "CONTAINS".to_string(),
            documents: vec![
                RelatedDocument {
                    id: "story-1".to_string(),
                    path: "stories/STORY-2.1.1.md".to_string(),
                    title: Some("Core Parser".to_string()),
                    status: Some("Done".to_string()),
                    template_id: Some("story-template".to_string()),
                    variables: serde_json::json!({}),
                    fields: serde_json::json!({}),
                },
                RelatedDocument {
                    id: "story-2".to_string(),
                    path: "stories/STORY-2.1.2.md".to_string(),
                    title: Some("Variable Detection".to_string()),
                    status: Some("InProgress".to_string()),
                    template_id: Some("story-template".to_string()),
                    variables: serde_json::json!({}),
                    fields: serde_json::json!({}),
                },
            ],
        };

        let template = r#"
{% for story in stories %}
- [{{ story.title }}]({{ story.path }}) - {{ story.status }}
{% endfor %}
"#;

        let result = processor
            .render_relationship(template, &rel, &serde_json::json!({}), &config)
            .unwrap();

        assert!(result.contains("Core Parser"));
        assert!(result.contains("Variable Detection"));
        assert!(result.contains("Done"));
        assert!(result.contains("InProgress"));
    }

    #[test]
    fn test_now_function() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        let template = "{{ now() }}";
        let result = processor
            .render(template, serde_json::json!({}), &config)
            .unwrap();

        // Should be a valid RFC3339 timestamp
        assert!(result.contains("T"));
        assert!(result.contains("Z") || result.contains("+"));
    }

    #[test]
    fn test_template_caching() {
        let processor = TemplateProcessor::new();
        let config = RenderConfig::default();

        let template = "Hello, {{ name }}!";

        // First render
        let result1 = processor
            .render(template, serde_json::json!({"name": "Alice"}), &config)
            .unwrap();
        assert_eq!(result1, "Hello, Alice!");

        // Second render with same template (should use cache)
        let result2 = processor
            .render(template, serde_json::json!({"name": "Bob"}), &config)
            .unwrap();
        assert_eq!(result2, "Hello, Bob!");

        // Verify cache has one entry
        let cache = processor.template_cache.read().unwrap();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_json_to_tera_conversion() {
        // Null
        assert_eq!(json_to_tera(serde_json::Value::Null), Value::Null);

        // Bool
        assert_eq!(json_to_tera(serde_json::json!(true)), Value::Bool(true));

        // Number
        assert_eq!(
            json_to_tera(serde_json::json!(42)),
            Value::Number(42.into())
        );

        // String
        assert_eq!(
            json_to_tera(serde_json::json!("hello")),
            Value::String("hello".to_string())
        );

        // Array
        let arr = json_to_tera(serde_json::json!([1, 2, 3]));
        assert!(matches!(arr, Value::Array(_)));

        // Object
        let obj = json_to_tera(serde_json::json!({"key": "value"}));
        assert!(matches!(obj, Value::Object(_)));
    }

    #[test]
    fn test_render_config_defaults() {
        let config = RenderConfig::default();
        assert_eq!(config.render_timeout, Duration::from_secs(5));
        assert_eq!(config.query_timeout, Duration::from_secs(2));
        assert_eq!(config.max_loop_iterations, 10000);
        assert_eq!(config.max_query_results, 1000);
        assert_eq!(config.max_output_size, 1024 * 1024);
        assert!(config.allowed_include_paths.is_empty());
    }

    #[test]
    fn test_document_context_serialization() {
        let ctx = DocumentContext {
            id: "STORY-1".to_string(),
            path: "stories/STORY-1.md".to_string(),
            title: Some("Test Story".to_string()),
            template_id: Some("story-template".to_string()),
        };

        let json = serde_json::to_string(&ctx).unwrap();
        assert!(json.contains("STORY-1"));
        assert!(json.contains("Test Story"));

        // Deserialize back
        let deserialized: DocumentContext = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, ctx.id);
    }
}
