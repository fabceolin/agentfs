use agentfs_sdk::filesystem::duckagentfs::{DuckAgentFSConfig, DuckConnectionPool};
use agentfs_sdk::filesystem::DuckAgentFS;
use agentfs_sdk::{get_mounts, FileSystem, Mount};
use anyhow::{Context, Result};
use std::{
    io::{self, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(target_os = "linux")]
use crate::fuse::FuseMountOptions;
#[cfg(target_os = "linux")]
use crate::handler::{DefaultHandler, GraphDocsDirInjector, GraphDocsHandler, HandlerRegistry};

/// Arguments for the mount command.
#[derive(Debug, Clone)]
pub struct MountArgs {
    /// The agent filesystem ID or path.
    pub id_or_path: String,
    /// The mountpoint path.
    pub mountpoint: PathBuf,
    /// Automatically unmount when the process exits.
    pub auto_unmount: bool,
    /// Allow root to access the mount.
    pub allow_root: bool,
    /// Run in foreground (don't daemonize).
    pub foreground: bool,
    /// User ID to report for all files (defaults to current user).
    pub uid: Option<u32>,
    /// Group ID to report for all files (defaults to current group).
    pub gid: Option<u32>,
}

/// Resolve database path from ID or path string.
///
/// Supports:
/// - `:memory:` for in-memory database
/// - Existing file path (used directly)
/// - Agent ID (looks for `.agentfs/{id}.duckdb`)
#[cfg(target_os = "linux")]
fn resolve_db_path(id_or_path: &str) -> Result<String> {
    if id_or_path == ":memory:" {
        return Ok(":memory:".to_string());
    }

    let path = std::path::Path::new(id_or_path);
    if path.exists() {
        return Ok(id_or_path.to_string());
    }

    // Try as agent ID
    let agentfs_dir = agentfs_sdk::agentfs_dir();
    let db_path = agentfs_dir.join(format!("{}.duckdb", id_or_path));
    if db_path.exists() {
        return Ok(db_path.to_string_lossy().to_string());
    }

    anyhow::bail!(
        "DuckDB database not found: {} (tried {} and {:?})",
        id_or_path,
        id_or_path,
        db_path
    )
}

/// Check if gd_documents table exists (GraphDocs tables present).
#[cfg(target_os = "linux")]
fn has_graphdocs_tables(pool: &DuckConnectionPool) -> bool {
    let conn = match pool.get_connection() {
        Ok(c) => c,
        Err(_) => return false,
    };

    // Check if gd_documents table exists
    let result: std::result::Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'gd_documents'",
        [],
        |row| row.get(0),
    );

    matches!(result, Ok(count) if count > 0)
}

/// Create handler registry with GraphDocs support if available.
#[cfg(target_os = "linux")]
fn create_handler_registry(
    fs: Arc<dyn FileSystem>,
    pool: &DuckConnectionPool,
) -> HandlerRegistry {
    let default_handler = Arc::new(DefaultHandler::new(fs));

    // Check if GraphDocs tables exist
    if has_graphdocs_tables(pool) {
        tracing::info!("GraphDocs tables detected, registering handlers");

        // Create registry with default handler
        let mut registry = HandlerRegistry::new(default_handler.clone());

        // Register GraphDocsHandler for /.graphdocs/ virtual directory
        let graphdocs_handler = Arc::new(GraphDocsHandler::new(pool.clone()));
        registry.register(graphdocs_handler);

        // Register GraphDocsDirInjector to add .graphdocs to root listings
        let injector = Arc::new(GraphDocsDirInjector::new(default_handler));
        registry.register(injector);

        registry
    } else {
        tracing::debug!("No GraphDocs tables found, using default handler only");
        HandlerRegistry::new(default_handler)
    }
}

/// Mount the agent filesystem using FUSE.
#[cfg(target_os = "linux")]
pub fn mount(args: MountArgs) -> Result<()> {
    // Resolve database path (DuckDB only)
    let db_path = resolve_db_path(&args.id_or_path)?;

    let fsname = format!(
        "agentfs:{}",
        std::fs::canonicalize(&args.id_or_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| args.id_or_path.clone())
    );

    if !args.mountpoint.exists() {
        anyhow::bail!("Mountpoint does not exist: {}", args.mountpoint.display());
    }

    let mountpoint = std::fs::canonicalize(args.mountpoint.clone())?;

    let fuse_opts = FuseMountOptions {
        mountpoint: args.mountpoint,
        auto_unmount: args.auto_unmount,
        allow_root: args.allow_root,
        fsname,
        uid: args.uid,
        gid: args.gid,
    };

    let mount = move || {
        let rt = crate::get_runtime();

        // Open DuckDB database
        let config = DuckAgentFSConfig {
            path: db_path.clone(),
            ..Default::default()
        };

        let duckfs = rt
            .block_on(DuckAgentFS::open(config))
            .context("Failed to open DuckDB database")?;

        // Get the connection pool for handler registration
        let pool = duckfs.pool();

        // Create filesystem reference
        let fs: Arc<dyn FileSystem> = Arc::new(duckfs);

        // Create handler registry with GraphDocs support if tables exist
        let handler_registry = create_handler_registry(fs.clone(), &pool);

        crate::fuse::mount(fs, fuse_opts, rt, Some(handler_registry))
    };

    if args.foreground {
        mount()
    } else {
        crate::daemon::daemonize(
            mount,
            move || is_mounted(&mountpoint),
            std::time::Duration::from_secs(10),
        )
    }
}

/// Mount the agent filesystem using FUSE (macOS - not supported).
#[cfg(target_os = "macos")]
pub fn mount(_args: MountArgs) -> Result<()> {
    anyhow::bail!(
        "FUSE mounting is not supported on macOS in this version.\n\
         Use `agentfs nfs` to mount via NFS instead."
    );
}

/// Check if a path is a mountpoint by comparing device IDs
#[cfg(target_os = "linux")]
fn is_mounted(path: &std::path::Path) -> bool {
    let path_meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => std::path::Path::new("/"),
    };

    let parent_meta = match std::fs::metadata(parent) {
        Ok(m) => m,
        Err(_) => return false,
    };

    // Different device IDs means it's a mountpoint
    path_meta.dev() != parent_meta.dev()
}

/// List all currently mounted agentfs filesystems
pub fn list_mounts<W: Write>(out: &mut W) {
    let mounts = get_mounts();

    if mounts.is_empty() {
        let _ = writeln!(out, "No agentfs filesystems mounted.");
        return;
    }

    // Calculate column widths
    let id_width = mounts.iter().map(|m| m.id.len()).max().unwrap_or(2).max(2);
    let mount_width = mounts
        .iter()
        .map(|m| m.mountpoint.to_string_lossy().len())
        .max()
        .unwrap_or(10)
        .max(10);

    // Print header
    let _ = writeln!(
        out,
        "{:<id_width$}  {:<mount_width$}",
        "ID",
        "MOUNTPOINT",
        id_width = id_width,
        mount_width = mount_width
    );

    // Print mounts
    for mount in &mounts {
        let _ = writeln!(
            out,
            "{:<id_width$}  {:<mount_width$}",
            mount.id,
            mount.mountpoint.display(),
            id_width = id_width,
            mount_width = mount_width
        );
    }
}

/// Check if a mount point is in use by any process.
///
/// Scans /proc to find processes with open files or current working directory
/// on the given mountpoint.
fn is_mount_in_use(mountpoint: &Path) -> bool {
    let mountpoint = match mountpoint.canonicalize() {
        Ok(p) => p,
        Err(_) => return false, // Can't check, assume not in use
    };

    let proc_dir = match std::fs::read_dir("/proc") {
        Ok(dir) => dir,
        Err(_) => return false,
    };

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Only check numeric directories (PIDs)
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let pid_path = entry.path();

        // Check cwd
        if let Ok(cwd) = std::fs::read_link(pid_path.join("cwd")) {
            if cwd.starts_with(&mountpoint) {
                return true;
            }
        }

        // Check open file descriptors
        let fd_dir = pid_path.join("fd");
        if let Ok(fds) = std::fs::read_dir(&fd_dir) {
            for fd_entry in fds.flatten() {
                if let Ok(target) = std::fs::read_link(fd_entry.path()) {
                    if target.starts_with(&mountpoint) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Unmount a FUSE filesystem.
///
/// Tries fusermount3 first, then falls back to fusermount.
fn unmount_fuse(mountpoint: &Path) -> Result<()> {
    const FUSERMOUNT_COMMANDS: &[&str] = &["fusermount3", "fusermount"];

    for cmd in FUSERMOUNT_COMMANDS {
        let result = std::process::Command::new(cmd)
            .args(["-u"])
            .arg(mountpoint.as_os_str())
            .status();

        match result {
            Ok(status) if status.success() => return Ok(()),
            Ok(_) => continue,  // Command ran but failed, try next
            Err(_) => continue, // Command not found, try next
        }
    }

    anyhow::bail!(
        "Failed to unmount {}. You may need to unmount manually with: fusermount -u {}",
        mountpoint.display(),
        mountpoint.display()
    )
}

/// Ask for user confirmation.
fn confirm(prompt: &str) -> bool {
    eprint!("{} ", prompt);
    let _ = io::stderr().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }

    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Prune unused agentfs mount points.
///
/// Finds all mounted agentfs filesystems that are not in use by any process
/// and unmounts them.
pub fn prune_mounts(force: bool) -> Result<()> {
    let mounts = get_mounts();

    // Get active session IDs to exclude from pruning
    let active_sessions = super::ps::active_session_ids();

    // Find unused mounts (not in use by any process and no active session)
    let unused_mounts: Vec<&Mount> = mounts
        .iter()
        .filter(|m| !is_mount_in_use(&m.mountpoint) && !active_sessions.contains(&m.id))
        .collect();

    if unused_mounts.is_empty() {
        println!("Nothing to prune.");
        return Ok(());
    }

    // Display what will be unmounted
    println!("The following unused mount points will be unmounted:");
    println!();
    for mount in &unused_mounts {
        println!("  {} -> {}", mount.id, mount.mountpoint.display());
    }
    println!();

    // Ask for confirmation unless --force
    if !force && !confirm("Are you sure? (y/N)") {
        println!("Aborted.");
        return Ok(());
    }

    // Unmount each unused mount
    let mut errors = Vec::new();
    for mount in &unused_mounts {
        print!("Unmounting {}... ", mount.mountpoint.display());
        let _ = io::stdout().flush();

        match unmount_fuse(&mount.mountpoint) {
            Ok(()) => println!("done"),
            Err(e) => {
                println!("failed");
                errors.push(format!("{}: {}", mount.mountpoint.display(), e));
            }
        }
    }

    if !errors.is_empty() {
        eprintln!();
        eprintln!("Some mounts could not be unmounted:");
        for error in &errors {
            eprintln!("  {}", error);
        }
        anyhow::bail!("Failed to unmount {} mount(s)", errors.len());
    }

    Ok(())
}
