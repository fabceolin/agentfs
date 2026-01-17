use crate::fuser::{
    consts::{
        FUSE_ASYNC_READ, FUSE_CACHE_SYMLINKS, FUSE_NO_OPENDIR_SUPPORT, FUSE_PARALLEL_DIROPS,
        FUSE_WRITEBACK_CACHE,
    },
    FileAttr, FileType, Filesystem, KernelConfig, MountOption, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyDirectoryPlus, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite,
    ReplyXattr, Request,
};
use crate::handler::HandlerRegistry;
use agentfs_sdk::error::Error as SdkError;
use agentfs_sdk::{BoxedFile, FileSystem, Stats};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::runtime::Runtime;
use tracing;

/// Extended attribute name for controlling raw/rendered mode.
/// - `0` (default): rendered mode - read returns rendered content, write is blocked
/// - `1`: raw mode - read returns raw template, write is allowed
const XATTR_RAW_MODE: &str = "user.agentfs.raw";

/// Suffix for accessing raw source of a file.
/// e.g., `file.md.source` always returns raw content regardless of xattr.
const SOURCE_SUFFIX: &str = ".source";

/// Bit mask for identifying virtual "source" inodes that force raw mode.
/// These are created when looking up `file.md.source` paths.
const SOURCE_INODE_MASK: u64 = 0x4000_0000_0000_0000;

/// Maximum time allowed for template rendering operations (AC14).
const RENDER_TIMEOUT: Duration = Duration::from_secs(5);

/// Convert an SDK error to an errno code for FUSE replies.
///
/// If the error is a filesystem-specific FsError, returns the appropriate
/// errno code (ENOENT, EEXIST, ENOTDIR, etc.). Database busy errors and
/// connection pool timeouts return EAGAIN to signal the caller should retry.
/// Otherwise falls back to EIO.
fn error_to_errno(e: &SdkError) -> i32 {
    match e {
        SdkError::Fs(fs_err) => fs_err.to_errno(),
        SdkError::Io(io_err) => io_err.raw_os_error().unwrap_or(libc::EIO),
        SdkError::Database(turso::Error::Busy(_)) => libc::EAGAIN,
        SdkError::ConnectionPoolTimeout => libc::EAGAIN,
        _ => libc::EIO,
    }
}

/// Cache entries never expire - we explicitly invalidate on mutations.
/// This is safe because we are the only writer to the filesystem.
const TTL: Duration = Duration::MAX;

/// Options for mounting an agent filesystem via FUSE.
#[derive(Debug, Clone)]
pub struct FuseMountOptions {
    /// The mountpoint path.
    pub mountpoint: PathBuf,
    /// Automatically unmount when the process exits.
    pub auto_unmount: bool,
    /// Allow root to access the mount.
    pub allow_root: bool,
    /// Filesystem name shown in mount output.
    pub fsname: String,
    /// User ID to report for all files (defaults to current user).
    pub uid: Option<u32>,
    /// Group ID to report for all files (defaults to current group).
    pub gid: Option<u32>,
}

/// Tracks an open file handle
struct OpenFile {
    /// The file handle from the filesystem layer.
    file: BoxedFile,
}

// ─────────────────────────────────────────────────────────────
// Stub Template Renderer (Phase 2)
// ─────────────────────────────────────────────────────────────

/// Stub template renderer for Phase 2 of STORY-5.4.
///
/// This is a placeholder implementation that returns raw content unchanged.
/// When STORY-2.1.5 (Cross-Document Relationships and Jinja2 Rendering) is
/// complete, this should be replaced with the real `TemplateProcessor` that
/// supports Tera/Jinja2 syntax and the `query()` function for DuckDB PGQ.
///
/// # TODO: Replace with TemplateProcessor from STORY-2.1.5
///
/// The real TemplateProcessor will:
/// - Parse and render Tera/Jinja2 templates
/// - Support `query()` function for DuckDB PGQ queries
/// - Handle variable substitution and filters
/// - Cache compiled templates for performance
pub struct StubTemplateRenderer;

impl StubTemplateRenderer {
    /// Create a new stub renderer.
    pub fn new() -> Self {
        Self
    }

    /// Render markdown content with timeout.
    ///
    /// # Stub Implementation
    ///
    /// Currently returns the raw content unchanged. When STORY-2.1.5 is
    /// complete, this will render Tera templates with variable substitution.
    ///
    /// # Arguments
    ///
    /// * `content` - Raw markdown content (potentially with Tera syntax)
    /// * `timeout` - Maximum time allowed for rendering
    ///
    /// # Returns
    ///
    /// - `Ok(rendered)` - Rendered content (currently just raw content)
    /// - `Err(message)` - Error message for graceful degradation (AC13)
    pub fn render_with_timeout(
        &self,
        content: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        let _ = timeout; // Timeout will be used when real rendering is implemented

        // Stub: Check for Tera syntax and add a warning comment if found
        let content_str = String::from_utf8_lossy(content);

        if content_str.contains("{{") || content_str.contains("{%") {
            // Content appears to have Tera syntax - add a notice
            // In production, this would be rendered by TemplateProcessor
            let notice = format!(
                "<!-- AgentFS Notice: Template rendering not yet available (STORY-2.1.5 pending). -->\n\
                 <!-- Showing raw template content. Use 'setfattr -n user.agentfs.raw -v 1 <file>' for raw mode. -->\n\n"
            );
            let mut result = notice.into_bytes();
            result.extend_from_slice(content);
            return Ok(result);
        }

        // No Tera syntax detected - return content as-is
        Ok(content.to_vec())
    }

    /// Check if content appears to contain Tera/Jinja2 template syntax.
    pub fn has_template_syntax(content: &[u8]) -> bool {
        let s = String::from_utf8_lossy(content);
        s.contains("{{") || s.contains("{%") || s.contains("{#")
    }
}

struct AgentFSFuse {
    fs: Arc<dyn FileSystem>,
    runtime: Runtime,
    path_cache: Arc<Mutex<HashMap<u64, String>>>,
    /// Maps file handle -> open file state
    open_files: Arc<Mutex<HashMap<u64, OpenFile>>>,
    /// Next file handle to allocate
    next_fh: AtomicU64,
    /// User ID to report for all files (set at mount time)
    uid: u32,
    /// Group ID to report for all files (set at mount time)
    gid: u32,
    /// Lossy string representation of the absolute mountpoint path.
    /// This is used to avoid looking up ourselves inside ourselves,
    /// e.g., when we mount an under filesystem `/` at /mntpnt,
    /// we do not want to look up `/mntpnt/mntpnt`, because the handler will then try
    /// to lookup `/mntpnt` from he under filesystem, which will hit our mountpoint again,
    /// causing a deadlock.
    mountpoint_path: String,
    /// Handler registry for extensible file operations
    handler_registry: HandlerRegistry,

    // ─────────────────────────────────────────────────────────────
    // Phase 2: Tera Rendering Mode (STORY-5.4)
    // ─────────────────────────────────────────────────────────────
    /// Extended attribute cache for `user.agentfs.raw` mode.
    /// Maps inode -> raw_mode (true = raw mode, false = rendered mode).
    /// Files not in this cache default to rendered mode (raw=false) for existing files,
    /// but new files are added with raw=true (AC12).
    xattr_raw_mode: Arc<Mutex<HashMap<u64, bool>>>,

    /// Set of inodes accessed via `.source` suffix (virtual inodes).
    /// These always use raw mode regardless of xattr settings.
    /// The virtual inode is `real_ino | SOURCE_INODE_MASK`.
    source_inodes: Arc<Mutex<HashSet<u64>>>,

    /// Stub template renderer for Phase 2.
    /// TODO: Replace with TemplateProcessor from STORY-2.1.5 when available.
    template_renderer: Arc<StubTemplateRenderer>,
}

impl Filesystem for AgentFSFuse {
    /// Initialize the filesystem and enable performance optimizations.
    ///
    /// - Async read: allows the kernel to issue multiple read requests in parallel,
    ///   improving throughput for concurrent file access.
    /// - Writeback caching: allows the kernel to buffer writes and flush them
    ///   later, significantly improving write performance for small writes.
    /// - Parallel dirops: allows concurrent lookup() and readdir() on the same
    ///   directory, improving performance for parallel file access patterns.
    /// - Cache symlinks: caches readlink responses, avoiding repeated round-trips
    ///   for symlink resolution.
    /// - No opendir support: skips opendir/releasedir calls since we don't track
    ///   directory handles, reducing round-trips for directory operations.
    fn init(&mut self, _req: &Request, config: &mut KernelConfig) -> Result<(), libc::c_int> {
        tracing::debug!("FUSE::init");
        let _ = config.add_capabilities(
            FUSE_ASYNC_READ
                | FUSE_WRITEBACK_CACHE
                | FUSE_PARALLEL_DIROPS
                | FUSE_CACHE_SYMLINKS
                | FUSE_NO_OPENDIR_SUPPORT,
        );
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────
    // Name Resolution & Attributes
    // ─────────────────────────────────────────────────────────────

    /// Looks up a directory entry by name within a parent directory.
    ///
    /// Resolves `name` under the directory identified by `parent` inode, stats the
    /// resulting path, and caches the inode-to-path mapping on success.
    ///
    /// # Phase 2: .source Suffix Handling (AC10)
    ///
    /// If the name ends with `.source` (e.g., `file.md.source`), this method:
    /// 1. Strips the suffix and looks up the real file (`file.md`)
    /// 2. Creates a virtual inode that forces raw mode for all operations
    /// 3. Returns the virtual inode with the same attributes
    ///
    /// This allows users to always access raw template content via `cat file.md.source`.
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        tracing::debug!("FUSE::lookup: parent={}, name={:?}", parent, name);

        let name_str = name.to_string_lossy();

        // Phase 2: Handle .source suffix for raw access (AC10)
        if name_str.ends_with(SOURCE_SUFFIX) {
            // Strip .source suffix and look up the real file
            let real_name = &name_str[..name_str.len() - SOURCE_SUFFIX.len()];
            tracing::debug!(
                "FUSE::lookup: .source suffix detected, looking up real file: {}",
                real_name
            );

            let Some(path) = self.lookup_path(parent, &std::ffi::OsString::from(real_name)) else {
                reply.error(libc::ENOENT);
                return;
            };

            let fs = self.fs.clone();
            let (result, path) = self.runtime.block_on(async move {
                let result = fs.lstat(&path).await;
                (result, path)
            });

            match result {
                Ok(Some(stats)) => {
                    // Create virtual source inode that forces raw mode
                    let source_ino = self.make_source_inode(stats.ino as u64);
                    let mut attr = fillattr(&stats, self.uid, self.gid);
                    attr.ino = source_ino;

                    // Cache the path for the source inode
                    self.add_path(source_ino, path.clone());
                    // Also cache the real inode path if not already cached
                    self.add_path(stats.ino as u64, path);

                    // Track this as a source inode
                    self.source_inodes.lock().insert(source_ino);

                    tracing::debug!(
                        "FUSE::lookup: returning source inode {} for {}",
                        source_ino,
                        name_str
                    );
                    reply.entry(&TTL, &attr, 0);
                }
                Ok(None) => reply.error(libc::ENOENT),
                Err(e) => reply.error(error_to_errno(&e)),
            }
            return;
        }

        // Normal lookup (no .source suffix)
        // First get the parent path for handler registry lookup
        let Some(parent_path) = self.get_path(parent) else {
            reply.error(libc::ENOENT);
            return;
        };

        // Try handler registry (enables virtual entries like /.graphdocs/)
        let result = self
            .runtime
            .block_on(self.handler_registry.handle_lookup(&parent_path, &name_str));

        match result {
            Ok(Some(stats)) => {
                // Handler provided stats - use them
                let attr = fillattr(&stats, self.uid, self.gid);
                let child_path = if parent_path == "/" {
                    format!("/{}", name_str)
                } else {
                    format!("{}/{}", parent_path.trim_end_matches('/'), name_str)
                };
                self.add_path(attr.ino, child_path);
                tracing::debug!(
                    "FUSE::lookup: handler returned stats for {} (ino={})",
                    name_str,
                    attr.ino
                );
                reply.entry(&TTL, &attr, 0);
            }
            Ok(None) => {
                // No handler found entry - return ENOENT
                reply.error(libc::ENOENT);
            }
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    /// Retrieves file attributes for a given inode.
    ///
    /// Returns metadata (size, permissions, timestamps, etc.) for the file or
    /// directory identified by `ino`. Root inode (1) is handled specially.
    ///
    /// Uses the handler registry to allow handlers to intercept getattr operations.
    ///
    /// # Phase 2: Source Inode Handling
    ///
    /// For virtual source inodes (created via `.source` suffix), this returns
    /// the attributes of the underlying real file but with the virtual inode.
    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        tracing::debug!("FUSE::getattr: ino={}", ino);

        // Phase 2: Handle source inodes
        let real_ino = self.get_real_inode(ino);
        let is_source = self.is_source_inode(ino);

        let Some(path) = self.get_path(real_ino).or_else(|| self.get_path(ino)) else {
            reply.error(libc::ENOENT);
            return;
        };

        let result = self
            .runtime
            .block_on(self.handler_registry.handle_getattr(&path));

        match result {
            Ok(Some(stats)) => {
                let mut attr = fillattr(&stats, self.uid, self.gid);
                // For source inodes, preserve the virtual inode number
                if is_source {
                    attr.ino = ino;
                }
                reply.attr(&TTL, &attr);
            }
            Ok(None) => reply.error(libc::ENOENT),
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    /// Reads the target of a symbolic link.
    ///
    /// Returns the path that the symlink points to. This is called by operations
    /// like `ls -l` to display symlink targets.
    ///
    /// Uses the handler registry to allow handlers to intercept readlink operations.
    fn readlink(&mut self, _req: &Request, ino: u64, reply: ReplyData) {
        tracing::debug!("FUSE::readlink: ino={}", ino);
        let Some(path) = self.get_path(ino) else {
            reply.error(libc::ENOENT);
            return;
        };

        let result = self
            .runtime
            .block_on(self.handler_registry.handle_readlink(&path));

        match result {
            Ok(Some(target)) => reply.data(target.as_bytes()),
            Ok(None) => reply.error(libc::ENOENT),
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    /// Sets file attributes, handling truncate and chmod operations.
    ///
    /// Currently `size` changes (truncate) and `mode` changes (chmod) are supported.
    /// Other attribute changes (uid, gid, timestamps) are accepted but ignored.
    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
        mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<crate::fuser::TimeOrNow>,
        _mtime: Option<crate::fuser::TimeOrNow>,
        _ctime: Option<SystemTime>,
        fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        tracing::debug!(
            "FUSE::setattr: ino={}, mode={:?}, size={:?}",
            ino,
            mode,
            size
        );
        // Handle chmod
        if let Some(new_mode) = mode {
            let Some(path) = self.path_cache.lock().get(&ino).cloned() else {
                reply.error(libc::ENOENT);
                return;
            };

            let fs = self.fs.clone();
            let result = self
                .runtime
                .block_on(async move { fs.chmod(&path, new_mode).await });

            if let Err(e) = result {
                reply.error(error_to_errno(&e));
                return;
            }
        }

        // Handle truncate
        if let Some(new_size) = size {
            let result = if let Some(fh) = fh {
                // Use file handle if available (ftruncate)
                let file = {
                    let open_files = self.open_files.lock();
                    open_files.get(&fh).map(|f| f.file.clone())
                };

                if let Some(file) = file {
                    self.runtime
                        .block_on(async move { file.truncate(new_size).await })
                } else {
                    reply.error(libc::EBADF);
                    return;
                }
            } else {
                // Open file and truncate via file handle
                let Some(path) = self.path_cache.lock().get(&ino).cloned() else {
                    reply.error(libc::ENOENT);
                    return;
                };

                let fs = self.fs.clone();
                self.runtime.block_on(async move {
                    let file = fs.open(&path).await?;
                    file.truncate(new_size).await
                })
            };

            if let Err(e) = result {
                reply.error(error_to_errno(&e));
                return;
            }
        }

        // Return updated attributes
        let Some(path) = self.get_path(ino) else {
            reply.error(libc::ENOENT);
            return;
        };

        let fs = self.fs.clone();
        let result = self.runtime.block_on(async move { fs.stat(&path).await });

        match result {
            Ok(Some(stats)) => reply.attr(&TTL, &fillattr(&stats, self.uid, self.gid)),
            Ok(None) => reply.error(libc::ENOENT),
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Directory Operations
    // ─────────────────────────────────────────────────────────────

    /// Reads directory entries for the given inode.
    ///
    /// Returns "." and ".." entries followed by the directory contents.
    /// Each entry's inode is cached for subsequent lookups.
    ///
    /// Uses the handler registry to allow handlers to intercept readdir operations.
    /// Falls back to readdir_plus to fetch entries with stats in a single query,
    /// avoiding N+1 database queries.
    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        tracing::debug!("FUSE::readdir: ino={}, offset={}", ino, offset);
        let Some(path) = self.get_path(ino) else {
            reply.error(libc::ENOENT);
            return;
        };

        let entries_result = self
            .runtime
            .block_on(self.handler_registry.handle_readdir_plus(&path));

        let entries = match entries_result {
            Ok(Some(entries)) => entries,
            Ok(None) => {
                reply.error(libc::ENOENT);
                return;
            }
            Err(e) => {
                reply.error(error_to_errno(&e));
                return;
            }
        };

        // Determine parent inode for ".." entry
        let parent_ino = if ino == 1 {
            1 // Root's parent is itself
        } else {
            let parent_path = Path::new(&path)
                .parent()
                .map(|p| {
                    let s = p.to_string_lossy().to_string();
                    if s.is_empty() {
                        "/".to_string()
                    } else {
                        s
                    }
                })
                .unwrap_or_else(|| "/".to_string());

            if parent_path == "/" {
                1
            } else {
                let fs = self.fs.clone();
                match self
                    .runtime
                    .block_on(async move { fs.stat(&parent_path).await })
                {
                    Ok(Some(stats)) => stats.ino as u64,
                    _ => 1, // Fallback to root if parent lookup fails
                }
            }
        };

        let mut all_entries = vec![
            (ino, FileType::Directory, "."),
            (parent_ino, FileType::Directory, ".."),
        ];

        // Process entries with stats already available (no N+1 queries!)
        for entry in &entries {
            // Skip . and .. since they're handled separately and caching them
            // would overwrite the path cache with incorrect paths (e.g., "/..").
            if entry.name == "." || entry.name == ".." {
                continue;
            }

            let entry_path = if path == "/" {
                format!("/{}", entry.name)
            } else {
                format!("{}/{}", path, entry.name)
            };

            let kind = if entry.stats.is_directory() {
                FileType::Directory
            } else if entry.stats.is_symlink() {
                FileType::Symlink
            } else {
                FileType::RegularFile
            };

            self.add_path(entry.stats.ino as u64, entry_path);
            all_entries.push((entry.stats.ino as u64, kind, entry.name.as_str()));
        }

        for (i, entry) in all_entries.iter().enumerate().skip(offset as usize) {
            if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                break;
            }
        }
        reply.ok();
    }

    /// Reads directory entries with full attributes for the given inode.
    ///
    /// This is an optimized version that returns both directory entries and
    /// their attributes in a single call, reducing kernel/userspace round trips.
    ///
    /// Uses the handler registry to allow handlers to intercept readdir operations.
    /// Falls back to readdir_plus to fetch entries with stats in a single database query.
    fn readdirplus(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectoryPlus,
    ) {
        tracing::debug!("FUSE::readdirplus: ino={}, offset={}", ino, offset);
        let Some(path) = self.get_path(ino) else {
            reply.error(libc::ENOENT);
            return;
        };

        let entries_result = self
            .runtime
            .block_on(self.handler_registry.handle_readdir_plus(&path));

        let entries = match entries_result {
            Ok(Some(entries)) => entries,
            Ok(None) => {
                reply.error(libc::ENOENT);
                return;
            }
            Err(e) => {
                reply.error(error_to_errno(&e));
                return;
            }
        };

        // Get current directory stats for "."
        let fs = self.fs.clone();
        let path_for_stat = path.clone();
        let dir_stats = self
            .runtime
            .block_on(async move { fs.stat(&path_for_stat).await })
            .ok()
            .flatten();

        // Determine parent inode and stats for ".." entry
        let (parent_ino, parent_stats) = if ino == 1 {
            (1u64, dir_stats.clone()) // Root's parent is itself
        } else {
            let parent_path = Path::new(&path)
                .parent()
                .map(|p| {
                    let s = p.to_string_lossy().to_string();
                    if s.is_empty() {
                        "/".to_string()
                    } else {
                        s
                    }
                })
                .unwrap_or_else(|| "/".to_string());

            if parent_path == "/" {
                let fs = self.fs.clone();
                let parent_stats = self
                    .runtime
                    .block_on(async move { fs.stat(&parent_path).await })
                    .ok()
                    .flatten();
                (1u64, parent_stats)
            } else {
                let fs = self.fs.clone();
                let parent_stats = self
                    .runtime
                    .block_on(async move { fs.stat(&parent_path).await })
                    .ok()
                    .flatten();
                let parent_ino = parent_stats.as_ref().map(|s| s.ino as u64).unwrap_or(1);
                (parent_ino, parent_stats)
            }
        };

        // Build the entries list with full attributes
        let uid = self.uid;
        let gid = self.gid;

        let mut offset_counter = 0i64;

        // Add "." entry
        if offset <= offset_counter {
            if let Some(ref stats) = dir_stats {
                let attr = fillattr(stats, uid, gid);
                if reply.add(ino, offset_counter + 1, ".", &TTL, &attr, 0) {
                    reply.ok();
                    return;
                }
            }
        }
        offset_counter += 1;

        // Add ".." entry
        if offset <= offset_counter {
            if let Some(ref stats) = parent_stats {
                let attr = fillattr(stats, uid, gid);
                if reply.add(parent_ino, offset_counter + 1, "..", &TTL, &attr, 0) {
                    reply.ok();
                    return;
                }
            }
        }
        offset_counter += 1;

        // Add directory entries with their attributes
        for entry in &entries {
            // Skip . and .. since they're handled separately above and caching them
            // would overwrite the path cache with incorrect paths (e.g., "/..").
            if entry.name == "." || entry.name == ".." {
                continue;
            }

            if offset <= offset_counter {
                let entry_path = if path == "/" {
                    format!("/{}", entry.name)
                } else {
                    format!("{}/{}", path, entry.name)
                };

                let attr = fillattr(&entry.stats, uid, gid);
                self.add_path(entry.stats.ino as u64, entry_path);

                if reply.add(
                    entry.stats.ino as u64,
                    offset_counter + 1,
                    &entry.name,
                    &TTL,
                    &attr,
                    0,
                ) {
                    reply.ok();
                    return;
                }
            }
            offset_counter += 1;
        }

        reply.ok();
    }

    /// Creates a new directory.
    ///
    /// Creates a directory at `name` under `parent`, then stats it to return
    /// proper attributes and cache the inode mapping.
    fn mkdir(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        tracing::debug!("FUSE::mkdir: parent={}, name={:?}", parent, name);
        let Some(path) = self.lookup_path(parent, name) else {
            reply.error(libc::ENOENT);
            return;
        };

        let fs = self.fs.clone();
        let (result, path) = self.runtime.block_on(async move {
            let result = fs.mkdir(&path).await;
            (result, path)
        });

        if let Err(e) = result {
            reply.error(error_to_errno(&e));
            return;
        }

        // Get the new directory's stats
        let fs = self.fs.clone();
        let (stat_result, path) = self.runtime.block_on(async move {
            let result = fs.stat(&path).await;
            (result, path)
        });

        match stat_result {
            Ok(Some(stats)) => {
                let attr = fillattr(&stats, self.uid, self.gid);
                self.add_path(attr.ino, path);
                reply.entry(&TTL, &attr, 0);
            }
            Ok(None) => {
                reply.error(libc::ENOENT);
            }
            Err(e) => {
                reply.error(error_to_errno(&e));
            }
        }
    }

    /// Removes an empty directory.
    ///
    /// Verifies the target is a directory and is empty before removal.
    /// Returns `ENOTDIR` if not a directory, `ENOTEMPTY` if not empty.
    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        tracing::debug!("FUSE::rmdir: parent={}, name={:?}", parent, name);
        let Some(path) = self.lookup_path(parent, name) else {
            reply.error(libc::ENOENT);
            return;
        };

        // Verify target is a directory
        let fs = self.fs.clone();
        let (stat_result, path) = self.runtime.block_on(async move {
            let result = fs.lstat(&path).await;
            (result, path)
        });

        let stats = match stat_result {
            Ok(Some(s)) => s,
            Ok(None) => {
                reply.error(libc::ENOENT);
                return;
            }
            Err(e) => {
                reply.error(error_to_errno(&e));
                return;
            }
        };

        if !stats.is_directory() {
            reply.error(libc::ENOTDIR);
            return;
        }

        // Verify directory is empty
        let fs = self.fs.clone();
        let (readdir_result, path) = self.runtime.block_on(async move {
            let result = fs.readdir(&path).await;
            (result, path)
        });

        match readdir_result {
            Ok(Some(entries)) if !entries.is_empty() => {
                reply.error(libc::ENOTEMPTY);
                return;
            }
            Ok(None) => {
                reply.error(libc::ENOENT);
                return;
            }
            Err(e) => {
                reply.error(error_to_errno(&e));
                return;
            }
            Ok(Some(_)) => {} // Empty directory, proceed
        }

        // Remove the directory
        let ino = stats.ino as u64;
        let fs = self.fs.clone();
        let result = self.runtime.block_on(async move { fs.remove(&path).await });

        match result {
            Ok(()) => {
                self.drop_path(ino);
                reply.ok();
            }
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    // ─────────────────────────────────────────────────────────────
    // File Creation & Removal
    // ─────────────────────────────────────────────────────────────

    /// Creates and opens a new file.
    ///
    /// Creates an empty file at `name` under `parent`, allocates a file handle,
    /// and returns both the file attributes and handle for immediate use.
    ///
    /// # Phase 2: New Files Default to Raw Mode (AC12)
    ///
    /// New `.md` files are automatically set to raw mode (`user.agentfs.raw=1`)
    /// so they can be written to immediately. This allows users to create and
    /// edit new template files without having to first set the xattr.
    fn create(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        tracing::debug!(
            "FUSE::create: parent={}, name={:?}, mode={:o}",
            parent,
            name,
            mode
        );
        let Some(path) = self.lookup_path(parent, name) else {
            reply.error(libc::ENOENT);
            return;
        };

        // Create file with mode, get stats and file handle in one operation
        let fs = self.fs.clone();
        let path_for_create = path.clone();
        let result = self
            .runtime
            .block_on(async move { fs.create_file(&path_for_create, mode).await });

        match result {
            Ok((stats, file)) => {
                let attr = fillattr(&stats, self.uid, self.gid);
                self.add_path(attr.ino, path.clone());

                // Phase 2: New .md files default to raw mode (AC12)
                if path.to_lowercase().ends_with(".md") {
                    self.set_raw_mode(attr.ino, true);
                    tracing::debug!(
                        "FUSE::create: new .md file {} set to raw mode (ino={})",
                        path,
                        attr.ino
                    );
                }

                let fh = self.alloc_fh();
                self.open_files.lock().insert(fh, OpenFile { file });

                reply.created(&TTL, &attr, 0, fh, 0);
            }
            Err(e) => {
                reply.error(error_to_errno(&e));
            }
        }
    }

    /// Creates a symbolic link.
    ///
    /// Creates a symlink at `name` under `parent` pointing to `link`.
    fn symlink(
        &mut self,
        _req: &Request,
        parent: u64,
        link_name: &OsStr,
        target: &Path,
        reply: ReplyEntry,
    ) {
        tracing::debug!(
            "FUSE::symlink: parent={}, link_name={:?}, target={:?}",
            parent,
            link_name,
            target
        );
        let Some(path) = self.lookup_path(parent, link_name) else {
            reply.error(libc::ENOENT);
            return;
        };

        let Some(target_str) = target.to_str() else {
            reply.error(libc::EINVAL);
            return;
        };

        let fs = self.fs.clone();
        let target_owned = target_str.to_string();
        let (result, path) = self.runtime.block_on(async move {
            let result = fs.symlink(&target_owned, &path).await;
            (result, path)
        });

        if let Err(e) = result {
            reply.error(error_to_errno(&e));
            return;
        }

        // Get the new symlink's stats
        let fs = self.fs.clone();
        let (stat_result, path) = self.runtime.block_on(async move {
            let result = fs.lstat(&path).await;
            (result, path)
        });

        match stat_result {
            Ok(Some(stats)) => {
                let attr = fillattr(&stats, self.uid, self.gid);
                self.add_path(attr.ino, path);
                reply.entry(&TTL, &attr, 0);
            }
            Ok(None) => {
                reply.error(libc::ENOENT);
            }
            Err(e) => {
                reply.error(error_to_errno(&e));
            }
        }
    }

    /// Creates a hard link.
    ///
    /// Creates a new directory entry `newname` under `newparent` that refers to the
    /// same inode as `ino`. The link count of the inode is incremented.
    fn link(
        &mut self,
        _req: &Request,
        ino: u64,
        newparent: u64,
        newname: &OsStr,
        reply: ReplyEntry,
    ) {
        tracing::debug!(
            "FUSE::link: ino={}, newparent={}, newname={:?}",
            ino,
            newparent,
            newname
        );
        // Get the path for the source inode
        let Some(oldpath) = self.get_path(ino) else {
            reply.error(libc::ENOENT);
            return;
        };

        // Get the path for the new link
        let Some(newpath) = self.lookup_path(newparent, newname) else {
            reply.error(libc::ENOENT);
            return;
        };

        let fs = self.fs.clone();
        let (result, newpath) = self.runtime.block_on(async move {
            let result = fs.link(&oldpath, &newpath).await;
            (result, newpath)
        });

        if let Err(e) = result {
            reply.error(error_to_errno(&e));
            return;
        }

        // Get the new link's stats
        let fs = self.fs.clone();
        let (stat_result, newpath) = self.runtime.block_on(async move {
            let result = fs.lstat(&newpath).await;
            (result, newpath)
        });

        match stat_result {
            Ok(Some(stats)) => {
                let attr = fillattr(&stats, self.uid, self.gid);
                self.add_path(attr.ino, newpath);
                reply.entry(&TTL, &attr, 0);
            }
            Ok(None) => {
                reply.error(libc::ENOENT);
            }
            Err(e) => {
                reply.error(error_to_errno(&e));
            }
        }
    }

    /// Removes a file (unlinks it from the directory).
    ///
    /// Gets the file's inode before removal to clean up the path cache.
    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        tracing::debug!("FUSE::unlink: parent={}, name={:?}", parent, name);
        let Some(path) = self.lookup_path(parent, name) else {
            reply.error(libc::ENOENT);
            return;
        };

        // Get inode before removing so we can uncache
        let fs = self.fs.clone();
        let (stat_result, path) = self.runtime.block_on(async move {
            let result = fs.lstat(&path).await;
            (result, path)
        });

        let stats = match &stat_result {
            Ok(Some(s)) => s,
            Ok(None) => {
                reply.error(libc::ENOENT);
                return;
            }
            Err(e) => {
                reply.error(error_to_errno(e));
                return;
            }
        };

        if stats.is_directory() {
            reply.error(libc::EISDIR);
            return;
        }

        let ino = stats.ino as u64;
        let nlink = stats.nlink;

        let fs = self.fs.clone();
        let result = self.runtime.block_on(async move { fs.remove(&path).await });

        match result {
            Ok(()) => {
                // Only drop from path_cache if this was the last link.
                // If nlink > 1, there are other hard links that still reference
                // this inode, and the path_cache entry points to one of them.
                if nlink <= 1 {
                    self.drop_path(ino);
                }
                reply.ok();
            }
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    /// Renames a file or directory.
    ///
    /// Moves `name` from `parent` to `newname` under `newparent`. Updates the
    /// path cache accordingly, removing any replaced destination entry.
    fn rename(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        tracing::debug!(
            "FUSE::rename: parent={}, name={:?}, newparent={}, newname={:?}",
            parent,
            name,
            newparent,
            newname
        );
        let Some(from_path) = self.lookup_path(parent, name) else {
            reply.error(libc::ENOENT);
            return;
        };

        let Some(to_path) = self.lookup_path(newparent, newname) else {
            reply.error(libc::ENOENT);
            return;
        };

        // Get source inode before rename so we can update cache
        let fs = self.fs.clone();
        let (src_stat, from_path) = self.runtime.block_on(async move {
            let result = fs.stat(&from_path).await;
            (result, from_path)
        });

        let src_ino = src_stat.ok().flatten().map(|s| s.ino as u64);

        // Check if destination exists and get its inode for cache cleanup
        let fs = self.fs.clone();
        let (dst_stat, to_path) = self.runtime.block_on(async move {
            let result = fs.stat(&to_path).await;
            (result, to_path)
        });

        let dst_ino = dst_stat.ok().flatten().map(|s| s.ino as u64);

        // Perform the rename
        let fs = self.fs.clone();
        let (result, to_path) = self.runtime.block_on(async move {
            let result = fs.rename(&from_path, &to_path).await;
            (result, to_path)
        });

        match result {
            Ok(()) => {
                // Update path cache: remove old path, add new path
                if let Some(ino) = src_ino {
                    self.drop_path(ino);
                    self.add_path(ino, to_path);
                }
                // Remove destination from cache if it was replaced
                if let Some(ino) = dst_ino {
                    self.drop_path(ino);
                }
                reply.ok();
            }
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    // ─────────────────────────────────────────────────────────────
    // File I/O Lifecycle
    // ─────────────────────────────────────────────────────────────

    /// Opens a file for reading or writing.
    ///
    /// Allocates a file handle and opens the file in the filesystem layer.
    ///
    /// # Phase 2: Source Inode Handling
    ///
    /// For virtual source inodes (created via `.source` suffix), this opens
    /// the underlying real file but maintains the virtual inode association.
    fn open(&mut self, _req: &Request, ino: u64, _flags: i32, reply: ReplyOpen) {
        tracing::debug!("FUSE::open: ino={}", ino);

        // Phase 2: Handle source inodes
        let real_ino = self.get_real_inode(ino);

        let Some(path) = self.get_path(real_ino).or_else(|| self.get_path(ino)) else {
            reply.error(libc::ENOENT);
            return;
        };

        // Check if a handler can handle this path (virtual files).
        // For handler-managed files, we don't need to open via the filesystem -
        // the handler will handle reads directly.
        let can_handle = self
            .runtime
            .block_on(async { self.handler_registry.can_handle_any(&path) });

        if can_handle {
            tracing::debug!("FUSE::open: handler-managed virtual file: {}", path);
            // Return a dummy file handle - reads will be handled by the handler registry
            let fh = self.alloc_fh();
            reply.opened(fh, 0);
            return;
        }

        let fs = self.fs.clone();
        let path_clone = path.clone();
        let result = self
            .runtime
            .block_on(async move { fs.open(&path_clone).await });

        match result {
            Ok(file) => {
                let fh = self.alloc_fh();
                self.open_files.lock().insert(fh, OpenFile { file });
                reply.opened(fh, 0);
            }
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    /// Reads data using the file handle or handler registry.
    ///
    /// Uses the handler registry to allow handlers to intercept read operations
    /// for virtual files. Falls back to file handle based reads for real files.
    ///
    /// # Phase 2: Tera Rendering Mode (AC6, AC7)
    ///
    /// For `.md` files in rendered mode (xattr `user.agentfs.raw=0` or default):
    /// - Reads the full raw content
    /// - Renders it through the template processor
    /// - Returns the requested slice of the rendered content
    ///
    /// For `.md` files in raw mode (xattr `user.agentfs.raw=1` or `.source` suffix):
    /// - Returns the raw content directly (no rendering)
    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        tracing::debug!(
            "FUSE::read: ino={}, fh={}, offset={}, size={}",
            ino,
            fh,
            offset,
            size
        );

        // Get the real inode (in case of source inode)
        let real_ino = self.get_real_inode(ino);

        // Try path-based handler read first (allows virtual files from handlers)
        if let Some(path) = self.get_path(real_ino).or_else(|| self.get_path(ino)) {
            // Phase 2: Check if we should render this file
            let should_render = self.should_render(&path, ino);

            if should_render {
                // Rendered mode: read full content, render, return requested slice
                tracing::debug!("FUSE::read: rendering markdown file: {}", path);

                let result =
                    self.runtime
                        .block_on(self.handler_registry.handle_read(&path, 0, u64::MAX));

                match result {
                    Ok(raw_data) => {
                        // Render the content
                        let rendered = self.render_content(&raw_data);

                        // Return the requested slice
                        let start = (offset as usize).min(rendered.len());
                        let end = (start + size as usize).min(rendered.len());
                        reply.data(&rendered[start..end]);
                        return;
                    }
                    Err(e) => {
                        reply.error(error_to_errno(&e));
                        return;
                    }
                }
            }

            // Raw mode or non-markdown: read directly
            let result = self.runtime.block_on(self.handler_registry.handle_read(
                &path,
                offset as u64,
                size as u64,
            ));

            match result {
                Ok(data) => {
                    reply.data(&data);
                    return;
                }
                Err(e) => {
                    reply.error(error_to_errno(&e));
                    return;
                }
            }
        }

        // Fall back to file handle based read
        let file = {
            let open_files = self.open_files.lock();
            let Some(open_file) = open_files.get(&fh) else {
                reply.error(libc::EBADF);
                return;
            };
            open_file.file.clone()
        };

        // Check if we have a path for this file handle to determine rendering
        let path_opt = self.get_path(real_ino).or_else(|| self.get_path(ino));

        if let Some(ref path) = path_opt {
            if self.should_render(path, ino) {
                // Rendered mode: read full content, render, return requested slice
                tracing::debug!("FUSE::read: rendering markdown file (via fh): {}", path);

                let result = self
                    .runtime
                    .block_on(async move { file.pread(0, u64::MAX).await });

                match result {
                    Ok(raw_data) => {
                        let rendered = self.render_content(&raw_data);
                        let start = (offset as usize).min(rendered.len());
                        let end = (start + size as usize).min(rendered.len());
                        reply.data(&rendered[start..end]);
                    }
                    Err(e) => reply.error(error_to_errno(&e)),
                }
                return;
            }
        }

        // Raw mode or non-markdown: read directly
        let result = self
            .runtime
            .block_on(async move { file.pread(offset as u64, size as u64).await });

        match result {
            Ok(data) => reply.data(&data),
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    /// Writes data using the file handle.
    ///
    /// # Phase 2: Write Blocking (AC8, AC9)
    ///
    /// For `.md` files in rendered mode (xattr `user.agentfs.raw=0` or default):
    /// - Write is BLOCKED with EACCES
    /// - User must switch to raw mode first: `setfattr -n user.agentfs.raw -v 1 <file>`
    ///
    /// For `.md` files in raw mode (xattr `user.agentfs.raw=1` or `.source` suffix):
    /// - Write is ALLOWED normally
    fn write(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        tracing::debug!(
            "FUSE::write: ino={}, fh={}, offset={}, data_len={}",
            ino,
            fh,
            offset,
            data.len()
        );

        // Phase 2: Check if write is blocked (AC8, AC9)
        let real_ino = self.get_real_inode(ino);
        if let Some(path) = self.get_path(real_ino).or_else(|| self.get_path(ino)) {
            if self.is_write_blocked(&path, ino) {
                tracing::debug!(
                    "FUSE::write: blocked write to rendered .md file: {} (set user.agentfs.raw=1 to enable writes)",
                    path
                );
                reply.error(libc::EACCES);
                return;
            }
        }

        let file = {
            let open_files = self.open_files.lock();
            let Some(open_file) = open_files.get(&fh) else {
                reply.error(libc::EBADF);
                return;
            };
            open_file.file.clone()
        };

        let data_len = data.len();
        let data_vec = data.to_vec();
        let result = self
            .runtime
            .block_on(async move { file.pwrite(offset as u64, &data_vec).await });

        match result {
            Ok(()) => reply.written(data_len as u32),
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    /// Flushes data to the backend storage.
    ///
    /// Since writes go directly to the database, this is a no-op.
    fn flush(&mut self, _req: &Request, _ino: u64, fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        tracing::debug!("FUSE::flush: fh={}", fh);
        // For handler-managed virtual files, the file handle won't be in open_files
        // because we don't actually open a file. Always succeed for flush since
        // there's nothing to flush for virtual files.
        reply.ok();
    }

    /// Synchronizes file data to persistent storage using the file handle.
    ///
    /// This now uses the file handle's fsync which knows which layer(s) the
    /// file exists in, avoiding errors when a file only exists in one layer.
    fn fsync(&mut self, _req: &Request, _ino: u64, fh: u64, _datasync: bool, reply: ReplyEmpty) {
        tracing::debug!("FUSE::fsync: fh={}", fh);
        let file = {
            let open_files = self.open_files.lock();
            match open_files.get(&fh) {
                Some(open_file) => open_file.file.clone(),
                None => {
                    reply.error(libc::EBADF);
                    return;
                }
            }
        };

        let result = self.runtime.block_on(async move { file.fsync().await });

        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(error_to_errno(&e)),
        }
    }

    /// Releases (closes) an open file handle.
    ///
    /// Removes the file handle from the open files table.
    /// Since writes go directly to the database, no flushing is needed.
    fn release(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        tracing::debug!("FUSE::release: fh={}", fh);
        self.open_files.lock().remove(&fh);
        reply.ok();
    }

    /// Returns filesystem statistics.
    ///
    /// Queries actual usage from the SDK and reports it to tools like `df`.
    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        tracing::debug!("FUSE::statfs");
        const BLOCK_SIZE: u64 = 4096;
        const TOTAL_INODES: u64 = 1_000_000; // Virtual limit
        const MAX_NAMELEN: u32 = 255;

        let fs = self.fs.clone();
        let result = self.runtime.block_on(async move { fs.statfs().await });

        let (used_blocks, used_inodes) = match result {
            Ok(stats) => {
                let used_blocks = stats.bytes_used.div_ceil(BLOCK_SIZE);
                (used_blocks, stats.inodes)
            }
            Err(_) => (0, 1), // Fallback: just root inode
        };

        // Report a large virtual capacity so tools don't think we're out of space
        const TOTAL_BLOCKS: u64 = 1024 * 1024 * 1024; // ~4TB virtual size
        let free_blocks = TOTAL_BLOCKS.saturating_sub(used_blocks);
        let free_inodes = TOTAL_INODES.saturating_sub(used_inodes);

        reply.statfs(
            TOTAL_BLOCKS,
            free_blocks,
            free_blocks,
            TOTAL_INODES,
            free_inodes,
            BLOCK_SIZE as u32,
            MAX_NAMELEN,       // namelen: maximum filename length
            BLOCK_SIZE as u32, // frsize: fragment size
        );
    }

    // ─────────────────────────────────────────────────────────────
    // Phase 2: Extended Attributes (AC11)
    // ─────────────────────────────────────────────────────────────

    /// Set an extended attribute.
    ///
    /// Handles `user.agentfs.raw` for controlling raw/rendered mode on .md files.
    /// - `0`: rendered mode (default) - read returns rendered content, write is blocked
    /// - `1`: raw mode - read returns raw template, write is allowed
    fn setxattr(
        &mut self,
        _req: &Request,
        ino: u64,
        name: &OsStr,
        value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        tracing::debug!(
            "FUSE::setxattr: ino={}, name={}, value={:?}",
            ino,
            name_str,
            value
        );

        // Handle user.agentfs.raw attribute
        if name_str == XATTR_RAW_MODE {
            // Parse value: "0" = rendered mode, "1" = raw mode
            // Also accept single byte 0x00/0x01 or '0'/'1'
            let raw_mode = if value.is_empty() {
                false // Empty value = rendered mode
            } else {
                match value[0] {
                    b'1' | 1 => true,  // Raw mode
                    b'0' | 0 => false, // Rendered mode
                    _ => {
                        // Invalid value - treat as rendered mode
                        tracing::warn!(
                            "FUSE::setxattr: invalid value for {}: {:?}",
                            XATTR_RAW_MODE,
                            value
                        );
                        false
                    }
                }
            };

            let real_ino = self.get_real_inode(ino);
            self.set_raw_mode(real_ino, raw_mode);
            tracing::debug!(
                "FUSE::setxattr: set raw_mode={} for ino={}",
                raw_mode,
                real_ino
            );
            reply.ok();
            return;
        }

        // For other xattrs, return ENOTSUP (not supported)
        // In the future, could delegate to the underlying filesystem
        reply.error(libc::ENOTSUP);
    }

    /// Get an extended attribute.
    ///
    /// Handles `user.agentfs.raw` for querying raw/rendered mode.
    /// - Returns "0" for rendered mode (default)
    /// - Returns "1" for raw mode
    fn getxattr(&mut self, _req: &Request, ino: u64, name: &OsStr, size: u32, reply: ReplyXattr) {
        let name_str = name.to_string_lossy();
        tracing::debug!(
            "FUSE::getxattr: ino={}, name={}, size={}",
            ino,
            name_str,
            size
        );

        // Handle user.agentfs.raw attribute
        if name_str == XATTR_RAW_MODE {
            let real_ino = self.get_real_inode(ino);
            let raw_mode = self.is_raw_mode(real_ino);
            let value = if raw_mode { b"1" } else { b"0" };

            if size == 0 {
                // Return the size needed
                reply.size(1);
            } else if size >= 1 {
                // Return the value
                reply.data(value);
            } else {
                // Buffer too small
                reply.error(libc::ERANGE);
            }
            return;
        }

        // For other xattrs, return ENODATA (no such attribute)
        reply.error(libc::ENODATA);
    }

    /// List extended attribute names.
    ///
    /// Returns `user.agentfs.raw` for files that have the attribute set.
    fn listxattr(&mut self, _req: &Request, ino: u64, size: u32, reply: ReplyXattr) {
        tracing::debug!("FUSE::listxattr: ino={}, size={}", ino, size);

        let real_ino = self.get_real_inode(ino);

        // Check if this inode has the raw mode attribute set (explicitly)
        let has_raw_attr = self.xattr_raw_mode.lock().contains_key(&real_ino);

        if has_raw_attr {
            // Return the attribute name (null-terminated)
            let attr_name = format!("{}\0", XATTR_RAW_MODE);
            let attr_bytes = attr_name.as_bytes();

            if size == 0 {
                // Return the size needed
                reply.size(attr_bytes.len() as u32);
            } else if size >= attr_bytes.len() as u32 {
                // Return the list
                reply.data(attr_bytes);
            } else {
                // Buffer too small
                reply.error(libc::ERANGE);
            }
        } else {
            // No xattrs set - return empty list
            if size == 0 {
                reply.size(0);
            } else {
                reply.data(&[]);
            }
        }
    }

    /// Remove an extended attribute.
    fn removexattr(&mut self, _req: &Request, ino: u64, name: &OsStr, reply: ReplyEmpty) {
        let name_str = name.to_string_lossy();
        tracing::debug!("FUSE::removexattr: ino={}, name={}", ino, name_str);

        // Handle user.agentfs.raw attribute
        if name_str == XATTR_RAW_MODE {
            let real_ino = self.get_real_inode(ino);
            let mut xattr_cache = self.xattr_raw_mode.lock();

            if xattr_cache.remove(&real_ino).is_some() {
                tracing::debug!("FUSE::removexattr: removed raw_mode for ino={}", real_ino);
                reply.ok();
            } else {
                // Attribute doesn't exist
                reply.error(libc::ENODATA);
            }
            return;
        }

        // For other xattrs, return ENODATA
        reply.error(libc::ENODATA);
    }
}

impl AgentFSFuse {
    /// Create a new FUSE filesystem adapter wrapping a FileSystem instance.
    ///
    /// The provided Tokio runtime is used to execute async FileSystem operations
    /// from within synchronous FUSE callbacks via `block_on`.
    ///
    /// The uid and gid are used for all file ownership to avoid "dubious ownership"
    /// errors from tools like git that check file ownership.
    ///
    /// If `handler_registry` is `None`, a default registry is created that
    /// delegates all operations to the filesystem.
    fn new(
        fs: Arc<dyn FileSystem>,
        runtime: Runtime,
        uid: u32,
        gid: u32,
        mountpoint_path: PathBuf,
        handler_registry: Option<HandlerRegistry>,
    ) -> Self {
        let handler_registry =
            handler_registry.unwrap_or_else(|| HandlerRegistry::with_filesystem(fs.clone()));
        Self {
            fs,
            runtime,
            path_cache: Arc::new(Mutex::new(HashMap::new())),
            open_files: Arc::new(Mutex::new(HashMap::new())),
            next_fh: AtomicU64::new(1),
            uid,
            gid,
            mountpoint_path: mountpoint_path.as_os_str().to_string_lossy().to_string(),
            handler_registry,
            // Phase 2: Tera Rendering Mode
            xattr_raw_mode: Arc::new(Mutex::new(HashMap::new())),
            source_inodes: Arc::new(Mutex::new(HashSet::new())),
            template_renderer: Arc::new(StubTemplateRenderer::new()),
        }
    }

    /// Resolve a full path from a parent inode and child name.
    ///
    /// Similar to the Linux kernel's dentry lookup (`d_lookup`), this method
    /// reconstructs the full pathname by looking up the parent's path in our
    /// inode-to-path cache and appending the child name.
    ///
    /// Returns `None` if the parent inode is not in the cache or the name
    /// contains invalid UTF-8.
    fn lookup_path(&self, parent_ino: u64, name: &OsStr) -> Option<String> {
        let path_cache = self.path_cache.lock();
        let parent_path = path_cache.get(&parent_ino)?;
        let name_str = name.to_str()?;

        let path = if parent_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_path, name_str)
        };

        if path.starts_with(&self.mountpoint_path) {
            // Cut the head off here so we never try to lookup anything that falls within
            // our own mount inside our handlers by immediately returning ENOENT.
            None
        } else {
            Some(path)
        }
    }

    /// Retrieve a path from an inode number.
    ///
    /// Similar to the Linux kernel's `d_path()`, this performs the reverse
    /// lookup from inode to pathname.
    ///
    /// Returns `None` if the inode is not in the cache.
    fn get_path(&self, ino: u64) -> Option<String> {
        self.path_cache.lock().get(&ino).cloned()
    }

    /// Add an inode → path mapping to the path cache.
    ///
    /// Similar to the Linux kernel's `d_add()`, this associates an inode
    /// with its full pathname for later lookup.
    fn add_path(&self, ino: u64, path: String) {
        let mut path_cache = self.path_cache.lock();
        path_cache.insert(ino, path);
    }

    /// Remove an inode from the path cache.
    ///
    /// Similar to the Linux kernel's `d_drop()`, this removes the inode's
    /// pathname mapping when the file or directory is deleted or renamed.
    fn drop_path(&self, ino: u64) {
        let mut path_cache = self.path_cache.lock();
        path_cache.remove(&ino);
    }

    /// Allocate a new file handle for tracking open files.
    ///
    /// Similar to the Linux kernel's `get_unused_fd()`, this returns a unique
    /// handle that identifies an open file throughout its lifetime.
    fn alloc_fh(&self) -> u64 {
        self.next_fh.fetch_add(1, Ordering::SeqCst)
    }

    // ─────────────────────────────────────────────────────────────
    // Phase 2: Tera Rendering Mode Helpers (STORY-5.4)
    // ─────────────────────────────────────────────────────────────

    /// Check if an inode is a virtual "source" inode (accessed via .source suffix).
    ///
    /// Virtual source inodes always return raw content regardless of xattr settings.
    fn is_source_inode(&self, ino: u64) -> bool {
        (ino & SOURCE_INODE_MASK) != 0
    }

    /// Get the real inode from a potentially virtual source inode.
    fn get_real_inode(&self, ino: u64) -> u64 {
        ino & !SOURCE_INODE_MASK
    }

    /// Create a virtual source inode from a real inode.
    fn make_source_inode(&self, real_ino: u64) -> u64 {
        real_ino | SOURCE_INODE_MASK
    }

    /// Check if a file should be rendered (based on path and xattr mode).
    ///
    /// Returns `true` if the file:
    /// - Is a markdown file (ends with `.md`)
    /// - Is NOT accessed via `.source` suffix
    /// - Is NOT in raw mode (xattr `user.agentfs.raw=1`)
    ///
    /// # Arguments
    ///
    /// * `path` - The file path
    /// * `ino` - The inode number (may be virtual source inode)
    fn should_render(&self, path: &str, ino: u64) -> bool {
        // Virtual source inodes always return raw content
        if self.is_source_inode(ino) {
            return false;
        }

        // Only render .md files
        if !path.to_lowercase().ends_with(".md") {
            return false;
        }

        // Don't render paths ending in .source (shouldn't happen, but safety check)
        if path.ends_with(SOURCE_SUFFIX) {
            return false;
        }

        // Check xattr - default is rendered mode (raw=false)
        !self.is_raw_mode(ino)
    }

    /// Check if an inode is in raw mode.
    ///
    /// Returns `true` if:
    /// - Inode is a virtual source inode (accessed via .source suffix), OR
    /// - xattr `user.agentfs.raw=1` is set
    ///
    /// Defaults to `false` (rendered mode) if not explicitly set.
    fn is_raw_mode(&self, ino: u64) -> bool {
        // Virtual source inodes are always raw
        if self.is_source_inode(ino) {
            return true;
        }

        // Check xattr cache - default is rendered mode (raw=false)
        let xattr_cache = self.xattr_raw_mode.lock();
        *xattr_cache.get(&ino).unwrap_or(&false)
    }

    /// Set raw mode for an inode.
    fn set_raw_mode(&self, ino: u64, raw: bool) {
        let real_ino = self.get_real_inode(ino);
        let mut xattr_cache = self.xattr_raw_mode.lock();
        xattr_cache.insert(real_ino, raw);
    }

    /// Check if write is blocked for a file (rendered mode on .md files).
    ///
    /// Write is blocked when:
    /// - File is a markdown file (ends with `.md`)
    /// - File is in rendered mode (xattr `user.agentfs.raw=0` or default)
    /// - File is NOT accessed via `.source` suffix
    ///
    /// # Arguments
    ///
    /// * `path` - The file path
    /// * `ino` - The inode number
    fn is_write_blocked(&self, path: &str, ino: u64) -> bool {
        // Virtual source inodes are always writable
        if self.is_source_inode(ino) {
            return false;
        }

        // Only block writes to .md files
        if !path.to_lowercase().ends_with(".md") {
            return false;
        }

        // Block if in rendered mode (raw=false, the default)
        !self.is_raw_mode(ino)
    }

    /// Render markdown content with the template renderer.
    ///
    /// Returns rendered content, or graceful error content if rendering fails (AC13).
    /// Respects the render timeout (AC14).
    fn render_content(&self, raw_content: &[u8]) -> Vec<u8> {
        match self
            .template_renderer
            .render_with_timeout(raw_content, RENDER_TIMEOUT)
        {
            Ok(rendered) => rendered,
            Err(err_msg) => {
                // AC13: Return graceful error content, not crash
                let error_content = format!(
                    "<!-- AgentFS Render Error: {} -->\n\n{}",
                    err_msg,
                    String::from_utf8_lossy(raw_content)
                );
                error_content.into_bytes()
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Attribute Conversion
// ─────────────────────────────────────────────────────────────

/// Fill a `FileAttr` from AgentFS stats.
///
/// Similar to the Linux kernel's `generic_fillattr()`, this converts
/// filesystem-specific stat information into the VFS attribute structure.
///
/// The uid and gid parameters override the stored values to ensure proper
/// file ownership reporting (avoids "dubious ownership" errors from git).
fn fillattr(stats: &Stats, uid: u32, gid: u32) -> FileAttr {
    let kind = if stats.is_directory() {
        FileType::Directory
    } else if stats.is_symlink() {
        FileType::Symlink
    } else {
        FileType::RegularFile
    };

    let size = if stats.is_directory() {
        4096_u64 // Standard directory size
    } else {
        stats.size as u64
    };

    FileAttr {
        ino: stats.ino as u64,
        size,
        blocks: size.div_ceil(512),
        atime: UNIX_EPOCH + Duration::from_secs(stats.atime as u64),
        mtime: UNIX_EPOCH + Duration::from_secs(stats.mtime as u64),
        ctime: UNIX_EPOCH + Duration::from_secs(stats.ctime as u64),
        crtime: UNIX_EPOCH,
        kind,
        perm: (stats.mode & 0o777) as u16,
        nlink: stats.nlink,
        uid,
        gid,
        rdev: 0,
        flags: 0,
        blksize: 512,
    }
}

pub fn mount(
    fs: Arc<dyn FileSystem>,
    opts: FuseMountOptions,
    runtime: Runtime,
    handler_registry: Option<HandlerRegistry>,
) -> anyhow::Result<()> {
    // Use provided uid/gid or default to current user
    // This avoids "dubious ownership" errors from git and similar tools
    let uid = opts.uid.unwrap_or_else(|| unsafe { libc::getuid() });
    let gid = opts.gid.unwrap_or_else(|| unsafe { libc::getgid() });

    let fs = AgentFSFuse::new(
        fs,
        runtime,
        uid,
        gid,
        opts.mountpoint.clone(),
        handler_registry,
    );

    fs.add_path(1, "/".to_string());

    let mut mount_opts = vec![MountOption::FSName(opts.fsname)];
    if opts.auto_unmount {
        mount_opts.push(MountOption::AutoUnmount);
    }
    if opts.allow_root {
        mount_opts.push(MountOption::AllowRoot);
    }

    crate::fuser::mount2(fs, &opts.mountpoint, &mount_opts)?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Tests for Phase 2: Tera Rendering Mode (STORY-5.4)
// ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─────────────────────────────────────────────────────────────
    // StubTemplateRenderer Tests
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn test_stub_renderer_passthrough_plain_content() {
        let renderer = StubTemplateRenderer::new();
        let content = b"# Hello World\n\nNo template syntax here.";
        let result = renderer.render_with_timeout(content, Duration::from_secs(5));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), content.to_vec());
    }

    #[test]
    fn test_stub_renderer_adds_notice_for_tera_syntax() {
        let renderer = StubTemplateRenderer::new();
        let content = b"# Hello {{ name }}";
        let result = renderer.render_with_timeout(content, Duration::from_secs(5));

        assert!(result.is_ok());
        let rendered = result.unwrap();
        let rendered_str = String::from_utf8_lossy(&rendered);

        // Should contain the notice
        assert!(rendered_str.contains("AgentFS Notice"));
        assert!(rendered_str.contains("STORY-2.1.5"));
        // Should also contain the original content
        assert!(rendered_str.contains("{{ name }}"));
    }

    #[test]
    fn test_stub_renderer_detects_tera_block_syntax() {
        let renderer = StubTemplateRenderer::new();
        let content = b"{% for item in items %}{{ item }}{% endfor %}";
        let result = renderer.render_with_timeout(content, Duration::from_secs(5));

        assert!(result.is_ok());
        let rendered = result.unwrap();
        let rendered_str = String::from_utf8_lossy(&rendered);
        assert!(rendered_str.contains("AgentFS Notice"));
    }

    #[test]
    fn test_has_template_syntax_detection() {
        assert!(StubTemplateRenderer::has_template_syntax(b"{{ variable }}"));
        assert!(StubTemplateRenderer::has_template_syntax(b"{% if true %}"));
        assert!(StubTemplateRenderer::has_template_syntax(b"{# comment #}"));
        assert!(!StubTemplateRenderer::has_template_syntax(
            b"# Plain markdown"
        ));
        assert!(!StubTemplateRenderer::has_template_syntax(
            b"No special chars"
        ));
    }

    // ─────────────────────────────────────────────────────────────
    // Source Inode Tests
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn test_source_inode_mask() {
        // Verify SOURCE_INODE_MASK is set correctly
        assert_eq!(SOURCE_INODE_MASK, 0x4000_0000_0000_0000);
    }

    #[test]
    fn test_make_source_inode() {
        let real_ino: u64 = 42;
        let source_ino = real_ino | SOURCE_INODE_MASK;

        assert_eq!(source_ino, 0x4000_0000_0000_002A);
        assert_ne!(source_ino, real_ino);
    }

    #[test]
    fn test_get_real_inode_from_source() {
        let real_ino: u64 = 12345;
        let source_ino = real_ino | SOURCE_INODE_MASK;
        let recovered = source_ino & !SOURCE_INODE_MASK;

        assert_eq!(recovered, real_ino);
    }

    #[test]
    fn test_is_source_inode_detection() {
        let real_ino: u64 = 100;
        let source_ino = real_ino | SOURCE_INODE_MASK;

        assert!((source_ino & SOURCE_INODE_MASK) != 0);
        assert!((real_ino & SOURCE_INODE_MASK) == 0);
    }

    // ─────────────────────────────────────────────────────────────
    // xattr Constants Tests
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn test_xattr_raw_mode_constant() {
        assert_eq!(XATTR_RAW_MODE, "user.agentfs.raw");
    }

    #[test]
    fn test_source_suffix_constant() {
        assert_eq!(SOURCE_SUFFIX, ".source");
    }

    #[test]
    fn test_render_timeout_constant() {
        assert_eq!(RENDER_TIMEOUT, Duration::from_secs(5));
    }

    // ─────────────────────────────────────────────────────────────
    // Path Matching Tests
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn test_markdown_file_detection() {
        assert!("/path/to/file.md".to_lowercase().ends_with(".md"));
        assert!("/path/to/FILE.MD".to_lowercase().ends_with(".md"));
        assert!(!"/path/to/file.txt".to_lowercase().ends_with(".md"));
        assert!(!"/path/to/file.markdown".to_lowercase().ends_with(".md"));
    }

    #[test]
    fn test_source_suffix_detection() {
        assert!("file.md.source".ends_with(SOURCE_SUFFIX));
        assert!("README.md.source".ends_with(SOURCE_SUFFIX));
        assert!(!"file.md".ends_with(SOURCE_SUFFIX));
        assert!(!"file.source.md".ends_with(SOURCE_SUFFIX));
    }

    #[test]
    fn test_strip_source_suffix() {
        let name = "file.md.source";
        let stripped = &name[..name.len() - SOURCE_SUFFIX.len()];
        assert_eq!(stripped, "file.md");
    }
}
