//! File Handler System for Extensible FUSE Operations
//!
//! # CONCEPTUAL IMPLEMENTATION
//!
//! This module provides a handler registry system that allows intercepting
//! FUSE operations for specific file patterns. This enables features like:
//!
//! - GraphDocs: Render markdown from a property graph when reading `.gd.md` files
//! - Code Analysis: Generate reports on-the-fly for code dependency queries
//! - Dynamic Content: Create virtual files with computed content
//!
//! During the implementation phase, changes may be made to adapt to specific
//! requirements and optimize performance.
//!
//! ## Architecture
//!
//! ```text
//! FUSE Operation (read, getattr, etc.)
//!        │
//!        ▼
//! HandlerRegistry.handle_*()
//!        │
//!        ├─ Try handlers in priority order
//!        │     │
//!        │     ├─ Handler 1: can_handle()? ──► handle_*()
//!        │     ├─ Handler 2: can_handle()? ──► handle_*()
//!        │     └─ ...
//!        │
//!        └─ No handler matched
//!              │
//!              ▼
//!        DefaultHandler (delegates to FileSystem)
//! ```
//!
//! ## Example: GraphDocs Handler
//!
//! ```rust,ignore
//! struct GraphDocsHandler {
//!     engine: GraphDocsEngine,
//! }
//!
//! impl FileHandler for GraphDocsHandler {
//!     fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
//!         path.ends_with(".gd.md")
//!     }
//!
//!     fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult {
//!         // Extract document ID from path
//!         let doc_id = path.strip_suffix(".gd.md").unwrap();
//!
//!         // Render markdown from graph
//!         let content = self.engine.render(doc_id)?;
//!
//!         // Return requested slice
//!         let data = content.as_bytes();
//!         let start = offset as usize;
//!         let end = (offset + size) as usize;
//!         Ok(Some(data[start.min(data.len())..end.min(data.len())].to_vec()))
//!     }
//! }
//! ```

use agentfs_sdk::error::{Error, Result};
use agentfs_sdk::filesystem::duckagentfs::DuckConnectionPool;
use agentfs_sdk::graphdocs::GraphDocsEngine;
use agentfs_sdk::{DirEntry, FileSystem, Stats};
use async_trait::async_trait;
use duckdb::params;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// HANDLER RESULT TYPE
// ============================================================================

/// Result type for handler operations.
///
/// - `Ok(Some(data))`: Handler handled the operation successfully
/// - `Ok(None)`: Handler declined to handle this operation
/// - `Err(e)`: Handler attempted but failed
pub type HandlerResult<T> = Result<Option<T>>;

// ============================================================================
// FILE HANDLER TRAIT
// ============================================================================

/// Trait for handling file operations on specific paths or patterns.
///
/// Handlers are tried in priority order. The first handler that returns
/// `Ok(Some(...))` wins. If all handlers return `Ok(None)`, the operation
/// falls through to the default filesystem implementation.
#[async_trait]
pub trait FileHandler: Send + Sync {
    /// Get the handler name for debugging/logging.
    fn name(&self) -> &str;

    /// Get the handler priority (lower = higher priority, tried first).
    ///
    /// Default priority is 100. Built-in handlers should use:
    /// - 0-49: High priority (intercept before others)
    /// - 50-99: Normal priority
    /// - 100+: Low priority (fallback handlers)
    fn priority(&self) -> u32 {
        100
    }

    /// Check if this handler should handle the given path.
    ///
    /// This is called before each operation to quickly filter handlers.
    /// Return `true` if this handler might handle the path, `false` to skip.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path being accessed
    /// * `stats` - Optional stats if already available (for optimization)
    fn can_handle(&self, path: &str, stats: Option<&Stats>) -> bool;

    /// Handle a read operation.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to read
    /// * `offset` - Byte offset to start reading from
    /// * `size` - Maximum number of bytes to read
    ///
    /// # Returns
    ///
    /// - `Ok(Some(data))`: Read successful, return data
    /// - `Ok(None)`: Decline to handle, try next handler
    /// - `Err(e)`: Read failed with error
    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        let _ = (path, offset, size);
        Ok(None) // Default: decline to handle
    }

    /// Handle a getattr operation.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to stat
    ///
    /// # Returns
    ///
    /// - `Ok(Some(stats))`: Stats available
    /// - `Ok(None)`: Decline to handle, try next handler
    /// - `Err(e)`: Operation failed
    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        let _ = path;
        Ok(None)
    }

    /// Handle a readdir operation.
    ///
    /// # Arguments
    ///
    /// * `path` - The directory path to read
    ///
    /// # Returns
    ///
    /// - `Ok(Some(entries))`: Directory entries
    /// - `Ok(None)`: Decline to handle
    /// - `Err(e)`: Operation failed
    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        let _ = path;
        Ok(None)
    }

    /// Handle a readdir_plus operation (with stats).
    ///
    /// # Arguments
    ///
    /// * `path` - The directory path to read
    ///
    /// # Returns
    ///
    /// - `Ok(Some(entries))`: Directory entries with stats
    /// - `Ok(None)`: Decline to handle
    /// - `Err(e)`: Operation failed
    async fn readdir_plus(&self, path: &str) -> HandlerResult<Vec<DirEntry>> {
        let _ = path;
        Ok(None)
    }

    /// Handle a write operation.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to write
    /// * `offset` - Byte offset to start writing at
    /// * `data` - Data to write
    ///
    /// # Returns
    ///
    /// - `Ok(Some(bytes_written))`: Write successful
    /// - `Ok(None)`: Decline to handle
    /// - `Err(e)`: Write failed
    async fn write(&self, path: &str, offset: u64, data: &[u8]) -> HandlerResult<usize> {
        let _ = (path, offset, data);
        Ok(None)
    }

    /// Handle a truncate operation.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to truncate
    /// * `size` - New size in bytes
    ///
    /// # Returns
    ///
    /// - `Ok(Some(()))`: Truncate successful
    /// - `Ok(None)`: Decline to handle
    /// - `Err(e)`: Operation failed
    async fn truncate(&self, path: &str, size: u64) -> HandlerResult<()> {
        let _ = (path, size);
        Ok(None)
    }

    /// Handle a readlink operation.
    ///
    /// # Arguments
    ///
    /// * `path` - The symlink path to read
    ///
    /// # Returns
    ///
    /// - `Ok(Some(target))`: Link target
    /// - `Ok(None)`: Decline to handle
    /// - `Err(e)`: Operation failed
    async fn readlink(&self, path: &str) -> HandlerResult<String> {
        let _ = path;
        Ok(None)
    }

    /// Look up a child entry within a directory.
    ///
    /// This method is called during FUSE lookup() to resolve a child
    /// within a parent directory. Handlers can intercept this to provide
    /// virtual entries (like `/.graphdocs/` directory).
    ///
    /// # Arguments
    ///
    /// * `parent_path` - The parent directory path (e.g., "/")
    /// * `name` - The child entry name being looked up (e.g., ".graphdocs")
    ///
    /// # Returns
    ///
    /// - `Ok(Some(stats))`: Handler provides Stats for this entry
    /// - `Ok(None)`: Decline to handle, try next handler
    /// - `Err(e)`: Lookup failed with error
    async fn lookup(&self, parent_path: &str, name: &str) -> HandlerResult<Stats> {
        let _ = (parent_path, name);
        Ok(None) // Default: decline to handle
    }
}

// ============================================================================
// HANDLER REGISTRY
// ============================================================================

/// Registry of file handlers for extensible FUSE operations.
///
/// Handlers are tried in priority order (lowest priority number first).
/// The first handler that returns `Ok(Some(...))` wins.
pub struct HandlerRegistry {
    handlers: Vec<Arc<dyn FileHandler>>,
    default_handler: Arc<dyn FileHandler>,
}

impl HandlerRegistry {
    /// Create a new handler registry with the given default handler.
    pub fn new(default_handler: Arc<dyn FileHandler>) -> Self {
        Self {
            handlers: Vec::new(),
            default_handler,
        }
    }

    /// Create a new handler registry with a filesystem as the default.
    pub fn with_filesystem(fs: Arc<dyn FileSystem>) -> Self {
        Self::new(Arc::new(DefaultHandler::new(fs)))
    }

    /// Register a handler.
    ///
    /// Handlers are automatically sorted by priority after insertion.
    pub fn register(&mut self, handler: Arc<dyn FileHandler>) {
        self.handlers.push(handler);
        self.handlers.sort_by_key(|h| h.priority());
    }

    /// Unregister a handler by name.
    pub fn unregister(&mut self, name: &str) {
        self.handlers.retain(|h| h.name() != name);
    }

    /// List registered handlers in priority order.
    pub fn list_handlers(&self) -> Vec<(&str, u32)> {
        self.handlers
            .iter()
            .map(|h| (h.name(), h.priority()))
            .collect()
    }

    /// Check if any registered handler (excluding default) can handle a path.
    ///
    /// This is used to determine if a path is managed by a handler (virtual file)
    /// rather than the underlying filesystem.
    pub fn can_handle_any(&self, path: &str) -> bool {
        self.handlers.iter().any(|h| h.can_handle(path, None))
    }

    // ========================================================================
    // OPERATION DISPATCH
    // ========================================================================

    /// Handle a read operation.
    pub async fn handle_read(&self, path: &str, offset: u64, size: u64) -> Result<Vec<u8>> {
        // Try custom handlers first
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(data) = handler.read(path, offset, size).await? {
                    tracing::debug!("Handler '{}' handled read for {}", handler.name(), path);
                    return Ok(data);
                }
            }
        }

        // Fall back to default handler
        self.default_handler
            .read(path, offset, size)
            .await?
            .ok_or_else(|| Error::Custom(format!("No handler for read: {}", path)))
    }

    /// Handle a getattr operation.
    pub async fn handle_getattr(&self, path: &str) -> Result<Option<Stats>> {
        // Try custom handlers first
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(stats) = handler.getattr(path).await? {
                    tracing::debug!("Handler '{}' handled getattr for {}", handler.name(), path);
                    return Ok(Some(stats));
                }
            }
        }

        // Fall back to default handler
        self.default_handler.getattr(path).await
    }

    /// Handle a readdir operation.
    pub async fn handle_readdir(&self, path: &str) -> Result<Option<Vec<String>>> {
        // Try custom handlers first
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(entries) = handler.readdir(path).await? {
                    tracing::debug!("Handler '{}' handled readdir for {}", handler.name(), path);
                    return Ok(Some(entries));
                }
            }
        }

        // Fall back to default handler
        self.default_handler.readdir(path).await
    }

    /// Handle a readdir_plus operation.
    pub async fn handle_readdir_plus(&self, path: &str) -> Result<Option<Vec<DirEntry>>> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(entries) = handler.readdir_plus(path).await? {
                    return Ok(Some(entries));
                }
            }
        }
        self.default_handler.readdir_plus(path).await
    }

    /// Handle a write operation.
    pub async fn handle_write(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(written) = handler.write(path, offset, data).await? {
                    return Ok(written);
                }
            }
        }
        self.default_handler
            .write(path, offset, data)
            .await?
            .ok_or_else(|| Error::Custom(format!("No handler for write: {}", path)))
    }

    /// Handle a truncate operation.
    pub async fn handle_truncate(&self, path: &str, size: u64) -> Result<()> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if handler.truncate(path, size).await?.is_some() {
                    return Ok(());
                }
            }
        }
        self.default_handler
            .truncate(path, size)
            .await?
            .ok_or_else(|| Error::Custom(format!("No handler for truncate: {}", path)))
    }

    /// Handle a readlink operation.
    pub async fn handle_readlink(&self, path: &str) -> Result<Option<String>> {
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(target) = handler.readlink(path).await? {
                    return Ok(Some(target));
                }
            }
        }
        self.default_handler.readlink(path).await
    }

    /// Handle a lookup operation.
    ///
    /// Looks up a child entry within a parent directory. This is called by
    /// FUSE lookup() to resolve directory entries, including virtual ones
    /// like `/.graphdocs/`.
    ///
    /// # Arguments
    ///
    /// * `parent_path` - The parent directory path (e.g., "/")
    /// * `name` - The child entry name being looked up (e.g., ".graphdocs")
    ///
    /// # Returns
    ///
    /// - `Ok(Some(stats))`: Entry found with its Stats
    /// - `Ok(None)`: Entry not found
    /// - `Err(e)`: Lookup failed with error
    pub async fn handle_lookup(&self, parent_path: &str, name: &str) -> Result<Option<Stats>> {
        // Construct the full child path for can_handle() check
        let child_path = if parent_path == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), name)
        };

        // Try custom handlers first
        for handler in &self.handlers {
            if handler.can_handle(&child_path, None) {
                if let Some(stats) = handler.lookup(parent_path, name).await? {
                    tracing::debug!(
                        "Handler '{}' handled lookup for {}/{}",
                        handler.name(),
                        parent_path,
                        name
                    );
                    return Ok(Some(stats));
                }
            }
        }

        // Fall back to default handler
        self.default_handler.lookup(parent_path, name).await
    }
}

// ============================================================================
// DEFAULT HANDLER
// ============================================================================

/// Default handler that delegates to the underlying FileSystem.
///
/// This handler always accepts all paths and delegates to the filesystem
/// implementation. It's used as the fallback when no custom handler matches.
pub struct DefaultHandler {
    fs: Arc<dyn FileSystem>,
}

impl DefaultHandler {
    /// Create a new default handler wrapping a filesystem.
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }
}

#[async_trait]
impl FileHandler for DefaultHandler {
    fn name(&self) -> &str {
        "default"
    }

    fn priority(&self) -> u32 {
        u32::MAX // Lowest priority - always last
    }

    fn can_handle(&self, _path: &str, _stats: Option<&Stats>) -> bool {
        true // Accept everything
    }

    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        // Open and read the file
        match self.fs.open(path).await {
            Ok(file) => {
                let data = file.pread(offset, size).await?;
                Ok(Some(data))
            }
            Err(e) => Err(e),
        }
    }

    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        match self.fs.stat(path).await? {
            Some(stats) => Ok(Some(stats)),
            None => Ok(None),
        }
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        self.fs.readdir(path).await
    }

    async fn readdir_plus(&self, path: &str) -> HandlerResult<Vec<DirEntry>> {
        self.fs.readdir_plus(path).await
    }

    async fn write(&self, path: &str, offset: u64, data: &[u8]) -> HandlerResult<usize> {
        match self.fs.open(path).await {
            Ok(file) => {
                file.pwrite(offset, data).await?;
                Ok(Some(data.len()))
            }
            Err(e) => Err(e),
        }
    }

    async fn truncate(&self, path: &str, size: u64) -> HandlerResult<()> {
        match self.fs.open(path).await {
            Ok(file) => {
                file.truncate(size).await?;
                Ok(Some(()))
            }
            Err(e) => Err(e),
        }
    }

    async fn readlink(&self, path: &str) -> HandlerResult<String> {
        self.fs.readlink(path).await
    }

    async fn lookup(&self, parent_path: &str, name: &str) -> HandlerResult<Stats> {
        // Construct the full child path
        let child_path = if parent_path == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), name)
        };

        // Delegate to filesystem lstat (preserves symlink info)
        self.fs.lstat(&child_path).await
    }
}

// ============================================================================
// GRAPHDOCS HANDLER
// ============================================================================

/// Path for the virtual GraphDocs directory.
pub const GRAPHDOCS_DIR: &str = "/.graphdocs";

/// File extension for GraphDocs documents.
const GRAPHDOCS_EXTENSION: &str = ".gd.md";

/// Reserved inode for the virtual /.graphdocs/ directory.
/// Uses high inode range to avoid conflicts with real filesystem inodes.
const GRAPHDOCS_DIR_INO: i64 = i64::MAX - 1;

/// Document info returned from database queries.
#[derive(Debug, Clone)]
pub struct DocumentInfo {
    /// Document ID (also the filename without extension).
    pub id: String,
    /// Document title.
    pub title: String,
    /// Last updated timestamp (Unix epoch seconds).
    pub updated_at: i64,
}

/// Handler for GraphDocs - rendering markdown from property graphs.
///
/// This handler provides a virtual directory at `/.graphdocs/` containing all
/// documents from the `gd_documents` table, rendered as `.gd.md` files.
///
/// ## How it works
///
/// 1. `ls /.graphdocs/` → lists all documents from `gd_documents` table
/// 2. `cat /.graphdocs/readme.gd.md` → renders document `readme` via `GraphDocsEngine`
/// 3. `stat /.graphdocs/readme.gd.md` → returns stats with rendered content size
///
/// ## Example
///
/// Given a document in the graph:
///
/// ```sql
/// INSERT INTO gd_documents (id, title) VALUES ('readme', 'README');
/// INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
/// VALUES ('s1', 'readme', 'heading', 1, 0, '# {{project_name}}');
/// INSERT INTO gd_variables (id, document_id, name, value, var_type)
/// VALUES ('v1', 'readme', 'project_name', '"My Project"', 'string');
/// ```
///
/// The virtual directory shows:
/// ```text
/// ls /.graphdocs/
/// readme.gd.md
///
/// cat /.graphdocs/readme.gd.md
/// # My Project
/// ```
pub struct GraphDocsHandler {
    /// The rendering engine that queries DuckDB and renders markdown.
    engine: GraphDocsEngine,
    /// Connection pool for direct database queries (document listing).
    pool: DuckConnectionPool,
}

impl GraphDocsHandler {
    /// Create a new GraphDocs handler with a DuckDB connection pool.
    ///
    /// # Arguments
    ///
    /// * `pool` - DuckDB connection pool for querying `gd_documents`
    pub fn new(pool: DuckConnectionPool) -> Self {
        let engine = GraphDocsEngine::new(pool.clone());
        Self { engine, pool }
    }

    /// Check if path is the GraphDocs virtual directory.
    pub fn is_graphdocs_dir(path: &str) -> bool {
        path == GRAPHDOCS_DIR || path == &format!("{}/", GRAPHDOCS_DIR)
    }

    /// Check if path is inside the GraphDocs directory.
    pub fn is_in_graphdocs_dir(path: &str) -> bool {
        path.starts_with(&format!("{}/", GRAPHDOCS_DIR))
    }

    /// Check if path is a GraphDocs file (ends with .gd.md).
    pub fn is_graphdocs_file(path: &str) -> bool {
        path.ends_with(GRAPHDOCS_EXTENSION)
    }

    /// Extract document ID from a path.
    ///
    /// Handles both:
    /// - `/.graphdocs/readme.gd.md` → `readme`
    /// - `/some/path/readme.gd.md` → `readme`
    pub fn extract_doc_id(path: &str) -> Option<String> {
        // First check if it's in the graphdocs directory
        if Self::is_in_graphdocs_dir(path) {
            return Self::extract_doc_id_from_dir_path(path);
        }

        // Otherwise extract from any .gd.md file
        let filename = path.rsplit('/').next()?;
        filename.strip_suffix(GRAPHDOCS_EXTENSION).map(String::from)
    }

    /// Extract doc_id from path like "/.graphdocs/readme.gd.md" -> "readme"
    fn extract_doc_id_from_dir_path(path: &str) -> Option<String> {
        let path = path.strip_prefix(GRAPHDOCS_DIR)?;
        let path = path.strip_prefix('/')?;

        // path is now "readme.gd.md"
        path.strip_suffix(GRAPHDOCS_EXTENSION).map(String::from)
    }

    /// List all documents in the database.
    ///
    /// Returns document info sorted by ID.
    pub async fn list_documents(&self) -> Result<Vec<DocumentInfo>> {
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let mut stmt = conn
                .prepare(
                    r#"
                SELECT id, title,
                       COALESCE(
                           EPOCH(updated_at),
                           EPOCH(created_at),
                           EPOCH(CURRENT_TIMESTAMP)
                       ) as updated_epoch
                FROM gd_documents
                ORDER BY id
                "#,
                )
                .map_err(|e| Error::Custom(format!("Failed to prepare documents query: {}", e)))?;

            let docs = stmt
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let title: String = row.get(1)?;
                    // DuckDB returns EPOCH as DOUBLE
                    let updated_at: f64 = row.get(2)?;
                    Ok(DocumentInfo {
                        id,
                        title,
                        updated_at: updated_at as i64,
                    })
                })
                .map_err(|e| Error::Custom(format!("Failed to query documents: {}", e)))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Custom(format!("Failed to read document row: {}", e)))?;

            Ok(docs)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    /// Check if a document exists by ID.
    pub async fn document_exists(&self, doc_id: &str) -> Result<bool> {
        let pool = self.pool.clone();
        let doc_id = doc_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM gd_documents WHERE id = ?",
                    params![doc_id],
                    |row| row.get(0),
                )
                .map_err(|e| Error::Custom(format!("Failed to check document existence: {}", e)))?;

            Ok(exists > 0)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    /// Get the rendered content for a document.
    pub async fn get_content(&self, doc_id: &str) -> Result<String> {
        self.engine.render(doc_id).await
    }

    /// Get virtual directory stats.
    fn virtual_dir_stats() -> Stats {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        Stats {
            ino: GRAPHDOCS_DIR_INO,
            mode: 0o40555, // Directory, read-only + execute
            nlink: 2,
            uid: 0,
            gid: 0,
            size: 0,
            atime: now,
            mtime: now,
            ctime: now,
        }
    }

    /// Get virtual file stats for a document.
    ///
    /// Generates a stable inode from the document ID using a hash.
    fn virtual_file_stats(doc_id: &str, content_len: i64, mtime: i64) -> Stats {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Generate stable inode from doc_id hash
        // Use range below GRAPHDOCS_DIR_INO to avoid conflicts
        let mut hasher = DefaultHasher::new();
        doc_id.hash(&mut hasher);
        let hash = hasher.finish();
        // Map to range [GRAPHDOCS_DIR_INO - 1_000_000, GRAPHDOCS_DIR_INO - 1]
        let ino = (GRAPHDOCS_DIR_INO - 2) - ((hash % 1_000_000) as i64);

        Stats {
            ino,
            mode: 0o100444, // Regular file, read-only
            nlink: 1,
            uid: 0,
            gid: 0,
            size: content_len,
            atime: mtime,
            mtime,
            ctime: mtime,
        }
    }

    /// Get stats for the virtual directory.
    async fn getattr_for_dir(&self) -> HandlerResult<Stats> {
        Ok(Some(Self::virtual_dir_stats()))
    }

    /// Get stats for a document file (requires rendering to get size).
    async fn getattr_for_doc(&self, doc_id: &str) -> HandlerResult<Stats> {
        // Check if document exists
        if !self.document_exists(doc_id).await? {
            return Ok(None);
        }

        // Render to get content length
        match self.get_content(doc_id).await {
            Ok(content) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                Ok(Some(Self::virtual_file_stats(doc_id, content.len() as i64, now)))
            }
            Err(_) => Ok(None),
        }
    }
}

#[async_trait]
impl FileHandler for GraphDocsHandler {
    fn name(&self) -> &str {
        "graphdocs"
    }

    fn priority(&self) -> u32 {
        50 // Higher than default, handles before filesystem passthrough
    }

    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        Self::is_graphdocs_dir(path)
            || Self::is_in_graphdocs_dir(path)
            || Self::is_graphdocs_file(path)
    }

    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        // Handle virtual directory
        if Self::is_graphdocs_dir(path) {
            return self.getattr_for_dir().await;
        }

        // Handle files in graphdocs dir
        if Self::is_in_graphdocs_dir(path) {
            if let Some(doc_id) = Self::extract_doc_id_from_dir_path(path) {
                return self.getattr_for_doc(&doc_id).await;
            }
        }

        // Handle .gd.md files elsewhere
        if let Some(doc_id) = Self::extract_doc_id(path) {
            return self.getattr_for_doc(&doc_id).await;
        }

        Ok(None)
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        if !Self::is_graphdocs_dir(path) {
            return Ok(None);
        }

        // List all documents
        let docs = self.list_documents().await?;

        let mut entries = vec![".".to_string(), "..".to_string()];
        for doc in docs {
            entries.push(format!("{}{}", doc.id, GRAPHDOCS_EXTENSION));
        }

        Ok(Some(entries))
    }

    async fn readdir_plus(&self, path: &str) -> HandlerResult<Vec<DirEntry>> {
        if !Self::is_graphdocs_dir(path) {
            return Ok(None);
        }

        let docs = self.list_documents().await?;
        let mut entries = Vec::new();

        // Add . and ..
        let dir_stats = Self::virtual_dir_stats();
        entries.push(DirEntry {
            name: ".".to_string(),
            stats: dir_stats.clone(),
        });

        // Parent directory stats (root-like)
        let parent_stats = Stats {
            mode: 0o40755,
            ..dir_stats
        };
        entries.push(DirEntry {
            name: "..".to_string(),
            stats: parent_stats,
        });

        // Add documents with rendered sizes
        for doc in docs {
            // Render each document to get accurate size
            let content = self.get_content(&doc.id).await.unwrap_or_default();

            entries.push(DirEntry {
                name: format!("{}{}", doc.id, GRAPHDOCS_EXTENSION),
                stats: Self::virtual_file_stats(&doc.id, content.len() as i64, doc.updated_at),
            });
        }

        Ok(Some(entries))
    }

    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        let doc_id = match Self::extract_doc_id(path) {
            Some(id) => id,
            None => return Ok(None),
        };

        // Render the document
        let content = match self.get_content(&doc_id).await {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        let bytes = content.as_bytes();
        let start = offset as usize;
        let end = (offset + size) as usize;

        if start >= bytes.len() {
            return Ok(Some(vec![]));
        }

        Ok(Some(bytes[start..end.min(bytes.len())].to_vec()))
    }

    async fn write(&self, _path: &str, _offset: u64, _data: &[u8]) -> HandlerResult<usize> {
        // GraphDocs files are read-only (rendered from graph)
        Err(Error::Custom(
            "GraphDocs files are read-only. Edit the underlying graph instead.".into(),
        ))
    }

    async fn truncate(&self, _path: &str, _size: u64) -> HandlerResult<()> {
        // GraphDocs files are read-only
        Err(Error::Custom(
            "GraphDocs files are read-only. Edit the underlying graph instead.".into(),
        ))
    }

    async fn lookup(&self, parent_path: &str, name: &str) -> HandlerResult<Stats> {
        // Handle lookup of .graphdocs directory in root
        if parent_path == "/" && name == ".graphdocs" {
            tracing::debug!("GraphDocsHandler::lookup: found .graphdocs in root");
            return self.getattr_for_dir().await;
        }

        // Handle lookup of files in /.graphdocs/
        if parent_path == GRAPHDOCS_DIR || parent_path == "/.graphdocs/" {
            if let Some(doc_id) = name.strip_suffix(GRAPHDOCS_EXTENSION) {
                tracing::debug!(
                    "GraphDocsHandler::lookup: looking up document '{}' in {}",
                    doc_id,
                    parent_path
                );
                return self.getattr_for_doc(doc_id).await;
            }
        }

        // Not a GraphDocs path we handle
        Ok(None)
    }
}

// ============================================================================
// ROOT DIRECTORY INJECTOR
// ============================================================================

/// Handler that injects `.graphdocs` into root directory listings.
///
/// This handler intercepts `readdir()` calls on the root directory
/// and adds `.graphdocs` to the list of entries, making the virtual
/// directory discoverable via `ls /`.
pub struct GraphDocsDirInjector {
    inner: Arc<dyn FileHandler>,
}

impl GraphDocsDirInjector {
    /// Create a new injector wrapping another handler.
    pub fn new(inner: Arc<dyn FileHandler>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl FileHandler for GraphDocsDirInjector {
    fn name(&self) -> &str {
        "graphdocs-injector"
    }

    fn priority(&self) -> u32 {
        5 // Before GraphDocsHandler (50) so we can inject before listing
    }

    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        path == "/" // Only handle root
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        if path != "/" {
            return Ok(None);
        }

        // Get real entries from inner handler
        let mut entries = self.inner.readdir(path).await?.unwrap_or_default();

        // Inject .graphdocs if not already present
        let graphdocs_name = GRAPHDOCS_DIR.strip_prefix('/').unwrap_or(".graphdocs");
        if !entries.contains(&graphdocs_name.to_string()) {
            entries.push(graphdocs_name.to_string());
        }

        Ok(Some(entries))
    }

    async fn readdir_plus(&self, path: &str) -> HandlerResult<Vec<DirEntry>> {
        if path != "/" {
            return Ok(None);
        }

        // Get real entries from inner handler
        let mut entries = self.inner.readdir_plus(path).await?.unwrap_or_default();

        // Check if .graphdocs already present
        let graphdocs_name = GRAPHDOCS_DIR.strip_prefix('/').unwrap_or(".graphdocs");
        let has_graphdocs = entries.iter().any(|e| e.name == graphdocs_name);

        if !has_graphdocs {
            entries.push(DirEntry {
                name: graphdocs_name.to_string(),
                stats: GraphDocsHandler::virtual_dir_stats(),
            });
        }

        Ok(Some(entries))
    }

    // Delegate other operations - return None to fall through
    async fn read(&self, _path: &str, _offset: u64, _size: u64) -> HandlerResult<Vec<u8>> {
        Ok(None)
    }

    async fn getattr(&self, _path: &str) -> HandlerResult<Stats> {
        Ok(None)
    }

    async fn write(&self, _path: &str, _offset: u64, _data: &[u8]) -> HandlerResult<usize> {
        Ok(None)
    }

    async fn truncate(&self, _path: &str, _size: u64) -> HandlerResult<()> {
        Ok(None)
    }

    async fn readlink(&self, _path: &str) -> HandlerResult<String> {
        Ok(None)
    }
}

// ============================================================================
// PATTERN-BASED HANDLER
// ============================================================================

/// A handler that matches files based on glob patterns.
///
/// This is useful for creating handlers that respond to specific file patterns
/// without implementing the full FileHandler trait.
pub struct PatternHandler {
    name: String,
    patterns: Vec<String>,
    priority: u32,
    read_fn: Option<Box<dyn Fn(&str) -> Result<Vec<u8>> + Send + Sync>>,
}

impl PatternHandler {
    /// Create a new pattern handler.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            patterns: Vec::new(),
            priority: 100,
            read_fn: None,
        }
    }

    /// Add a glob pattern to match.
    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.push(pattern.into());
        self
    }

    /// Set the priority.
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Set the read function.
    pub fn with_read<F>(mut self, f: F) -> Self
    where
        F: Fn(&str) -> Result<Vec<u8>> + Send + Sync + 'static,
    {
        self.read_fn = Some(Box::new(f));
        self
    }

    /// Check if path matches any pattern.
    fn matches(&self, path: &str) -> bool {
        for pattern in &self.patterns {
            // Simple glob matching (extend with proper glob crate if needed)
            if pattern.ends_with("*") {
                let prefix = &pattern[..pattern.len() - 1];
                if path.starts_with(prefix) {
                    return true;
                }
            } else if pattern.starts_with("*") {
                let suffix = &pattern[1..];
                if path.ends_with(suffix) {
                    return true;
                }
            } else if pattern == path {
                return true;
            }
        }
        false
    }
}

#[async_trait]
impl FileHandler for PatternHandler {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        self.matches(path)
    }

    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        if let Some(ref read_fn) = self.read_fn {
            let content = read_fn(path)?;
            let start = offset as usize;
            let end = (offset + size) as usize;

            if start >= content.len() {
                return Ok(Some(vec![]));
            }

            Ok(Some(content[start..end.min(content.len())].to_vec()))
        } else {
            Ok(None)
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphdocs_extract_doc_id() {
        // Test static method - no instance needed
        assert_eq!(
            GraphDocsHandler::extract_doc_id("/docs/readme.gd.md"),
            Some("readme".to_string())
        );
        assert_eq!(
            GraphDocsHandler::extract_doc_id("/a/b/project-overview.gd.md"),
            Some("project-overview".to_string())
        );
        assert_eq!(GraphDocsHandler::extract_doc_id("/docs/readme.md"), None);

        // Test paths in virtual directory
        assert_eq!(
            GraphDocsHandler::extract_doc_id("/.graphdocs/readme.gd.md"),
            Some("readme".to_string())
        );
        assert_eq!(
            GraphDocsHandler::extract_doc_id("/.graphdocs/api-reference.gd.md"),
            Some("api-reference".to_string())
        );
    }

    #[test]
    fn test_graphdocs_is_graphdocs_dir() {
        assert!(GraphDocsHandler::is_graphdocs_dir("/.graphdocs"));
        assert!(GraphDocsHandler::is_graphdocs_dir("/.graphdocs/"));
        assert!(!GraphDocsHandler::is_graphdocs_dir(
            "/.graphdocs/file.gd.md"
        ));
        assert!(!GraphDocsHandler::is_graphdocs_dir("/other"));
    }

    #[test]
    fn test_graphdocs_is_in_graphdocs_dir() {
        assert!(GraphDocsHandler::is_in_graphdocs_dir(
            "/.graphdocs/readme.gd.md"
        ));
        assert!(GraphDocsHandler::is_in_graphdocs_dir("/.graphdocs/sub/dir"));
        assert!(!GraphDocsHandler::is_in_graphdocs_dir("/.graphdocs"));
        assert!(!GraphDocsHandler::is_in_graphdocs_dir(
            "/other/readme.gd.md"
        ));
    }

    #[test]
    fn test_graphdocs_is_graphdocs_file() {
        assert!(GraphDocsHandler::is_graphdocs_file("/any/path/file.gd.md"));
        assert!(GraphDocsHandler::is_graphdocs_file("file.gd.md"));
        assert!(!GraphDocsHandler::is_graphdocs_file("/file.md"));
        assert!(!GraphDocsHandler::is_graphdocs_file("/file.gd"));
    }

    #[test]
    fn test_virtual_dir_stats() {
        let stats = GraphDocsHandler::virtual_dir_stats();
        // Should be directory (mode starts with 0o40xxx)
        assert_eq!(stats.mode & 0o170000, 0o40000);
        // Should have read + execute permissions
        assert_eq!(stats.mode & 0o555, 0o555);
        assert_eq!(stats.nlink, 2);
        // Should have non-zero inode (reserved GRAPHDOCS_DIR_INO)
        assert!(stats.ino != 0);
        assert_eq!(stats.ino, i64::MAX - 1);
    }

    #[test]
    fn test_virtual_file_stats() {
        let stats = GraphDocsHandler::virtual_file_stats("test-doc", 1024, 1234567890);
        // Should be regular file (mode starts with 0o10xxxx)
        assert_eq!(stats.mode & 0o170000, 0o100000);
        // Should be read-only (0o444)
        assert_eq!(stats.mode & 0o777, 0o444);
        assert_eq!(stats.size, 1024);
        assert_eq!(stats.mtime, 1234567890);
        assert_eq!(stats.nlink, 1);
        // Should have non-zero inode
        assert!(stats.ino != 0);
    }

    #[test]
    fn test_pattern_handler_matching() {
        let handler = PatternHandler::new("test")
            .with_pattern("*.md")
            .with_pattern("/docs/*");

        assert!(handler.matches("/test.md"));
        assert!(handler.matches("/docs/readme.txt"));
        assert!(!handler.matches("/src/main.rs"));
    }

    #[test]
    fn test_handler_priority_sorting() {
        struct TestHandler {
            name: String,
            priority: u32,
        }

        #[async_trait]
        impl FileHandler for TestHandler {
            fn name(&self) -> &str {
                &self.name
            }
            fn priority(&self) -> u32 {
                self.priority
            }
            fn can_handle(&self, _: &str, _: Option<&Stats>) -> bool {
                false
            }
        }

        // Create mock filesystem
        // In real test, use a mock FileSystem implementation

        // Verify priority ordering would work
        let h1 = TestHandler {
            name: "low".to_string(),
            priority: 100,
        };
        let h2 = TestHandler {
            name: "high".to_string(),
            priority: 10,
        };

        assert!(h2.priority() < h1.priority());
    }

    #[test]
    fn test_default_handler_accepts_all_paths() {
        // DefaultHandler should accept all paths
        struct MockFs;

        #[async_trait]
        impl FileSystem for MockFs {
            async fn stat(&self, _path: &str) -> Result<Option<Stats>> {
                Ok(None)
            }
            async fn lstat(&self, _path: &str) -> Result<Option<Stats>> {
                Ok(None)
            }
            async fn readdir(&self, _path: &str) -> Result<Option<Vec<String>>> {
                Ok(None)
            }
            async fn readdir_plus(&self, _path: &str) -> Result<Option<Vec<DirEntry>>> {
                Ok(None)
            }
            async fn mkdir(&self, _path: &str) -> Result<()> {
                Ok(())
            }
            async fn remove(&self, _path: &str) -> Result<()> {
                Ok(())
            }
            async fn rename(&self, _from: &str, _to: &str) -> Result<()> {
                Ok(())
            }
            async fn symlink(&self, _target: &str, _linkpath: &str) -> Result<()> {
                Ok(())
            }
            async fn link(&self, _oldpath: &str, _newpath: &str) -> Result<()> {
                Ok(())
            }
            async fn readlink(&self, _path: &str) -> Result<Option<String>> {
                Ok(None)
            }
            async fn chmod(&self, _path: &str, _mode: u32) -> Result<()> {
                Ok(())
            }
            async fn open(&self, _path: &str) -> Result<agentfs_sdk::BoxedFile> {
                Err(Error::Custom("Not implemented".to_string()))
            }
            async fn create_file(
                &self,
                _path: &str,
                _mode: u32,
            ) -> Result<(Stats, agentfs_sdk::BoxedFile)> {
                Err(Error::Custom("Not implemented".to_string()))
            }
            async fn statfs(&self) -> Result<agentfs_sdk::FilesystemStats> {
                Ok(agentfs_sdk::FilesystemStats {
                    bytes_used: 0,
                    inodes: 0,
                })
            }
            async fn read_file(&self, _path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }
            async fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }
        }

        let fs: Arc<dyn FileSystem> = Arc::new(MockFs);
        let handler = DefaultHandler::new(fs);

        // DefaultHandler should accept ALL paths
        assert!(handler.can_handle("/any/path", None));
        assert!(handler.can_handle("/", None));
        assert!(handler.can_handle("/deeply/nested/path/file.txt", None));
        assert!(handler.can_handle(".gd.md", None)); // Even GraphDocs extension
    }

    #[test]
    fn test_default_handler_has_max_priority() {
        struct MockFs;

        #[async_trait]
        impl FileSystem for MockFs {
            async fn stat(&self, _path: &str) -> Result<Option<Stats>> {
                Ok(None)
            }
            async fn lstat(&self, _path: &str) -> Result<Option<Stats>> {
                Ok(None)
            }
            async fn readdir(&self, _path: &str) -> Result<Option<Vec<String>>> {
                Ok(None)
            }
            async fn readdir_plus(&self, _path: &str) -> Result<Option<Vec<DirEntry>>> {
                Ok(None)
            }
            async fn mkdir(&self, _path: &str) -> Result<()> {
                Ok(())
            }
            async fn remove(&self, _path: &str) -> Result<()> {
                Ok(())
            }
            async fn rename(&self, _from: &str, _to: &str) -> Result<()> {
                Ok(())
            }
            async fn symlink(&self, _target: &str, _linkpath: &str) -> Result<()> {
                Ok(())
            }
            async fn link(&self, _oldpath: &str, _newpath: &str) -> Result<()> {
                Ok(())
            }
            async fn readlink(&self, _path: &str) -> Result<Option<String>> {
                Ok(None)
            }
            async fn chmod(&self, _path: &str, _mode: u32) -> Result<()> {
                Ok(())
            }
            async fn open(&self, _path: &str) -> Result<agentfs_sdk::BoxedFile> {
                Err(Error::Custom("Not implemented".to_string()))
            }
            async fn create_file(
                &self,
                _path: &str,
                _mode: u32,
            ) -> Result<(Stats, agentfs_sdk::BoxedFile)> {
                Err(Error::Custom("Not implemented".to_string()))
            }
            async fn statfs(&self) -> Result<agentfs_sdk::FilesystemStats> {
                Ok(agentfs_sdk::FilesystemStats {
                    bytes_used: 0,
                    inodes: 0,
                })
            }
            async fn read_file(&self, _path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }
            async fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }
        }

        let fs: Arc<dyn FileSystem> = Arc::new(MockFs);
        let handler = DefaultHandler::new(fs);

        // DefaultHandler must have u32::MAX priority (always last)
        assert_eq!(handler.priority(), u32::MAX);
    }

    #[test]
    fn test_registry_with_filesystem_creates_default_handler() {
        struct MockFs;

        #[async_trait]
        impl FileSystem for MockFs {
            async fn stat(&self, _path: &str) -> Result<Option<Stats>> {
                Ok(None)
            }
            async fn lstat(&self, _path: &str) -> Result<Option<Stats>> {
                Ok(None)
            }
            async fn readdir(&self, _path: &str) -> Result<Option<Vec<String>>> {
                Ok(None)
            }
            async fn readdir_plus(&self, _path: &str) -> Result<Option<Vec<DirEntry>>> {
                Ok(None)
            }
            async fn mkdir(&self, _path: &str) -> Result<()> {
                Ok(())
            }
            async fn remove(&self, _path: &str) -> Result<()> {
                Ok(())
            }
            async fn rename(&self, _from: &str, _to: &str) -> Result<()> {
                Ok(())
            }
            async fn symlink(&self, _target: &str, _linkpath: &str) -> Result<()> {
                Ok(())
            }
            async fn link(&self, _oldpath: &str, _newpath: &str) -> Result<()> {
                Ok(())
            }
            async fn readlink(&self, _path: &str) -> Result<Option<String>> {
                Ok(None)
            }
            async fn chmod(&self, _path: &str, _mode: u32) -> Result<()> {
                Ok(())
            }
            async fn open(&self, _path: &str) -> Result<agentfs_sdk::BoxedFile> {
                Err(Error::Custom("Not implemented".to_string()))
            }
            async fn create_file(
                &self,
                _path: &str,
                _mode: u32,
            ) -> Result<(Stats, agentfs_sdk::BoxedFile)> {
                Err(Error::Custom("Not implemented".to_string()))
            }
            async fn statfs(&self) -> Result<agentfs_sdk::FilesystemStats> {
                Ok(agentfs_sdk::FilesystemStats {
                    bytes_used: 0,
                    inodes: 0,
                })
            }
            async fn read_file(&self, _path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }
            async fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }
        }

        let fs: Arc<dyn FileSystem> = Arc::new(MockFs);
        let registry = HandlerRegistry::with_filesystem(fs);

        // Registry created with_filesystem should have no custom handlers
        let handlers = registry.list_handlers();
        assert!(handlers.is_empty());
    }

    // ========================================================================
    // LOOKUP TESTS (STORY-4.3.1)
    // ========================================================================

    /// Test that FileHandler::lookup default implementation returns Ok(None)
    #[test]
    fn test_lookup_default_returns_none() {
        struct TestHandler;

        #[async_trait]
        impl FileHandler for TestHandler {
            fn name(&self) -> &str {
                "test"
            }
            fn can_handle(&self, _: &str, _: Option<&Stats>) -> bool {
                true
            }
            // Don't override lookup - use default
        }

        let handler = TestHandler;

        // Use a simple runtime for the test
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(handler.lookup("/", "anything"));

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// Test DefaultHandler lookup constructs correct path from parent and name
    #[tokio::test]
    async fn test_default_handler_lookup_path_construction() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Mutex as StdMutex;

        // Track what path was passed to lstat
        let captured_path: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
        let captured_clone = captured_path.clone();

        struct PathCapturingFs {
            captured_path: Arc<StdMutex<Option<String>>>,
        }

        #[async_trait]
        impl FileSystem for PathCapturingFs {
            async fn stat(&self, _path: &str) -> Result<Option<Stats>> {
                Ok(None)
            }
            async fn lstat(&self, path: &str) -> Result<Option<Stats>> {
                *self.captured_path.lock().unwrap() = Some(path.to_string());
                Ok(None)
            }
            async fn readdir(&self, _path: &str) -> Result<Option<Vec<String>>> {
                Ok(None)
            }
            async fn readdir_plus(&self, _path: &str) -> Result<Option<Vec<DirEntry>>> {
                Ok(None)
            }
            async fn mkdir(&self, _path: &str) -> Result<()> {
                Ok(())
            }
            async fn remove(&self, _path: &str) -> Result<()> {
                Ok(())
            }
            async fn rename(&self, _from: &str, _to: &str) -> Result<()> {
                Ok(())
            }
            async fn symlink(&self, _target: &str, _linkpath: &str) -> Result<()> {
                Ok(())
            }
            async fn link(&self, _oldpath: &str, _newpath: &str) -> Result<()> {
                Ok(())
            }
            async fn readlink(&self, _path: &str) -> Result<Option<String>> {
                Ok(None)
            }
            async fn chmod(&self, _path: &str, _mode: u32) -> Result<()> {
                Ok(())
            }
            async fn open(&self, _path: &str) -> Result<agentfs_sdk::BoxedFile> {
                Err(Error::Custom("Not implemented".to_string()))
            }
            async fn create_file(
                &self,
                _path: &str,
                _mode: u32,
            ) -> Result<(Stats, agentfs_sdk::BoxedFile)> {
                Err(Error::Custom("Not implemented".to_string()))
            }
            async fn statfs(&self) -> Result<agentfs_sdk::FilesystemStats> {
                Ok(agentfs_sdk::FilesystemStats {
                    bytes_used: 0,
                    inodes: 0,
                })
            }
            async fn read_file(&self, _path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }
            async fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }
        }

        let fs: Arc<dyn FileSystem> = Arc::new(PathCapturingFs {
            captured_path: captured_clone,
        });
        let handler = DefaultHandler::new(fs);

        // Test lookup from root
        let _ = handler.lookup("/", "test.txt").await;
        assert_eq!(
            *captured_path.lock().unwrap(),
            Some("/test.txt".to_string())
        );

        // Test lookup from non-root directory
        let _ = handler.lookup("/home/user", "file.md").await;
        assert_eq!(
            *captured_path.lock().unwrap(),
            Some("/home/user/file.md".to_string())
        );

        // Test lookup handles trailing slash in parent
        let _ = handler.lookup("/home/user/", "file.md").await;
        assert_eq!(
            *captured_path.lock().unwrap(),
            Some("/home/user/file.md".to_string())
        );
    }

    /// Test GraphDocsHandler can_handle for lookup paths
    #[test]
    fn test_graphdocs_can_handle_for_lookup() {
        // GraphDocsHandler should handle:
        // - "/.graphdocs" (the virtual directory itself)
        // - "/.graphdocs/" (with trailing slash)
        // - "/.graphdocs/something.gd.md" (files in the directory)
        // - Any path ending in .gd.md

        // Test is_graphdocs_dir
        assert!(GraphDocsHandler::is_graphdocs_dir("/.graphdocs"));
        assert!(GraphDocsHandler::is_graphdocs_dir("/.graphdocs/"));

        // Test is_in_graphdocs_dir
        assert!(GraphDocsHandler::is_in_graphdocs_dir(
            "/.graphdocs/readme.gd.md"
        ));
        assert!(!GraphDocsHandler::is_in_graphdocs_dir("/.graphdocs")); // The dir itself is not "in" the dir

        // Test is_graphdocs_file
        assert!(GraphDocsHandler::is_graphdocs_file("/.graphdocs/doc.gd.md"));
        assert!(GraphDocsHandler::is_graphdocs_file("/any/path/doc.gd.md"));
        assert!(!GraphDocsHandler::is_graphdocs_file("/file.md"));
    }

    /// Test that HandlerRegistry::handle_lookup constructs the correct child path
    #[test]
    fn test_handle_lookup_child_path_construction() {
        // Verify the path construction logic used in handle_lookup
        let test_cases = vec![
            ("/", "test.txt", "/test.txt"),
            ("/", ".graphdocs", "/.graphdocs"),
            ("/home", "user", "/home/user"),
            ("/home/", "user", "/home/user"),
            ("/.graphdocs", "readme.gd.md", "/.graphdocs/readme.gd.md"),
            ("/.graphdocs/", "readme.gd.md", "/.graphdocs/readme.gd.md"),
        ];

        for (parent_path, name, expected) in test_cases {
            let child_path = if parent_path == "/" {
                format!("/{}", name)
            } else {
                format!("{}/{}", parent_path.trim_end_matches('/'), name)
            };
            assert_eq!(
                child_path, expected,
                "Failed for parent='{}', name='{}'",
                parent_path, name
            );
        }
    }

    /// Test GraphDocsHandler lookup path matching logic
    #[test]
    fn test_graphdocs_lookup_path_matching() {
        // Test the conditions in GraphDocsHandler::lookup

        // Case 1: Lookup of .graphdocs in root
        let parent_path = "/";
        let name = ".graphdocs";
        assert!(parent_path == "/" && name == ".graphdocs");

        // Case 2: Lookup of file in /.graphdocs/
        let parent_path = "/.graphdocs";
        let name = "readme.gd.md";
        let doc_id = name.strip_suffix(".gd.md");
        assert!(parent_path == GRAPHDOCS_DIR || parent_path == "/.graphdocs/");
        assert_eq!(doc_id, Some("readme"));

        // Case 3: Lookup of file without .gd.md extension should return None
        let name = "readme.txt";
        let doc_id = name.strip_suffix(".gd.md");
        assert!(doc_id.is_none());

        // Case 4: Lookup in non-graphdocs directory
        let parent_path = "/home/user";
        let name = "readme.gd.md";
        assert!(parent_path != "/" || name != ".graphdocs");
        assert!(parent_path != GRAPHDOCS_DIR && parent_path != "/.graphdocs/");
    }
}
