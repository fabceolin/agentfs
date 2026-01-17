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
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    BoxedFile, DirEntry, File, FileSystem, FilesystemStats, FsError, Stats, DEFAULT_DIR_MODE,
    DEFAULT_FILE_MODE, S_IFDIR, S_IFLNK, S_IFMT, S_IFREG,
};

use duckdb::{params, Connection};

const ROOT_INO: i64 = 1;
const DEFAULT_CHUNK_SIZE: usize = 4096;
const DENTRY_CACHE_MAX_SIZE: usize = 10000;

// ============================================================================
// CONNECTION POOL
// ============================================================================

/// Connection pool for DuckDB with single-writer semantics.
///
/// DuckDB Connection is not Send+Sync, so we use std::sync::Mutex and
/// spawn_blocking for async operations. This serializes all database access
/// which is fine for DuckDB's single-writer model.
#[derive(Clone)]
pub struct DuckConnectionPool {
    conn: Arc<Mutex<Connection>>,
}

impl DuckConnectionPool {
    /// Create a new connection pool.
    ///
    /// Opens a DuckDB database at the given path (or in-memory if `:memory:`).
    pub fn new(path: &str) -> Result<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory().map_err(|e| Error::Custom(e.to_string()))?
        } else {
            Connection::open(path).map_err(|e| Error::Custom(e.to_string()))?
        };

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Get a connection for read operations.
    ///
    /// Returns a guard to the connection. Use within spawn_blocking for async.
    pub fn get_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|e| Error::Custom(format!("Mutex poisoned: {}", e)))
    }

    /// Get a connection for write operations.
    ///
    /// Same as get_connection since we use std::sync::Mutex for serialization.
    pub fn get_write_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.get_connection()
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
        let pool = DuckConnectionPool::new(&config.path)?;

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
        let pool = DuckConnectionPool::new(&config.path)?;

        let fs = Self {
            pool,
            dentry_cache: Arc::new(DentryCache::new(config.dentry_cache_size)),
            embedding_generator,
            config,
        };

        fs.init_schema().await?;
        Ok(fs)
    }

    /// Get a connection for read operations.
    ///
    /// Returns a guard to the connection. Use within spawn_blocking for async.
    pub fn get_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.pool.get_connection()
    }

    /// Get a connection for write operations.
    ///
    /// Same as get_connection since DuckDB uses single-writer semantics.
    pub fn get_write_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.pool.get_write_connection()
    }

    /// Get the underlying connection pool.
    ///
    /// This is useful for advanced operations that need direct pool access,
    /// such as creating GraphDocsEngine instances.
    pub fn pool(&self) -> DuckConnectionPool {
        self.pool.clone()
    }

    /// Initialize the database schema.
    async fn init_schema(&self) -> Result<()> {
        let conn = self.pool.get_write_connection()?;

        // Execute the DDL from schema/duckagentfs.sql
        conn.execute_batch(include_str!("../../../../schema/duckagentfs.sql"))
            .map_err(|e| Error::Custom(format!("Failed to initialize schema: {}", e)))?;

        // Load extensions if enabled
        if self.config.enable_vss {
            conn.execute_batch("INSTALL vss; LOAD vss;")
                .map_err(|e| Error::Custom(format!("Failed to load VSS extension: {}", e)))?;
        }

        if self.config.enable_pgq {
            conn.execute_batch("INSTALL duckpgq; LOAD duckpgq;")
                .map_err(|e| Error::Custom(format!("Failed to load DuckPGQ extension: {}", e)))?;
        }

        Ok(())
    }

    // ========================================================================
    // JOURNAL OPERATIONS (Synchronous - called inside spawn_blocking)
    // ========================================================================

    /// Append a filesystem event to the journal (synchronous).
    ///
    /// This is a synchronous helper meant to be called from within spawn_blocking.
    fn append_journal_event_sync(
        conn: &Connection,
        inode: i64,
        event_type: &str,
        parent: Option<i64>,
        name: Option<&str>,
        mode: Option<u32>,
        size: Option<i64>,
        nlink: Option<u32>,
        old_parent: Option<i64>,
        old_name: Option<&str>,
        actor_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<i64> {
        let mut stmt = conn
            .prepare(
                r#"
                INSERT INTO fs_journal (
                    inode, event_type, parent, name, mode, size, nlink,
                    old_parent, old_name, actor_id, session_id
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                RETURNING event_id
                "#,
            )
            .map_err(|e| Error::Custom(format!("Failed to prepare journal insert: {}", e)))?;

        let event_id: i64 = stmt
            .query_row(
                params![
                    inode,
                    event_type,
                    parent,
                    name,
                    mode.map(|m| m as i64),
                    size,
                    nlink.map(|n| n as i64),
                    old_parent,
                    old_name,
                    actor_id,
                    session_id
                ],
                |row| row.get(0),
            )
            .map_err(|e| Error::Custom(format!("Failed to insert journal event: {}", e)))?;

        Ok(event_id)
    }

    /// Allocate a new inode number (synchronous).
    fn allocate_inode_sync(conn: &Connection) -> Result<i64> {
        let ino: i64 = conn
            .query_row("SELECT nextval('fs_inode_seq')", [], |row| row.get(0))
            .map_err(|e| Error::Custom(format!("Failed to allocate inode: {}", e)))?;
        Ok(ino)
    }

    // ========================================================================
    // PATH RESOLUTION (Synchronous - called inside spawn_blocking)
    // ========================================================================

    /// Resolve a path to an inode, optionally following symlinks (synchronous).
    ///
    /// This is a synchronous helper meant to be called from within spawn_blocking.
    fn resolve_path_sync(
        conn: &Connection,
        dentry_cache: &DentryCache,
        path: &str,
        follow_symlinks: bool,
    ) -> Result<Option<i64>> {
        Self::resolve_path_sync_inner(conn, dentry_cache, path, follow_symlinks, 0)
    }

    fn resolve_path_sync_inner(
        conn: &Connection,
        dentry_cache: &DentryCache,
        path: &str,
        follow_symlinks: bool,
        symlink_depth: usize,
    ) -> Result<Option<i64>> {
        const MAX_SYMLINK_DEPTH: usize = 40;

        let path = Self::normalize_path(path);
        if path == "/" {
            return Ok(Some(ROOT_INO));
        }

        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_ino = ROOT_INO;
        let mut current_symlink_depth = symlink_depth;

        for (i, component) in components.iter().enumerate() {
            // Check cache first
            if let Some(child_ino) = dentry_cache.get(current_ino, component) {
                current_ino = child_ino;
                continue;
            }

            // Query database for entry
            let child = Self::lookup_child_sync(conn, current_ino, component)?;

            match child {
                None => return Ok(None),
                Some((child_ino, mode)) => {
                    dentry_cache.insert(current_ino, component, child_ino);

                    // Handle symlinks
                    let is_last = i == components.len() - 1;
                    let is_symlink = (mode & S_IFMT) == S_IFLNK;

                    if is_symlink && (follow_symlinks || !is_last) {
                        current_symlink_depth += 1;
                        if current_symlink_depth > MAX_SYMLINK_DEPTH {
                            return Err(Error::Fs(FsError::SymlinkLoop));
                        }

                        // Read symlink target and resolve recursively
                        if let Some(target) = Self::read_symlink_target_sync(conn, child_ino)? {
                            let resolved = if target.starts_with('/') {
                                Self::resolve_path_sync_inner(
                                    conn,
                                    dentry_cache,
                                    &target,
                                    follow_symlinks,
                                    current_symlink_depth,
                                )?
                            } else {
                                // Relative symlink - resolve from parent
                                let parent_path = components[..i].join("/");
                                let full_target = format!("/{}/{}", parent_path, target);
                                Self::resolve_path_sync_inner(
                                    conn,
                                    dentry_cache,
                                    &full_target,
                                    follow_symlinks,
                                    current_symlink_depth,
                                )?
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

    /// Look up a child entry in a directory (synchronous).
    fn lookup_child_sync(
        conn: &Connection,
        parent_ino: i64,
        name: &str,
    ) -> Result<Option<(i64, u32)>> {
        let result: std::result::Result<(i64, i64), duckdb::Error> = conn.query_row(
            "SELECT inode, mode FROM fs_current WHERE parent = ? AND name = ?",
            params![parent_ino, name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match result {
            Ok((ino, mode)) => Ok(Some((ino, mode as u32))),
            Err(duckdb::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Custom(format!("Failed to lookup child: {}", e))),
        }
    }

    /// Read the target of a symlink (synchronous).
    fn read_symlink_target_sync(conn: &Connection, ino: i64) -> Result<Option<String>> {
        // Symlink targets are stored in fs_data as the first chunk
        let result: std::result::Result<Vec<u8>, duckdb::Error> = conn.query_row(
            "SELECT data FROM fs_data WHERE inode = ? AND chunk_idx = 0",
            params![ino],
            |row| row.get(0),
        );

        match result {
            Ok(data) => Ok(Some(String::from_utf8(data).map_err(|e| {
                Error::Custom(format!("Invalid symlink target: {}", e))
            })?)),
            Err(duckdb::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Custom(format!("Failed to read symlink: {}", e))),
        }
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
    // INODE OPERATIONS (Synchronous - called inside spawn_blocking)
    // ========================================================================

    /// Get stats for an inode (synchronous).
    fn stat_inode_sync(conn: &Connection, ino: i64) -> Result<Option<Stats>> {
        let result: std::result::Result<Stats, duckdb::Error> = conn.query_row(
            r#"
            SELECT inode, mode, nlink, size,
                   EXTRACT(EPOCH FROM mtime)::BIGINT as mtime
            FROM fs_current
            WHERE inode = ?
            "#,
            params![ino],
            |row| {
                let mode: i64 = row.get(1)?;
                let nlink: i64 = row.get(2)?;
                let size: i64 = row.get(3)?;
                let mtime: i64 = row.get(4)?;
                Ok(Stats {
                    ino: row.get(0)?,
                    mode: mode as u32,
                    nlink: nlink as u32,
                    uid: 0,
                    gid: 0,
                    size,
                    atime: mtime,
                    mtime,
                    ctime: mtime,
                })
            },
        );

        match result {
            Ok(stats) => Ok(Some(stats)),
            Err(duckdb::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Custom(format!("Failed to stat inode: {}", e))),
        }
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
        let _conn = self.pool.get_connection()?;
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
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<(String, f32, String)>> {
        if !self.config.enable_vss {
            return Err(Error::Custom("VSS not enabled".into()));
        }

        // Generate query embedding
        let query_embedding = self.embedding_generator.generate(query).await?;

        let _conn = self.pool.get_connection()?;

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

            let _conn = self.pool.get_write_connection()?;

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
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;
            match Self::resolve_path_sync(&conn, &dentry_cache, &path, true)? {
                Some(ino) => Self::stat_inode_sync(&conn, ino),
                None => Ok(None),
            }
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn lstat(&self, path: &str) -> Result<Option<Stats>> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;
            match Self::resolve_path_sync(&conn, &dentry_cache, &path, false)? {
                Some(ino) => Self::stat_inode_sync(&conn, ino),
                None => Ok(None),
            }
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let ino = match Self::resolve_path_sync(&conn, &dentry_cache, &path, true)? {
                Some(ino) => ino,
                None => return Ok(None),
            };

            let stats = match Self::stat_inode_sync(&conn, ino)? {
                Some(s) => s,
                None => return Ok(None),
            };

            if !stats.is_file() {
                return Err(Error::Fs(FsError::IsADirectory));
            }

            let data = Self::read_data_chunks_sync(&conn, ino, stats.size)?;
            Ok(Some(data))
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let config = self.config.clone();
        let path = path.to_string();
        let data = data.to_vec();
        // Clone data for embedding update (needs to live after spawn_blocking)
        let data_for_embedding = data.clone();

        // First, ensure parent directories exist (may need recursive mkdir)
        let (parent_path, name) = Self::split_path(&path);
        let parent_path = parent_path.to_string();
        let name = name.to_string();

        // Check if parent exists
        let parent_exists = {
            let pool = pool.clone();
            let dentry_cache = dentry_cache.clone();
            let parent_path = parent_path.clone();
            tokio::task::spawn_blocking(move || {
                let conn = pool.get_connection()?;
                Self::resolve_path_sync(&conn, &dentry_cache, &parent_path, true)
            })
            .await
            .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))??
        };

        // Create parent directories if needed
        if parent_exists.is_none() {
            self.mkdir_recursive_impl(&parent_path).await?;
        }

        // Now write the file
        let result = tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            // Resolve parent
            let parent_ino = Self::resolve_path_sync(&conn, &dentry_cache, &parent_path, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            // Check if file exists
            let existing = Self::lookup_child_sync(&conn, parent_ino, &name)?;

            let ino = match existing {
                Some((ino, mode)) => {
                    if (mode & S_IFMT) == S_IFDIR {
                        return Err(Error::Fs(FsError::IsADirectory));
                    }

                    // Update existing file - append 'update' event
                    Self::append_journal_event_sync(
                        &conn,
                        ino,
                        "update",
                        Some(parent_ino),
                        Some(&name),
                        Some(DEFAULT_FILE_MODE),
                        Some(data.len() as i64),
                        None,
                        None,
                        None,
                        config.actor_id.as_deref(),
                        config.session_id.as_deref(),
                    )?;

                    ino
                }
                None => {
                    // Create new file
                    let ino = Self::allocate_inode_sync(&conn)?;

                    Self::append_journal_event_sync(
                        &conn,
                        ino,
                        "create",
                        Some(parent_ino),
                        Some(&name),
                        Some(DEFAULT_FILE_MODE),
                        Some(data.len() as i64),
                        Some(1),
                        None,
                        None,
                        config.actor_id.as_deref(),
                        config.session_id.as_deref(),
                    )?;

                    dentry_cache.insert(parent_ino, &name, ino);
                    ino
                }
            };

            // Write data chunks
            Self::write_data_chunks_sync(&conn, ino, &data, config.chunk_size)?;

            Ok(ino)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))??;

        // Update embedding if VSS enabled (outside spawn_blocking since it's async)
        if self.config.enable_vss {
            self.update_embedding_async(result, &data_for_embedding)
                .await?;
        }

        Ok(())
    }

    async fn readdir(&self, path: &str) -> Result<Option<Vec<String>>> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let ino = match Self::resolve_path_sync(&conn, &dentry_cache, &path, true)? {
                Some(ino) => ino,
                None => return Ok(None),
            };

            let stats = match Self::stat_inode_sync(&conn, ino)? {
                Some(s) => s,
                None => return Ok(None),
            };

            if !stats.is_directory() {
                return Err(Error::Fs(FsError::NotADirectory));
            }

            let entries = Self::readdir_sync(&conn, ino)?;
            Ok(Some(entries))
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn readdir_plus(&self, path: &str) -> Result<Option<Vec<DirEntry>>> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let ino = match Self::resolve_path_sync(&conn, &dentry_cache, &path, true)? {
                Some(ino) => ino,
                None => return Ok(None),
            };

            let stats = match Self::stat_inode_sync(&conn, ino)? {
                Some(s) => s,
                None => return Ok(None),
            };

            if !stats.is_directory() {
                return Err(Error::Fs(FsError::NotADirectory));
            }

            let parent_stats = if ino == ROOT_INO {
                stats.clone()
            } else {
                Self::stat_inode_sync(&conn, ROOT_INO)?.unwrap_or(stats.clone())
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

            // Query children with stats
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT name, inode, mode, nlink, size,
                           EXTRACT(EPOCH FROM mtime)::BIGINT as mtime
                    FROM fs_current
                    WHERE parent = ? AND inode != ?
                    "#,
                )
                .map_err(|e| Error::Custom(format!("Failed to prepare readdir_plus: {}", e)))?;

            let rows = stmt
                .query_map(params![ino, ino], |row| {
                    let mode: i64 = row.get(2)?;
                    let nlink: i64 = row.get(3)?;
                    let size: i64 = row.get(4)?;
                    let mtime: i64 = row.get(5)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        Stats {
                            ino: row.get(1)?,
                            mode: mode as u32,
                            nlink: nlink as u32,
                            uid: 0,
                            gid: 0,
                            size,
                            atime: mtime,
                            mtime,
                            ctime: mtime,
                        },
                    ))
                })
                .map_err(|e| Error::Custom(format!("Failed to query readdir_plus: {}", e)))?;

            for row_result in rows {
                let (name, child_stats) = row_result
                    .map_err(|e| Error::Custom(format!("Failed to read entry: {}", e)))?;
                entries.push(DirEntry {
                    name,
                    stats: child_stats,
                });
            }

            Ok(Some(entries))
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn mkdir(&self, path: &str) -> Result<()> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let config = self.config.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let (parent_path, name) = Self::split_path(&path);
            let conn = pool.get_connection()?;

            let parent_ino = Self::resolve_path_sync(&conn, &dentry_cache, parent_path, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            // Check parent is a directory
            let parent_stats =
                Self::stat_inode_sync(&conn, parent_ino)?.ok_or(Error::Fs(FsError::NotFound))?;

            if !parent_stats.is_directory() {
                return Err(Error::Fs(FsError::NotADirectory));
            }

            // Check if name already exists
            if Self::lookup_child_sync(&conn, parent_ino, name)?.is_some() {
                return Err(Error::Fs(FsError::AlreadyExists));
            }

            let ino = Self::allocate_inode_sync(&conn)?;

            Self::append_journal_event_sync(
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
                config.actor_id.as_deref(),
                config.session_id.as_deref(),
            )?;

            dentry_cache.insert(parent_ino, name, ino);

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn remove(&self, path: &str) -> Result<()> {
        if path == "/" {
            return Err(Error::Fs(FsError::RootOperation));
        }

        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let config = self.config.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let (parent_path, name) = Self::split_path(&path);
            let conn = pool.get_connection()?;

            let parent_ino = Self::resolve_path_sync(&conn, &dentry_cache, parent_path, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            let (ino, mode) = Self::lookup_child_sync(&conn, parent_ino, name)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            // Check if directory is empty
            if (mode & S_IFMT) == S_IFDIR {
                let entries = Self::readdir_sync(&conn, ino)?;
                if entries.len() > 2 {
                    // . and ..
                    return Err(Error::Fs(FsError::NotEmpty));
                }
            }

            Self::append_journal_event_sync(
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
                config.actor_id.as_deref(),
                config.session_id.as_deref(),
            )?;

            dentry_cache.remove(parent_ino, name);

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<()> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let config = self.config.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let ino = Self::resolve_path_sync(&conn, &dentry_cache, &path, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            let stats = Self::stat_inode_sync(&conn, ino)?.ok_or(Error::Fs(FsError::NotFound))?;

            // Preserve file type, update permissions
            let new_mode = (stats.mode & S_IFMT) | (mode & 0o7777);

            Self::append_journal_event_sync(
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
                config.actor_id.as_deref(),
                config.session_id.as_deref(),
            )?;

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let config = self.config.clone();
        let from = from.to_string();
        let to = to.to_string();

        tokio::task::spawn_blocking(move || {
            let (from_parent_path, from_name) = Self::split_path(&from);
            let (to_parent_path, to_name) = Self::split_path(&to);
            let conn = pool.get_connection()?;

            let from_parent_ino =
                Self::resolve_path_sync(&conn, &dentry_cache, from_parent_path, true)?
                    .ok_or(Error::Fs(FsError::NotFound))?;

            let to_parent_ino =
                Self::resolve_path_sync(&conn, &dentry_cache, to_parent_path, true)?
                    .ok_or(Error::Fs(FsError::NotFound))?;

            let (ino, file_mode) = Self::lookup_child_sync(&conn, from_parent_ino, from_name)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            // Check target doesn't exist (or handle overwrite)
            if let Some((target_ino, target_mode)) =
                Self::lookup_child_sync(&conn, to_parent_ino, to_name)?
            {
                // Can't rename directory over file or vice versa
                if (file_mode & S_IFMT) != (target_mode & S_IFMT) {
                    return Err(Error::Fs(FsError::InvalidRename));
                }

                // Delete target
                Self::append_journal_event_sync(
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
                    config.actor_id.as_deref(),
                    config.session_id.as_deref(),
                )?;
            }

            Self::append_journal_event_sync(
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
                config.actor_id.as_deref(),
                config.session_id.as_deref(),
            )?;

            // Update cache
            dentry_cache.remove(from_parent_ino, from_name);
            dentry_cache.insert(to_parent_ino, to_name, ino);

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn symlink(&self, target: &str, linkpath: &str) -> Result<()> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let config = self.config.clone();
        let target = target.to_string();
        let linkpath = linkpath.to_string();

        tokio::task::spawn_blocking(move || {
            let (parent_path, name) = Self::split_path(&linkpath);
            let conn = pool.get_connection()?;

            let parent_ino = Self::resolve_path_sync(&conn, &dentry_cache, parent_path, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            if Self::lookup_child_sync(&conn, parent_ino, name)?.is_some() {
                return Err(Error::Fs(FsError::AlreadyExists));
            }

            let ino = Self::allocate_inode_sync(&conn)?;

            Self::append_journal_event_sync(
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
                config.actor_id.as_deref(),
                config.session_id.as_deref(),
            )?;

            // Store symlink target as file data
            Self::write_data_chunks_sync(&conn, ino, target.as_bytes(), config.chunk_size)?;

            dentry_cache.insert(parent_ino, name, ino);

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn link(&self, oldpath: &str, newpath: &str) -> Result<()> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let config = self.config.clone();
        let oldpath = oldpath.to_string();
        let newpath = newpath.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let old_ino = Self::resolve_path_sync(&conn, &dentry_cache, &oldpath, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            let old_stats =
                Self::stat_inode_sync(&conn, old_ino)?.ok_or(Error::Fs(FsError::NotFound))?;

            if old_stats.is_directory() {
                return Err(Error::Fs(FsError::IsADirectory));
            }

            let (parent_path, name) = Self::split_path(&newpath);

            let parent_ino = Self::resolve_path_sync(&conn, &dentry_cache, parent_path, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            if Self::lookup_child_sync(&conn, parent_ino, name)?.is_some() {
                return Err(Error::Fs(FsError::AlreadyExists));
            }

            // Create link event (same inode, new name)
            Self::append_journal_event_sync(
                &conn,
                old_ino,
                "link",
                Some(parent_ino),
                Some(name),
                Some(old_stats.mode),
                Some(old_stats.size),
                Some(old_stats.nlink + 1),
                None,
                None,
                config.actor_id.as_deref(),
                config.session_id.as_deref(),
            )?;

            dentry_cache.insert(parent_ino, name, old_ino);

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn readlink(&self, path: &str) -> Result<Option<String>> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let ino = match Self::resolve_path_sync(&conn, &dentry_cache, &path, false)? {
                Some(ino) => ino,
                None => return Ok(None),
            };

            let stats = match Self::stat_inode_sync(&conn, ino)? {
                Some(s) => s,
                None => return Ok(None),
            };

            if !stats.is_symlink() {
                return Err(Error::Fs(FsError::NotASymlink));
            }

            Self::read_symlink_target_sync(&conn, ino)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn statfs(&self) -> Result<FilesystemStats> {
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let inodes: i64 = conn
                .query_row("SELECT COUNT(DISTINCT inode) FROM fs_current", [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);

            let bytes_used: i64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM fs_data",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            Ok(FilesystemStats {
                inodes: inodes as u64,
                bytes_used: bytes_used as u64,
            })
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn open(&self, path: &str) -> Result<BoxedFile> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let chunk_size = self.config.chunk_size;
        let path = path.to_string();

        let ino = tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            let ino = Self::resolve_path_sync(&conn, &dentry_cache, &path, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            let stats = Self::stat_inode_sync(&conn, ino)?.ok_or(Error::Fs(FsError::NotFound))?;

            if !stats.is_file() {
                return Err(Error::Fs(FsError::IsADirectory));
            }

            Ok(ino)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))??;

        Ok(Arc::new(DuckAgentFSFile {
            pool: self.pool.clone(),
            ino,
            chunk_size,
        }))
    }

    async fn create_file(&self, path: &str, mode: u32) -> Result<(Stats, BoxedFile)> {
        let pool = self.pool.clone();
        let dentry_cache = self.dentry_cache.clone();
        let config = self.config.clone();
        let path = path.to_string();

        let (ino, stats) = tokio::task::spawn_blocking(move || {
            let (parent_path, name) = Self::split_path(&path);
            let conn = pool.get_connection()?;

            let parent_ino = Self::resolve_path_sync(&conn, &dentry_cache, parent_path, true)?
                .ok_or(Error::Fs(FsError::NotFound))?;

            if Self::lookup_child_sync(&conn, parent_ino, name)?.is_some() {
                return Err(Error::Fs(FsError::AlreadyExists));
            }

            let ino = Self::allocate_inode_sync(&conn)?;

            let file_mode = (mode & 0o7777) | S_IFREG;

            Self::append_journal_event_sync(
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
                config.actor_id.as_deref(),
                config.session_id.as_deref(),
            )?;

            dentry_cache.insert(parent_ino, name, ino);

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

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

            Ok((ino, stats))
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))??;

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
    /// Write data to file in chunks (synchronous).
    fn write_data_chunks_sync(
        conn: &Connection,
        ino: i64,
        data: &[u8],
        chunk_size: usize,
    ) -> Result<()> {
        // Delete existing chunks
        conn.execute("DELETE FROM fs_data WHERE inode = ?", params![ino])
            .map_err(|e| Error::Custom(format!("Failed to delete existing chunks: {}", e)))?;

        // Write new chunks
        for (idx, chunk) in data.chunks(chunk_size).enumerate() {
            conn.execute(
                "INSERT INTO fs_data (inode, chunk_idx, data) VALUES (?, ?, ?)",
                params![ino, idx as i64, chunk],
            )
            .map_err(|e| Error::Custom(format!("Failed to write chunk: {}", e)))?;
        }

        Ok(())
    }

    /// Read all data chunks for an inode (synchronous).
    fn read_data_chunks_sync(conn: &Connection, ino: i64, size: i64) -> Result<Vec<u8>> {
        let mut stmt = conn
            .prepare("SELECT data FROM fs_data WHERE inode = ? ORDER BY chunk_idx")
            .map_err(|e| Error::Custom(format!("Failed to prepare read chunks: {}", e)))?;

        let chunks = stmt
            .query_map(params![ino], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|e| Error::Custom(format!("Failed to query chunks: {}", e)))?;

        let mut data = Vec::new();
        for chunk_result in chunks {
            let chunk =
                chunk_result.map_err(|e| Error::Custom(format!("Failed to read chunk: {}", e)))?;
            data.extend(chunk);
        }

        // Truncate to actual file size
        data.truncate(size as usize);
        Ok(data)
    }

    /// Read directory entries (synchronous).
    fn readdir_sync(conn: &Connection, ino: i64) -> Result<Vec<String>> {
        let mut entries = vec![".".to_string(), "..".to_string()];

        let mut stmt = conn
            .prepare("SELECT name FROM fs_current WHERE parent = ? AND inode != ?")
            .map_err(|e| Error::Custom(format!("Failed to prepare readdir: {}", e)))?;

        let names = stmt
            .query_map(params![ino, ino], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Custom(format!("Failed to query directory: {}", e)))?;

        for name_result in names {
            let name =
                name_result.map_err(|e| Error::Custom(format!("Failed to read entry: {}", e)))?;
            entries.push(name);
        }

        Ok(entries)
    }

    /// Recursively create parent directories.
    async fn mkdir_recursive_impl(&self, path: &str) -> Result<()> {
        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let mut current_path = String::new();
        for component in components {
            current_path.push('/');
            current_path.push_str(component);

            // Check if exists
            let exists = {
                let pool = self.pool.clone();
                let dentry_cache = self.dentry_cache.clone();
                let path = current_path.clone();
                tokio::task::spawn_blocking(move || {
                    let conn = pool.get_connection()?;
                    Self::resolve_path_sync(&conn, &dentry_cache, &path, true)
                })
                .await
                .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))??
            };

            if exists.is_none() {
                self.mkdir(&current_path).await?;
            }
        }

        Ok(())
    }

    /// Update embedding for a file (async wrapper).
    async fn update_embedding_async(&self, ino: i64, content: &[u8]) -> Result<()> {
        if !self.config.enable_vss {
            return Ok(());
        }

        // Only embed text-like content
        if let Ok(text) = std::str::from_utf8(content) {
            let embedding = self.embedding_generator.generate(text).await?;
            let model_name = self.embedding_generator.model_name().to_string();

            let pool = self.pool.clone();
            let text = text.to_string();

            tokio::task::spawn_blocking(move || -> Result<()> {
                let conn = pool.get_connection()?;

                conn.execute(
                    r#"
                    INSERT INTO fs_embeddings (inode, embedding, model, content_hash)
                    VALUES (?, ?, ?, md5(?))
                    ON CONFLICT (inode) DO UPDATE SET
                        embedding = excluded.embedding,
                        model = excluded.model,
                        content_hash = excluded.content_hash,
                        updated_at = CURRENT_TIMESTAMP
                    "#,
                    params![
                        ino,
                        serde_json::to_string(&embedding).map_err(|e| Error::Custom(format!(
                            "Failed to serialize embedding: {}",
                            e
                        )))?,
                        model_name,
                        text
                    ],
                )
                .map_err(|e| Error::Custom(format!("Failed to update embedding: {}", e)))?;

                Ok(())
            })
            .await
            .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))??;
        }

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
        let pool = self.pool.clone();
        let ino = self.ino;
        let chunk_size = self.chunk_size;

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            // Get file size
            let file_size: i64 = conn
                .query_row(
                    "SELECT size FROM fs_current WHERE inode = ?",
                    params![ino],
                    |row| row.get(0),
                )
                .map_err(|e| Error::Custom(format!("Failed to get file size: {}", e)))?;

            let chunk_size_u64 = chunk_size as u64;
            let start_chunk = offset / chunk_size_u64;
            let end_chunk = (offset + size).saturating_sub(1) / chunk_size_u64;

            let mut stmt = conn
                .prepare(
                    "SELECT chunk_idx, data FROM fs_data WHERE inode = ? AND chunk_idx >= ? AND chunk_idx <= ? ORDER BY chunk_idx",
                )
                .map_err(|e| Error::Custom(format!("Failed to prepare pread: {}", e)))?;

            let rows = stmt
                .query_map(params![ino, start_chunk as i64, end_chunk as i64], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(|e| Error::Custom(format!("Failed to query pread: {}", e)))?;

            let mut result = Vec::new();
            for row_result in rows {
                let (_idx, chunk) =
                    row_result.map_err(|e| Error::Custom(format!("Failed to read chunk: {}", e)))?;
                result.extend(chunk);
            }

            // Adjust for offset within first chunk
            let offset_in_chunk = (offset % chunk_size_u64) as usize;
            if offset_in_chunk > 0 && !result.is_empty() {
                result = result[offset_in_chunk..].to_vec();
            }

            // Truncate to requested size
            let actual_size = std::cmp::min(size as usize, (file_size as u64 - offset) as usize);
            result.truncate(actual_size);

            Ok(result)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn pwrite(&self, offset: u64, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let pool = self.pool.clone();
        let ino = self.ino;
        let chunk_size = self.chunk_size;
        let data = data.to_vec();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            // Read current file data
            let current_size: i64 = conn
                .query_row(
                    "SELECT COALESCE(size, 0) FROM fs_current WHERE inode = ?",
                    params![ino],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let mut file_data = DuckAgentFS::read_data_chunks_sync(&conn, ino, current_size)?;

            // Extend file data if needed
            let end_offset = offset as usize + data.len();
            if end_offset > file_data.len() {
                file_data.resize(end_offset, 0);
            }

            // Write data at offset
            file_data[offset as usize..end_offset].copy_from_slice(&data);

            // Rewrite all chunks
            DuckAgentFS::write_data_chunks_sync(&conn, ino, &file_data, chunk_size)?;

            // Update file size in journal
            DuckAgentFS::append_journal_event_sync(
                &conn,
                ino,
                "update",
                None,
                None,
                None,
                Some(file_data.len() as i64),
                None,
                None,
                None,
                None,
                None,
            )?;

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn truncate(&self, size: u64) -> Result<()> {
        let pool = self.pool.clone();
        let ino = self.ino;
        let chunk_size = self.chunk_size;

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;

            // Read current data
            let current_size: i64 = conn
                .query_row(
                    "SELECT COALESCE(size, 0) FROM fs_current WHERE inode = ?",
                    params![ino],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let mut file_data = DuckAgentFS::read_data_chunks_sync(&conn, ino, current_size)?;

            // Resize
            file_data.resize(size as usize, 0);

            // Rewrite chunks
            DuckAgentFS::write_data_chunks_sync(&conn, ino, &file_data, chunk_size)?;

            // Update size in journal
            DuckAgentFS::append_journal_event_sync(
                &conn,
                ino,
                "update",
                None,
                None,
                None,
                Some(size as i64),
                None,
                None,
                None,
                None,
                None,
            )?;

            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn fsync(&self) -> Result<()> {
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;
            conn.execute("CHECKPOINT", [])
                .map_err(|e| Error::Custom(format!("Failed to checkpoint: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }

    async fn fstat(&self) -> Result<Stats> {
        let pool = self.pool.clone();
        let ino = self.ino;

        tokio::task::spawn_blocking(move || {
            let conn = pool.get_connection()?;
            DuckAgentFS::stat_inode_sync(&conn, ino)?.ok_or(Error::Fs(FsError::NotFound))
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking join error: {}", e)))?
    }
}

// ============================================================================
// SNAPSHOT (TIME-TRAVEL)
// ============================================================================

/// A read-only snapshot of DuckAgentFS at a specific point in time.
///
/// This struct provides the same FileSystem interface but queries
/// historical state instead of current state.
///
/// NOTE: Time-travel is an advanced feature. The current implementation
/// delegates to the underlying fs for read operations. Full time-travel
/// functionality will be implemented in a later story.
pub struct DuckAgentFSSnapshot {
    fs: DuckAgentFS,
    event_id: i64,
}

impl DuckAgentFSSnapshot {
    /// Get the event ID this snapshot represents.
    pub fn event_id(&self) -> i64 {
        self.event_id
    }
}

// Implement read-only FileSystem for snapshot
// NOTE: Currently delegates to underlying fs. Full time-travel
// implementation will filter queries by event_id in a later story.
#[async_trait]
impl FileSystem for DuckAgentFSSnapshot {
    async fn stat(&self, path: &str) -> Result<Option<Stats>> {
        // TODO: Implement time-travel query filtering by event_id
        self.fs.stat(path).await
    }

    async fn lstat(&self, path: &str) -> Result<Option<Stats>> {
        // TODO: Implement time-travel query filtering by event_id
        self.fs.lstat(path).await
    }

    async fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        // TODO: Implement time-travel query filtering by event_id
        self.fs.read_file(path).await
    }

    async fn write_file(&self, _path: &str, _data: &[u8]) -> Result<()> {
        Err(Error::Custom("Snapshots are read-only".into()))
    }

    async fn readdir(&self, path: &str) -> Result<Option<Vec<String>>> {
        // TODO: Implement time-travel query filtering by event_id
        self.fs.readdir(path).await
    }

    async fn readdir_plus(&self, path: &str) -> Result<Option<Vec<DirEntry>>> {
        // TODO: Implement time-travel query filtering by event_id
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
        // TODO: Implement time-travel query filtering by event_id
        self.fs.readlink(path).await
    }

    async fn statfs(&self) -> Result<FilesystemStats> {
        // TODO: Implement time-travel query filtering by event_id
        self.fs.statfs().await
    }

    async fn open(&self, path: &str) -> Result<BoxedFile> {
        // TODO: Implement time-travel query filtering by event_id
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

    /// Test that DuckDB integration works: connection opens, schema loads, root exists.
    #[tokio::test]
    async fn test_duckdb_integration() {
        // Create in-memory database
        let config = DuckAgentFSConfig {
            path: ":memory:".to_string(),
            ..Default::default()
        };

        // Open should succeed and load schema
        let fs = DuckAgentFS::open(config)
            .await
            .expect("Failed to open DuckAgentFS");

        // Verify we can query the database - check root inode exists
        let conn = fs.pool.get_connection().expect("Failed to get connection");

        // Query root inode from fs_journal
        let mut stmt = conn
            .prepare("SELECT inode, event_type, mode FROM fs_journal WHERE inode = 1")
            .expect("Failed to prepare statement");

        let result: std::result::Result<(i64, String, u32), _> =
            stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)));

        let (inode, event_type, mode) = result.expect("Root inode should exist");
        assert_eq!(inode, 1, "Root inode should be 1");
        assert_eq!(event_type, "create", "Root event should be 'create'");
        assert_eq!(mode, 16877, "Root mode should be S_IFDIR | 0755 (16877)");
    }

    /// Test that connection pool provides proper synchronization
    #[tokio::test]
    async fn test_connection_pool() {
        let pool = DuckConnectionPool::new(":memory:").expect("Failed to create pool");

        // Read connection should work
        let conn = pool
            .get_connection()
            .expect("Failed to get read connection");
        drop(conn);

        // Write connection should work
        let write_conn = pool
            .get_write_connection()
            .expect("Failed to get write connection");

        // Execute a simple query through write connection
        write_conn
            .execute("CREATE TABLE test (id INTEGER)", [])
            .expect("Failed to create table");
    }

    /// Test schema includes all expected tables
    #[tokio::test]
    async fn test_schema_tables_exist() {
        let config = DuckAgentFSConfig {
            path: ":memory:".to_string(),
            ..Default::default()
        };

        let fs = DuckAgentFS::open(config)
            .await
            .expect("Failed to open DuckAgentFS");
        let conn = fs.pool.get_connection().expect("Failed to get connection");

        // Check that key tables exist by querying them
        let tables = vec![
            "fs_journal",
            "fs_data",
            "kv_store",
            "tool_calls",
            "fs_embeddings",
            "code_symbols",
            "gd_documents",
        ];

        for table in tables {
            let query = format!("SELECT COUNT(*) FROM {}", table);
            let result: std::result::Result<i64, _> = conn.query_row(&query, [], |row| row.get(0));
            assert!(result.is_ok(), "Table {} should exist", table);
        }
    }

    /// Test that fs_current view coalesces values from multiple journal events.
    ///
    /// This test verifies the fix for STORY-BUG-001 where fs_current would return
    /// NULL for fields that weren't updated in the latest journal event.
    ///
    /// Scenario:
    /// 1. Create a file (event has parent, name, mode, size=0)
    /// 2. Update only the size (event has ONLY size, other fields are NULL)
    /// 3. Query fs_current and verify all fields are present (coalesced)
    #[tokio::test]
    async fn test_fs_current_coalesces_multi_event_inodes() {
        let config = DuckAgentFSConfig {
            path: ":memory:".to_string(),
            ..Default::default()
        };

        let fs = DuckAgentFS::open(config)
            .await
            .expect("Failed to open DuckAgentFS");
        let conn = fs.pool.get_connection().expect("Failed to get connection");

        // Step 1: Create a file with full metadata
        let inode: i64 = 100;
        conn.execute(
            "INSERT INTO fs_journal (inode, event_type, parent, name, mode, size, nlink) VALUES (?, 'create', 1, 'test.txt', 33188, 0, 1)",
            params![inode],
        ).expect("Failed to insert create event");

        // Step 2: Update only the size (simulating a write operation)
        // This is how the bug manifested - only size is set, other fields are NULL
        conn.execute(
            "INSERT INTO fs_journal (inode, event_type, size) VALUES (?, 'update', 100)",
            params![inode],
        ).expect("Failed to insert update event");

        // Step 3: Query fs_current and verify all fields are coalesced
        let result: (Option<i64>, Option<String>, Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT parent, name, mode, size FROM fs_current WHERE inode = ?",
                params![inode],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("Failed to query fs_current");

        // Verify coalescing worked - all fields should be present
        assert_eq!(result.0, Some(1), "parent should be coalesced from create event");
        assert_eq!(result.1, Some("test.txt".to_string()), "name should be coalesced from create event");
        assert_eq!(result.2, Some(33188), "mode should be coalesced from create event");
        assert_eq!(result.3, Some(100), "size should be from latest update event");
    }

    /// Test write-then-read via FileSystem trait.
    ///
    /// This is an integration test that verifies the complete flow:
    /// create file -> write content -> read back.
    #[tokio::test]
    async fn test_write_then_read_file() {
        let config = DuckAgentFSConfig {
            path: ":memory:".to_string(),
            ..Default::default()
        };

        let fs = DuckAgentFS::open(config)
            .await
            .expect("Failed to open DuckAgentFS");

        // Write a file
        let content = b"Hello, DuckAgentFS!";
        fs.write_file("/test.txt", content)
            .await
            .expect("Failed to write file");

        // Read it back
        let read_content = fs
            .read_file("/test.txt")
            .await
            .expect("Failed to read file")
            .expect("File should exist");

        assert_eq!(read_content, content, "Read content should match written content");

        // Stat the file to verify metadata is complete
        let stats = fs
            .stat("/test.txt")
            .await
            .expect("Failed to stat file")
            .expect("File should exist");

        assert!(stats.is_file(), "Should be a file");
        assert_eq!(stats.size, content.len() as i64, "Size should match content length");
        assert!(stats.mode > 0, "Mode should be set");
    }
}
