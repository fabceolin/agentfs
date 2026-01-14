//! DuckAgentFS - DuckDB-based filesystem implementation
//!
//! # CONCEPTUAL IMPLEMENTATION
//!
//! This file contains a conceptual implementation of DuckAgentFS using DuckDB
//! as the storage backend. During the implementation phase, changes may be made
//! to adapt to real-world requirements and DuckDB Rust bindings specifics.
//!
//! ## Key Differences from AgentFS (SQLite):
//!
//! 1. **Append-Only Journal Model**: Instead of mutating rows, all changes are
//!    appended to `fs_journal`. Current state is derived via `fs_current` view.
//!
//! 2. **Time-Travel**: Any historical state can be reconstructed by filtering
//!    journal events up to a specific `event_id`.
//!
//! 3. **Vector Search (VSS)**: Files can be indexed with embeddings for
//!    semantic search using DuckDB's VSS extension.
//!
//! 4. **Property Graphs (DuckPGQ)**: Code dependencies and document structure
//!    can be queried using graph patterns.
//!
//! 5. **Single Writer**: DuckDB uses single-writer semantics, so writes are
//!    serialized through a connection pool with write semaphore.

use crate::error::{Error, Result};
use async_trait::async_trait;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    BoxedFile, DirEntry, File, FileSystem, FilesystemStats, FsError, Stats, DEFAULT_DIR_MODE,
    DEFAULT_FILE_MODE, S_IFDIR, S_IFLNK, S_IFMT, S_IFREG,
};

// NOTE: These imports are conceptual. The actual DuckDB Rust crate may have
// different module structure. Adjust during implementation.
// use duckdb::{Connection, Transaction, params};

const ROOT_INO: i64 = 1;
const DEFAULT_CHUNK_SIZE: usize = 4096;
const DENTRY_CACHE_MAX_SIZE: usize = 10000;

// ============================================================================
// PLACEHOLDER TYPES
// ============================================================================
// These types represent what the DuckDB Rust bindings would provide.
// Replace with actual types from `duckdb` crate during implementation.

/// Placeholder for DuckDB connection
#[derive(Clone)]
pub struct DuckConnection {
    // In real implementation: duckdb::Connection
    _path: String,
}

/// Placeholder for DuckDB connection pool
#[derive(Clone)]
pub struct DuckConnectionPool {
    // Semaphore for single-writer semantics
    // write_semaphore: tokio::sync::Semaphore,
    _path: String,
}

impl DuckConnectionPool {
    pub async fn new(_path: &str) -> Result<Self> {
        Ok(Self {
            _path: _path.to_string(),
        })
    }

    pub async fn get_connection(&self) -> Result<DuckConnection> {
        Ok(DuckConnection {
            _path: self._path.clone(),
        })
    }

    pub async fn get_write_connection(&self) -> Result<DuckConnection> {
        // In real implementation: acquire write semaphore first
        Ok(DuckConnection {
            _path: self._path.clone(),
        })
    }
}

// ============================================================================
// EMBEDDING GENERATOR TRAIT
// ============================================================================

/// Trait for generating embeddings from text content.
///
/// Implement this trait to provide custom embedding generation for VSS.
#[async_trait]
pub trait EmbeddingGenerator: Send + Sync {
    /// Generate an embedding vector from text content.
    async fn generate(&self, content: &str) -> Result<Vec<f32>>;

    /// Get the model name for tracking purposes.
    fn model_name(&self) -> &str;

    /// Get the embedding dimension.
    fn dimension(&self) -> usize;
}

/// No-op embedding generator for when VSS is disabled.
pub struct NoOpEmbeddingGenerator;

#[async_trait]
impl EmbeddingGenerator for NoOpEmbeddingGenerator {
    async fn generate(&self, _content: &str) -> Result<Vec<f32>> {
        Ok(vec![])
    }

    fn model_name(&self) -> &str {
        "none"
    }

    fn dimension(&self) -> usize {
        0
    }
}

// ============================================================================
// DENTRY CACHE
// ============================================================================

/// LRU cache for directory entry lookups.
///
/// Maps (parent_ino, name) -> child_ino to avoid repeated database queries
/// during path resolution.
struct DentryCache {
    entries: Mutex<LruCache<(i64, String), i64>>,
}

impl DentryCache {
    fn new(max_size: usize) -> Self {
        Self {
            entries: Mutex::new(LruCache::new(
                NonZeroUsize::new(max_size).expect("cache size must be > 0"),
            )),
        }
    }

    fn get(&self, parent_ino: i64, name: &str) -> Option<i64> {
        self.entries
            .lock()
            .unwrap()
            .get(&(parent_ino, name.to_string()))
            .copied()
    }

    fn insert(&self, parent_ino: i64, name: &str, child_ino: i64) {
        self.entries
            .lock()
            .unwrap()
            .put((parent_ino, name.to_string()), child_ino);
    }

    fn remove(&self, parent_ino: i64, name: &str) {
        self.entries
            .lock()
            .unwrap()
            .pop(&(parent_ino, name.to_string()));
    }

    fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}

// ============================================================================
// DUCKAGENTFS
// ============================================================================

/// Configuration for DuckAgentFS
#[derive(Clone)]
pub struct DuckAgentFSConfig {
    /// Path to the DuckDB database file
    pub path: String,
    /// Chunk size for file data storage
    pub chunk_size: usize,
    /// Maximum entries in dentry cache
    pub dentry_cache_size: usize,
    /// Enable VSS (Vector Similarity Search)
    pub enable_vss: bool,
    /// Enable DuckPGQ (Property Graphs)
    pub enable_pgq: bool,
    /// Actor ID for audit trail
    pub actor_id: Option<String>,
    /// Session ID for audit trail
    pub session_id: Option<String>,
}

impl Default for DuckAgentFSConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            chunk_size: DEFAULT_CHUNK_SIZE,
            dentry_cache_size: DENTRY_CACHE_MAX_SIZE,
            enable_vss: false,
            enable_pgq: false,
            actor_id: None,
            session_id: None,
        }
    }
}

/// A filesystem backed by DuckDB with append-only journal model.
///
/// # Features
///
/// - **Time-Travel**: Query filesystem state at any point in history
/// - **Vector Search**: Semantic file search via embeddings
/// - **Property Graphs**: Code dependency analysis via DuckPGQ
/// - **Audit Trail**: All operations logged with actor/session tracking
///
/// # Example
///
/// ```rust,ignore
/// let config = DuckAgentFSConfig {
///     path: "/path/to/agent.duckdb".to_string(),
///     enable_vss: true,
///     ..Default::default()
/// };
/// let fs = DuckAgentFS::open(config).await?;
///
/// // Write a file
/// fs.write_file("/data/notes.txt", b"Hello, DuckAgentFS!").await?;
///
/// // Semantic search
/// let results = fs.search("meeting notes about project", 10).await?;
/// ```
#[derive(Clone)]
pub struct DuckAgentFS {
    pool: DuckConnectionPool,
    config: DuckAgentFSConfig,
    dentry_cache: Arc<DentryCache>,
    embedding_generator: Arc<dyn EmbeddingGenerator>,
}

impl DuckAgentFS {
    /// Open or create a DuckAgentFS database.
    pub async fn open(config: DuckAgentFSConfig) -> Result<Self> {
        let pool = DuckConnectionPool::new(&config.path).await?;

        let fs = Self {
            pool,
            dentry_cache: Arc::new(DentryCache::new(config.dentry_cache_size)),
            embedding_generator: Arc::new(NoOpEmbeddingGenerator),
            config,
        };

        fs.init_schema().await?;
        Ok(fs)
    }

    /// Open with custom embedding generator for VSS.
    pub async fn open_with_embeddings(
        config: DuckAgentFSConfig,
        embedding_generator: Arc<dyn EmbeddingGenerator>,
    ) -> Result<Self> {
        let pool = DuckConnectionPool::new(&config.path).await?;

        let fs = Self {
            pool,
            dentry_cache: Arc::new(DentryCache::new(config.dentry_cache_size)),
            embedding_generator,
            config,
        };

        fs.init_schema().await?;
        Ok(fs)
    }

    /// Initialize the database schema.
    async fn init_schema(&self) -> Result<()> {
        let _conn = self.pool.get_write_connection().await?;

        // NOTE: In real implementation, execute the DDL from schema/duckagentfs.sql
        // or embed the schema as a const string.
        //
        // conn.execute_batch(include_str!("../../../schema/duckagentfs.sql"))?;

        // Load extensions if enabled
        if self.config.enable_vss {
            // conn.execute("INSTALL vss; LOAD vss;", [])?;
        }

        if self.config.enable_pgq {
            // conn.execute("INSTALL duckpgq; LOAD duckpgq;", [])?;
        }

        Ok(())
    }

    // ========================================================================
    // JOURNAL OPERATIONS
    // ========================================================================

    /// Append a filesystem event to the journal.
    async fn append_journal_event(
        &self,
        conn: &DuckConnection,
        inode: i64,
        event_type: &str,
        parent: Option<i64>,
        name: Option<&str>,
        mode: Option<u32>,
        size: Option<i64>,
        nlink: Option<u32>,
        old_parent: Option<i64>,
        old_name: Option<&str>,
    ) -> Result<i64> {
        // NOTE: Conceptual SQL - adjust for actual DuckDB Rust bindings
        //
        // let event_id: i64 = conn.query_row(
        //     r#"
        //     INSERT INTO fs_journal (
        //         inode, event_type, parent, name, mode, size, nlink,
        //         old_parent, old_name, actor_id, session_id
        //     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        //     RETURNING event_id
        //     "#,
        //     params![
        //         inode, event_type, parent, name, mode, size, nlink,
        //         old_parent, old_name,
        //         self.config.actor_id.as_deref(),
        //         self.config.session_id.as_deref()
        //     ],
        //     |row| row.get(0)
        // )?;

        // Placeholder
        let _ = (conn, inode, event_type, parent, name, mode, size, nlink, old_parent, old_name);
        Ok(1)
    }

    /// Allocate a new inode number.
    async fn allocate_inode(&self, _conn: &DuckConnection) -> Result<i64> {
        // NOTE: Conceptual - use sequence
        // let ino: i64 = conn.query_row(
        //     "SELECT nextval('fs_inode_seq')",
        //     [],
        //     |row| row.get(0)
        // )?;
        Ok(2) // Placeholder
    }

    // ========================================================================
    // PATH RESOLUTION
    // ========================================================================

    /// Resolve a path to an inode, optionally following symlinks.
    async fn resolve_path(&self, path: &str, follow_symlinks: bool) -> Result<Option<i64>> {
        let path = Self::normalize_path(path);
        if path == "/" {
            return Ok(Some(ROOT_INO));
        }

        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_ino = ROOT_INO;
        let mut symlink_depth = 0;
        const MAX_SYMLINK_DEPTH: usize = 40;

        for (i, component) in components.iter().enumerate() {
            // Check cache first
            if let Some(child_ino) = self.dentry_cache.get(current_ino, component) {
                current_ino = child_ino;
                continue;
            }

            // Query database for entry
            let child = self.lookup_child(current_ino, component).await?;

            match child {
                None => return Ok(None),
                Some((child_ino, mode)) => {
                    self.dentry_cache.insert(current_ino, component, child_ino);

                    // Handle symlinks
                    let is_last = i == components.len() - 1;
                    let is_symlink = (mode & S_IFMT) == S_IFLNK;

                    if is_symlink && (follow_symlinks || !is_last) {
                        symlink_depth += 1;
                        if symlink_depth > MAX_SYMLINK_DEPTH {
                            return Err(Error::Fs(FsError::SymlinkLoop));
                        }

                        // Read symlink target and resolve recursively
                        if let Some(target) = self.read_symlink_target(child_ino).await? {
                            let resolved = if target.starts_with('/') {
                                self.resolve_path(&target, follow_symlinks).await?
                            } else {
                                // Relative symlink - resolve from parent
                                let parent_path = components[..i].join("/");
                                let full_target = format!("/{}/{}", parent_path, target);
                                self.resolve_path(&full_target, follow_symlinks).await?
                            };
                            current_ino = resolved.ok_or(Error::Fs(FsError::NotFound))?;
                        } else {
                            return Ok(None);
                        }
                    } else {
                        current_ino = child_ino;
                    }
                }
            }
        }

        Ok(Some(current_ino))
    }

    /// Look up a child entry in a directory.
    async fn lookup_child(&self, parent_ino: i64, name: &str) -> Result<Option<(i64, u32)>> {
        let _conn = self.pool.get_connection().await?;

        // NOTE: Conceptual SQL using fs_current view
        //
        // let result = conn.query_row(
        //     "SELECT inode, mode FROM fs_current WHERE parent = ? AND name = ?",
        //     params![parent_ino, name],
        //     |row| Ok((row.get(0)?, row.get(1)?))
        // ).optional()?;

        let _ = (parent_ino, name);
        Ok(None) // Placeholder
    }

    /// Read the target of a symlink.
    async fn read_symlink_target(&self, _ino: i64) -> Result<Option<String>> {
        // NOTE: Symlink targets stored in fs_data as first chunk
        Ok(None) // Placeholder
    }

    /// Get the parent inode and filename from a path.
    fn split_path(path: &str) -> (&str, &str) {
        let path = path.trim_end_matches('/');
        match path.rfind('/') {
            Some(pos) if pos == 0 => ("/", &path[1..]),
            Some(pos) => (&path[..pos], &path[pos + 1..]),
            None => ("/", path),
        }
    }

    /// Normalize a path (remove trailing slashes, handle . and ..)
    fn normalize_path(path: &str) -> String {
        let path = if path.is_empty() { "/" } else { path };
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            "/".to_string()
        } else {
            path.to_string()
        }
    }

    // ========================================================================
    // INODE OPERATIONS
    // ========================================================================

    /// Get stats for an inode.
    async fn stat_inode(&self, ino: i64) -> Result<Option<Stats>> {
        let _conn = self.pool.get_connection().await?;

        // NOTE: Conceptual SQL
        //
        // let row = conn.query_row(
        //     r#"
        //     SELECT inode, mode, nlink, uid, gid, size,
        //            EXTRACT(EPOCH FROM mtime)::BIGINT as mtime
        //     FROM fs_current
        //     WHERE inode = ?
        //     "#,
        //     params![ino],
        //     |row| Ok(Stats {
        //         ino: row.get(0)?,
        //         mode: row.get(1)?,
        //         nlink: row.get(2)?,
        //         uid: row.get(3)?,
        //         gid: row.get(4)?,
        //         size: row.get(5)?,
        //         atime: row.get::<_, i64>(6)?,
        //         mtime: row.get::<_, i64>(6)?,
        //         ctime: row.get::<_, i64>(6)?,
        //     })
        // ).optional()?;

        let _ = ino;
        Ok(None) // Placeholder
    }

    // ========================================================================
    // TIME-TRAVEL
    // ========================================================================

    /// Get filesystem state at a specific event ID.
    ///
    /// Returns a new DuckAgentFS instance that presents the filesystem
    /// as it was at the given event.
    pub async fn snapshot_at(&self, _event_id: i64) -> Result<DuckAgentFSSnapshot> {
        Ok(DuckAgentFSSnapshot {
            fs: self.clone(),
            event_id: _event_id,
        })
    }

    /// Get the current event ID (latest state).
    pub async fn current_event_id(&self) -> Result<i64> {
        let _conn = self.pool.get_connection().await?;
        // let event_id: i64 = conn.query_row(
        //     "SELECT MAX(event_id) FROM fs_journal",
        //     [],
        //     |row| row.get(0)
        // )?;
        Ok(0) // Placeholder
    }

    // ========================================================================
    // VECTOR SEARCH (VSS)
    // ========================================================================

    /// Search files by semantic similarity.
    ///
    /// # Arguments
    ///
    /// * `query` - Natural language query
    /// * `limit` - Maximum number of results
    ///
    /// # Returns
    ///
    /// Vector of (path, similarity_score, preview) tuples.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, f32, String)>> {
        if !self.config.enable_vss {
            return Err(Error::Custom("VSS not enabled".into()));
        }

        // Generate query embedding
        let query_embedding = self.embedding_generator.generate(query).await?;

        let _conn = self.pool.get_connection().await?;

        // NOTE: Conceptual SQL using VSS
        //
        // let results = conn.query_map(
        //     r#"
        //     SELECT
        //         t.path,
        //         array_cosine_similarity(e.embedding, ?::FLOAT[]) as similarity,
        //         LEFT(CAST(d.data AS VARCHAR), 500) as preview
        //     FROM fs_embeddings e
        //     JOIN fs_tree t ON t.inode = e.inode
        //     LEFT JOIN fs_data d ON d.inode = e.inode AND d.chunk_idx = 0
        //     ORDER BY similarity DESC
        //     LIMIT ?
        //     "#,
        //     params![query_embedding, limit],
        //     |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        // )?;

        let _ = (query_embedding, limit);
        Ok(vec![]) // Placeholder
    }

    /// Update embedding for a file.
    async fn update_embedding(&self, ino: i64, content: &[u8]) -> Result<()> {
        if !self.config.enable_vss {
            return Ok(());
        }

        // Only embed text-like content
        if let Ok(text) = std::str::from_utf8(content) {
            let embedding = self.embedding_generator.generate(text).await?;

            let _conn = self.pool.get_write_connection().await?;

            // NOTE: Conceptual SQL
            //
            // conn.execute(
            //     r#"
            //     INSERT OR REPLACE INTO fs_embeddings (inode, embedding, model, content_hash)
            //     VALUES (?, ?, ?, md5(?))
            //     "#,
            //     params![ino, embedding, self.embedding_generator.model_name(), text]
            // )?;

            let _ = (ino, embedding);
        }

        Ok(())
    }
}

// ============================================================================
// FILESYSTEM TRAIT IMPLEMENTATION
// ============================================================================

#[async_trait]
impl FileSystem for DuckAgentFS {
    async fn stat(&self, path: &str) -> Result<Option<Stats>> {
        match self.resolve_path(path, true).await? {
            Some(ino) => self.stat_inode(ino).await,
            None => Ok(None),
        }
    }

    async fn lstat(&self, path: &str) -> Result<Option<Stats>> {
        match self.resolve_path(path, false).await? {
            Some(ino) => self.stat_inode(ino).await,
            None => Ok(None),
        }
    }

    async fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let ino = match self.resolve_path(path, true).await? {
            Some(ino) => ino,
            None => return Ok(None),
        };

        let stats = match self.stat_inode(ino).await? {
            Some(s) => s,
            None => return Ok(None),
        };

        if !stats.is_file() {
            return Err(Error::Fs(FsError::IsADirectory));
        }

        // Read all chunks
        let _conn = self.pool.get_connection().await?;

        // NOTE: Conceptual SQL
        //
        // let mut data = Vec::new();
        // let mut stmt = conn.prepare(
        //     "SELECT data FROM fs_data WHERE inode = ? ORDER BY chunk_idx"
        // )?;
        // let rows = stmt.query_map(params![ino], |row| {
        //     let chunk: Vec<u8> = row.get(0)?;
        //     Ok(chunk)
        // })?;
        // for chunk in rows {
        //     data.extend(chunk?);
        // }
        // data.truncate(stats.size as usize);

        let _ = ino;
        Ok(Some(vec![])) // Placeholder
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let (parent_path, name) = Self::split_path(path);

        // Ensure parent directory exists
        let parent_ino = match self.resolve_path(parent_path, true).await? {
            Some(ino) => ino,
            None => {
                // Create parent directories recursively
                self.mkdir_recursive(parent_path).await?;
                self.resolve_path(parent_path, true)
                    .await?
                    .ok_or(Error::Fs(FsError::NotFound))?
            }
        };

        let conn = self.pool.get_write_connection().await?;

        // Check if file exists
        let existing = self.lookup_child(parent_ino, name).await?;

        let ino = match existing {
            Some((ino, mode)) => {
                if (mode & S_IFMT) == S_IFDIR {
                    return Err(Error::Fs(FsError::IsADirectory));
                }

                // Update existing file - append 'update' event
                self.append_journal_event(
                    &conn,
                    ino,
                    "update",
                    Some(parent_ino),
                    Some(name),
                    Some(DEFAULT_FILE_MODE),
                    Some(data.len() as i64),
                    None,
                    None,
                    None,
                )
                .await?;

                ino
            }
            None => {
                // Create new file
                let ino = self.allocate_inode(&conn).await?;

                self.append_journal_event(
                    &conn,
                    ino,
                    "create",
                    Some(parent_ino),
                    Some(name),
                    Some(DEFAULT_FILE_MODE),
                    Some(data.len() as i64),
                    Some(1),
                    None,
                    None,
                )
                .await?;

                self.dentry_cache.insert(parent_ino, name, ino);
                ino
            }
        };

        // Write data chunks
        self.write_data_chunks(&conn, ino, data).await?;

        // Update embedding if VSS enabled
        self.update_embedding(ino, data).await?;

        Ok(())
    }

    async fn readdir(&self, path: &str) -> Result<Option<Vec<String>>> {
        let ino = match self.resolve_path(path, true).await? {
            Some(ino) => ino,
            None => return Ok(None),
        };

        let stats = match self.stat_inode(ino).await? {
            Some(s) => s,
            None => return Ok(None),
        };

        if !stats.is_directory() {
            return Err(Error::Fs(FsError::NotADirectory));
        }

        let _conn = self.pool.get_connection().await?;

        // NOTE: Conceptual SQL
        //
        // let mut entries = vec![".".to_string(), "..".to_string()];
        // let mut stmt = conn.prepare(
        //     "SELECT name FROM fs_current WHERE parent = ? AND inode != ?"
        // )?;
        // let rows = stmt.query_map(params![ino, ino], |row| row.get(0))?;
        // for name in rows {
        //     entries.push(name?);
        // }

        let _ = ino;
        Ok(Some(vec![".".to_string(), "..".to_string()])) // Placeholder
    }

    async fn readdir_plus(&self, path: &str) -> Result<Option<Vec<DirEntry>>> {
        let ino = match self.resolve_path(path, true).await? {
            Some(ino) => ino,
            None => return Ok(None),
        };

        let stats = match self.stat_inode(ino).await? {
            Some(s) => s,
            None => return Ok(None),
        };

        if !stats.is_directory() {
            return Err(Error::Fs(FsError::NotADirectory));
        }

        let parent_stats = if ino == ROOT_INO {
            stats.clone()
        } else {
            // Get parent stats
            self.stat_inode(ROOT_INO).await?.unwrap_or(stats.clone())
        };

        let mut entries = vec![
            DirEntry {
                name: ".".to_string(),
                stats: stats.clone(),
            },
            DirEntry {
                name: "..".to_string(),
                stats: parent_stats,
            },
        ];

        // NOTE: Query children with stats
        // ... add children entries

        let _ = ino;
        Ok(Some(entries))
    }

    async fn mkdir(&self, path: &str) -> Result<()> {
        let (parent_path, name) = Self::split_path(path);

        let parent_ino = self
            .resolve_path(parent_path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        // Check parent is a directory
        let parent_stats = self
            .stat_inode(parent_ino)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        if !parent_stats.is_directory() {
            return Err(Error::Fs(FsError::NotADirectory));
        }

        // Check if name already exists
        if self.lookup_child(parent_ino, name).await?.is_some() {
            return Err(Error::Fs(FsError::AlreadyExists));
        }

        let conn = self.pool.get_write_connection().await?;
        let ino = self.allocate_inode(&conn).await?;

        self.append_journal_event(
            &conn,
            ino,
            "create",
            Some(parent_ino),
            Some(name),
            Some(DEFAULT_DIR_MODE),
            Some(0),
            Some(2),
            None,
            None,
        )
        .await?;

        self.dentry_cache.insert(parent_ino, name, ino);

        Ok(())
    }

    async fn remove(&self, path: &str) -> Result<()> {
        if path == "/" {
            return Err(Error::Fs(FsError::RootOperation));
        }

        let (parent_path, name) = Self::split_path(path);

        let parent_ino = self
            .resolve_path(parent_path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        let (ino, mode) = self
            .lookup_child(parent_ino, name)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        // Check if directory is empty
        if (mode & S_IFMT) == S_IFDIR {
            let children = self.readdir(path).await?;
            if let Some(entries) = children {
                if entries.len() > 2 {
                    // . and ..
                    return Err(Error::Fs(FsError::NotEmpty));
                }
            }
        }

        let conn = self.pool.get_write_connection().await?;

        self.append_journal_event(
            &conn,
            ino,
            "delete",
            Some(parent_ino),
            Some(name),
            None,
            None,
            None,
            None,
            None,
        )
        .await?;

        self.dentry_cache.remove(parent_ino, name);

        Ok(())
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<()> {
        let ino = self
            .resolve_path(path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        let stats = self
            .stat_inode(ino)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        // Preserve file type, update permissions
        let new_mode = (stats.mode & S_IFMT) | (mode & 0o7777);

        let conn = self.pool.get_write_connection().await?;

        self.append_journal_event(
            &conn,
            ino,
            "chmod",
            None,
            None,
            Some(new_mode),
            None,
            None,
            None,
            None,
        )
        .await?;

        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let (from_parent_path, from_name) = Self::split_path(from);
        let (to_parent_path, to_name) = Self::split_path(to);

        let from_parent_ino = self
            .resolve_path(from_parent_path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        let to_parent_ino = self
            .resolve_path(to_parent_path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        let (ino, _mode) = self
            .lookup_child(from_parent_ino, from_name)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        // Check target doesn't exist (or handle overwrite)
        if let Some((target_ino, target_mode)) =
            self.lookup_child(to_parent_ino, to_name).await?
        {
            // Can't rename directory over file or vice versa
            if (_mode & S_IFMT) != (target_mode & S_IFMT) {
                return Err(Error::Fs(FsError::InvalidRename));
            }

            // Delete target
            let conn = self.pool.get_write_connection().await?;
            self.append_journal_event(
                &conn,
                target_ino,
                "delete",
                Some(to_parent_ino),
                Some(to_name),
                None,
                None,
                None,
                None,
                None,
            )
            .await?;
        }

        let conn = self.pool.get_write_connection().await?;

        self.append_journal_event(
            &conn,
            ino,
            "rename",
            Some(to_parent_ino),
            Some(to_name),
            None,
            None,
            None,
            Some(from_parent_ino),
            Some(from_name),
        )
        .await?;

        // Update cache
        self.dentry_cache.remove(from_parent_ino, from_name);
        self.dentry_cache.insert(to_parent_ino, to_name, ino);

        Ok(())
    }

    async fn symlink(&self, target: &str, linkpath: &str) -> Result<()> {
        let (parent_path, name) = Self::split_path(linkpath);

        let parent_ino = self
            .resolve_path(parent_path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        if self.lookup_child(parent_ino, name).await?.is_some() {
            return Err(Error::Fs(FsError::AlreadyExists));
        }

        let conn = self.pool.get_write_connection().await?;
        let ino = self.allocate_inode(&conn).await?;

        self.append_journal_event(
            &conn,
            ino,
            "create",
            Some(parent_ino),
            Some(name),
            Some(S_IFLNK | 0o777),
            Some(target.len() as i64),
            Some(1),
            None,
            None,
        )
        .await?;

        // Store symlink target as file data
        self.write_data_chunks(&conn, ino, target.as_bytes())
            .await?;

        self.dentry_cache.insert(parent_ino, name, ino);

        Ok(())
    }

    async fn link(&self, oldpath: &str, newpath: &str) -> Result<()> {
        let old_ino = self
            .resolve_path(oldpath, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        let old_stats = self
            .stat_inode(old_ino)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        if old_stats.is_directory() {
            return Err(Error::Fs(FsError::IsADirectory));
        }

        let (parent_path, name) = Self::split_path(newpath);

        let parent_ino = self
            .resolve_path(parent_path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        if self.lookup_child(parent_ino, name).await?.is_some() {
            return Err(Error::Fs(FsError::AlreadyExists));
        }

        let conn = self.pool.get_write_connection().await?;

        // Create link event (same inode, new name)
        self.append_journal_event(
            &conn,
            old_ino,
            "create",
            Some(parent_ino),
            Some(name),
            Some(old_stats.mode),
            Some(old_stats.size),
            Some(old_stats.nlink + 1),
            None,
            None,
        )
        .await?;

        self.dentry_cache.insert(parent_ino, name, old_ino);

        Ok(())
    }

    async fn readlink(&self, path: &str) -> Result<Option<String>> {
        let ino = match self.resolve_path(path, false).await? {
            Some(ino) => ino,
            None => return Ok(None),
        };

        let stats = match self.stat_inode(ino).await? {
            Some(s) => s,
            None => return Ok(None),
        };

        if !stats.is_symlink() {
            return Err(Error::Fs(FsError::NotASymlink));
        }

        self.read_symlink_target(ino).await
    }

    async fn statfs(&self) -> Result<FilesystemStats> {
        let _conn = self.pool.get_connection().await?;

        // NOTE: Conceptual SQL
        //
        // let (inodes, bytes): (u64, u64) = conn.query_row(
        //     r#"
        //     SELECT
        //         (SELECT COUNT(DISTINCT inode) FROM fs_current),
        //         (SELECT COALESCE(SUM(LENGTH(data)), 0) FROM fs_data)
        //     "#,
        //     [],
        //     |row| Ok((row.get(0)?, row.get(1)?))
        // )?;

        Ok(FilesystemStats {
            inodes: 0,
            bytes_used: 0,
        })
    }

    async fn open(&self, path: &str) -> Result<BoxedFile> {
        let ino = self
            .resolve_path(path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        let stats = self
            .stat_inode(ino)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        if !stats.is_file() {
            return Err(Error::Fs(FsError::IsADirectory));
        }

        Ok(Arc::new(DuckAgentFSFile {
            pool: self.pool.clone(),
            ino,
            chunk_size: self.config.chunk_size,
        }))
    }

    async fn create_file(&self, path: &str, mode: u32) -> Result<(Stats, BoxedFile)> {
        let (parent_path, name) = Self::split_path(path);

        let parent_ino = self
            .resolve_path(parent_path, true)
            .await?
            .ok_or(Error::Fs(FsError::NotFound))?;

        if self.lookup_child(parent_ino, name).await?.is_some() {
            return Err(Error::Fs(FsError::AlreadyExists));
        }

        let conn = self.pool.get_write_connection().await?;
        let ino = self.allocate_inode(&conn).await?;

        let file_mode = (mode & 0o7777) | S_IFREG;

        self.append_journal_event(
            &conn,
            ino,
            "create",
            Some(parent_ino),
            Some(name),
            Some(file_mode),
            Some(0),
            Some(1),
            None,
            None,
        )
        .await?;

        self.dentry_cache.insert(parent_ino, name, ino);

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let stats = Stats {
            ino,
            mode: file_mode,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            atime: now,
            mtime: now,
            ctime: now,
        };

        let file = Arc::new(DuckAgentFSFile {
            pool: self.pool.clone(),
            ino,
            chunk_size: self.config.chunk_size,
        });

        Ok((stats, file))
    }
}

// ============================================================================
// HELPER METHODS
// ============================================================================

impl DuckAgentFS {
    /// Recursively create parent directories.
    async fn mkdir_recursive(&self, path: &str) -> Result<()> {
        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let mut current_path = String::new();
        for component in components {
            current_path.push('/');
            current_path.push_str(component);

            if self.resolve_path(&current_path, true).await?.is_none() {
                self.mkdir(&current_path).await?;
            }
        }

        Ok(())
    }

    /// Write data to file in chunks.
    async fn write_data_chunks(
        &self,
        _conn: &DuckConnection,
        _ino: i64,
        _data: &[u8],
    ) -> Result<()> {
        // NOTE: Conceptual implementation
        //
        // // Delete existing chunks
        // conn.execute("DELETE FROM fs_data WHERE inode = ?", params![ino])?;
        //
        // // Write new chunks
        // for (idx, chunk) in data.chunks(self.config.chunk_size).enumerate() {
        //     conn.execute(
        //         "INSERT INTO fs_data (inode, chunk_idx, data) VALUES (?, ?, ?)",
        //         params![ino, idx, chunk]
        //     )?;
        // }

        Ok(())
    }
}

// ============================================================================
// FILE HANDLE IMPLEMENTATION
// ============================================================================

/// An open file handle for DuckAgentFS.
pub struct DuckAgentFSFile {
    pool: DuckConnectionPool,
    ino: i64,
    chunk_size: usize,
}

#[async_trait]
impl File for DuckAgentFSFile {
    async fn pread(&self, offset: u64, size: u64) -> Result<Vec<u8>> {
        let _conn = self.pool.get_connection().await?;
        let chunk_size = self.chunk_size as u64;
        let start_chunk = offset / chunk_size;
        let _end_chunk = (offset + size).saturating_sub(1) / chunk_size;

        // NOTE: Conceptual implementation similar to AgentFSFile
        //
        // Query chunks from fs_data and assemble result

        let _ = start_chunk;
        Ok(vec![]) // Placeholder
    }

    async fn pwrite(&self, offset: u64, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let _conn = self.pool.get_write_connection().await?;

        // NOTE: Conceptual implementation
        // - Read existing data
        // - Merge with new data
        // - Write chunks
        // - Append 'update' event to journal

        let _ = (offset, data);
        Ok(())
    }

    async fn truncate(&self, size: u64) -> Result<()> {
        let _conn = self.pool.get_write_connection().await?;

        // NOTE: Conceptual implementation
        // - Delete chunks beyond new size
        // - Truncate final chunk if needed
        // - Append 'update' event to journal

        let _ = size;
        Ok(())
    }

    async fn fsync(&self) -> Result<()> {
        // DuckDB handles persistence automatically
        // Optionally call CHECKPOINT here
        Ok(())
    }

    async fn fstat(&self) -> Result<Stats> {
        let _conn = self.pool.get_connection().await?;

        // NOTE: Conceptual - query fs_current for this inode
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        Ok(Stats {
            ino: self.ino,
            mode: DEFAULT_FILE_MODE,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            atime: now,
            mtime: now,
            ctime: now,
        })
    }
}

// ============================================================================
// SNAPSHOT (TIME-TRAVEL)
// ============================================================================

/// A read-only snapshot of DuckAgentFS at a specific point in time.
///
/// This struct provides the same FileSystem interface but queries
/// historical state instead of current state.
pub struct DuckAgentFSSnapshot {
    fs: DuckAgentFS,
    event_id: i64,
}

impl DuckAgentFSSnapshot {
    /// Get the event ID this snapshot represents.
    pub fn event_id(&self) -> i64 {
        self.event_id
    }

    /// Get stats for an inode at the snapshot time.
    async fn stat_inode_at(&self, ino: i64) -> Result<Option<Stats>> {
        let _conn = self.fs.pool.get_connection().await?;

        // NOTE: Conceptual SQL with event_id filter
        //
        // WITH ranked AS (
        //     SELECT *,
        //            ROW_NUMBER() OVER (PARTITION BY inode ORDER BY event_id DESC) as rn
        //     FROM fs_journal
        //     WHERE event_id <= ? AND event_type != 'delete'
        // )
        // SELECT ... FROM ranked WHERE rn = 1 AND inode = ?

        let _ = ino;
        Ok(None)
    }
}

// Implement read-only FileSystem for snapshot
#[async_trait]
impl FileSystem for DuckAgentFSSnapshot {
    async fn stat(&self, path: &str) -> Result<Option<Stats>> {
        // Use snapshot-aware resolution
        match self.fs.resolve_path(path, true).await? {
            Some(ino) => self.stat_inode_at(ino).await,
            None => Ok(None),
        }
    }

    async fn lstat(&self, path: &str) -> Result<Option<Stats>> {
        match self.fs.resolve_path(path, false).await? {
            Some(ino) => self.stat_inode_at(ino).await,
            None => Ok(None),
        }
    }

    async fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        // Read file data at snapshot time
        // Implementation would filter fs_data by event_id
        self.fs.read_file(path).await
    }

    async fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn readdir(&self, path: &str) -> Result<Option<Vec<String>>> {
        // Query directory contents at snapshot time
        self.fs.readdir(path).await
    }

    async fn readdir_plus(&self, path: &str) -> Result<Option<Vec<DirEntry>>> {
        self.fs.readdir_plus(path).await
    }

    async fn mkdir(&self, _path: &str) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn remove(&self, _path: &str) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn chmod(&self, _path: &str, _mode: u32) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn rename(&self, _from: &str, _to: &str) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn symlink(&self, _target: &str, _linkpath: &str) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn link(&self, _oldpath: &str, _newpath: &str) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn readlink(&self, path: &str) -> Result<Option<String>> {
        self.fs.readlink(path).await
    }

    async fn statfs(&self) -> Result<FilesystemStats> {
        // Return stats at snapshot time
        self.fs.statfs().await
    }

    async fn open(&self, path: &str) -> Result<BoxedFile> {
        self.fs.open(path).await
    }

    async fn create_file(&self, _path: &str, _mode: u32) -> Result<(Stats, BoxedFile)> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(DuckAgentFS::normalize_path(""), "/");
        assert_eq!(DuckAgentFS::normalize_path("/"), "/");
        assert_eq!(DuckAgentFS::normalize_path("/foo/"), "/foo");
        assert_eq!(DuckAgentFS::normalize_path("/foo/bar"), "/foo/bar");
    }

    #[test]
    fn test_split_path() {
        assert_eq!(DuckAgentFS::split_path("/foo"), ("/", "foo"));
        assert_eq!(DuckAgentFS::split_path("/foo/bar"), ("/foo", "bar"));
        assert_eq!(DuckAgentFS::split_path("/a/b/c"), ("/a/b", "c"));
    }
}
