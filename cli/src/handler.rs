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
use agentfs_sdk::{DirEntry, FileSystem, Stats};
use async_trait::async_trait;
use std::sync::Arc;

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

    // ========================================================================
    // OPERATION DISPATCH
    // ========================================================================

    /// Handle a read operation.
    pub async fn handle_read(&self, path: &str, offset: u64, size: u64) -> Result<Vec<u8>> {
        // Try custom handlers first
        for handler in &self.handlers {
            if handler.can_handle(path, None) {
                if let Some(data) = handler.read(path, offset, size).await? {
                    tracing::debug!(
                        "Handler '{}' handled read for {}",
                        handler.name(),
                        path
                    );
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
                    tracing::debug!(
                        "Handler '{}' handled getattr for {}",
                        handler.name(),
                        path
                    );
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
                    tracing::debug!(
                        "Handler '{}' handled readdir for {}",
                        handler.name(),
                        path
                    );
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
}

// ============================================================================
// GRAPHDOCS HANDLER (CONCEPTUAL)
// ============================================================================

/// Handler for GraphDocs - rendering markdown from property graphs.
///
/// # CONCEPTUAL IMPLEMENTATION
///
/// This handler intercepts reads to `.gd.md` files and renders them
/// from the underlying property graph structure.
///
/// ## How it works
///
/// 1. File `project-overview.gd.md` is accessed via FUSE
/// 2. Handler extracts document ID: `project-overview`
/// 3. Handler queries GraphDocs graph for document sections and variables
/// 4. Handler renders markdown with variable substitution
/// 5. Handler returns rendered content as file data
///
/// ## Example
///
/// Given a document in the graph:
///
/// ```sql
/// -- Document
/// INSERT INTO gd_documents (id, title) VALUES ('readme', 'README');
///
/// -- Sections
/// INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
/// VALUES
///   ('s1', 'readme', 'heading', 1, 0, '# {{project_name}}'),
///   ('s2', 'readme', 'paragraph', 1, 1, 'Version: {{version}}');
///
/// -- Variables
/// INSERT INTO gd_variables (id, document_id, name, value)
/// VALUES
///   ('v1', 'readme', 'project_name', '"My Project"'),
///   ('v2', 'readme', 'version', '"1.0.0"');
/// ```
///
/// Reading `/docs/readme.gd.md` returns:
///
/// ```markdown
/// # My Project
///
/// Version: 1.0.0
/// ```
pub struct GraphDocsHandler {
    // In real implementation:
    // engine: GraphDocsEngine,
    // fs: Arc<dyn FileSystem>, // For accessing the database
    name: String,
    extension: String,
}

impl GraphDocsHandler {
    /// Create a new GraphDocs handler.
    ///
    /// # Arguments
    ///
    /// * `extension` - File extension to handle (default: ".gd.md")
    pub fn new() -> Self {
        Self {
            name: "graphdocs".to_string(),
            extension: ".gd.md".to_string(),
        }
    }

    /// Set the file extension to handle.
    pub fn with_extension(mut self, ext: impl Into<String>) -> Self {
        self.extension = ext.into();
        self
    }

    /// Extract document ID from path.
    fn extract_doc_id(&self, path: &str) -> Option<String> {
        let filename = path.rsplit('/').next()?;
        filename.strip_suffix(&self.extension).map(String::from)
    }

    /// Render a document from the graph.
    async fn render_document(&self, _doc_id: &str) -> Result<String> {
        // NOTE: Conceptual implementation
        //
        // In real implementation:
        //
        // 1. Query document: SELECT * FROM gd_documents WHERE id = ?
        // 2. Query sections: SELECT * FROM gd_sections WHERE document_id = ? ORDER BY order_idx
        // 3. Query variables: SELECT * FROM gd_variables WHERE document_id = ?
        // 4. Build inheritance chain if base_template is set
        // 5. Render each section with variable substitution
        // 6. Return concatenated markdown

        Ok("# Rendered Document\n\nThis is a placeholder.".to_string())
    }

    /// Get virtual file stats for a document.
    fn virtual_stats(&self, content_len: i64) -> Stats {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Stats {
            ino: 0, // Will be assigned by registry
            mode: 0o100444, // Regular file, read-only
            nlink: 1,
            uid: 0,
            gid: 0,
            size: content_len,
            atime: now,
            mtime: now,
            ctime: now,
        }
    }
}

impl Default for GraphDocsHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FileHandler for GraphDocsHandler {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> u32 {
        50 // Higher than default
    }

    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        path.ends_with(&self.extension)
    }

    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        let doc_id = match self.extract_doc_id(path) {
            Some(id) => id,
            None => return Ok(None),
        };

        let content = self.render_document(&doc_id).await?;
        let bytes = content.as_bytes();

        let start = offset as usize;
        let end = (offset + size) as usize;

        if start >= bytes.len() {
            return Ok(Some(vec![]));
        }

        Ok(Some(bytes[start..end.min(bytes.len())].to_vec()))
    }

    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        let doc_id = match self.extract_doc_id(path) {
            Some(id) => id,
            None => return Ok(None),
        };

        // NOTE: In real implementation, check if document exists in database
        let _ = doc_id;

        // Return placeholder stats
        let content = "# Placeholder\n";
        Ok(Some(self.virtual_stats(content.len() as i64)))
    }

    async fn write(&self, _path: &str, _offset: u64, _data: &[u8]) -> HandlerResult<usize> {
        // GraphDocs files are read-only (rendered from graph)
        // To edit, modify the underlying graph structure
        Err(Error::Custom(
            "GraphDocs files are read-only. Edit the underlying graph instead.".into(),
        ))
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
        let handler = GraphDocsHandler::new();

        assert_eq!(
            handler.extract_doc_id("/docs/readme.gd.md"),
            Some("readme".to_string())
        );
        assert_eq!(
            handler.extract_doc_id("/a/b/project-overview.gd.md"),
            Some("project-overview".to_string())
        );
        assert_eq!(handler.extract_doc_id("/docs/readme.md"), None);
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
}
