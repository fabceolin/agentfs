//! DuckDB CLI commands for managing DuckDB-backed agent filesystems
//!
//! This module provides CLI handlers for:
//! - `agentfs duckdb init <agent-id>` - Initialize a new DuckDB database
//!
//! Examples:
//!   agentfs duckdb init my-agent
//!   agentfs duckdb init my-agent --vss --pgq
//!   agentfs duckdb init my-agent --force

use agentfs_sdk::agentfs_dir;
use agentfs_sdk::filesystem::duckagentfs::{DuckAgentFS, DuckAgentFSConfig};
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

/// DuckDB subcommands
#[derive(Subcommand, Debug)]
pub enum DuckdbCommand {
    /// Initialize a new DuckDB database for an agent
    ///
    /// Creates a new DuckDB database file at .agentfs/<agent-id>.duckdb
    /// with the AgentFS schema initialized.
    ///
    /// Examples:
    ///   agentfs duckdb init my-agent
    ///   agentfs duckdb init my-agent --vss --pgq
    ///   agentfs duckdb init my-agent --force
    Init(InitArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Agent identifier (alphanumeric, hyphens, underscores)
    pub agent_id: String,

    /// Enable VSS (Vector Similarity Search) extension
    #[arg(long)]
    pub vss: bool,

    /// Enable PGQ (Property Graph Query) extension
    #[arg(long)]
    pub pgq: bool,

    /// Overwrite existing database if it exists
    #[arg(long)]
    pub force: bool,
}

/// Handle `agentfs duckdb init` command
pub async fn handle_init(args: InitArgs) -> Result<()> {
    // Validate agent ID
    if !is_valid_agent_id(&args.agent_id) {
        bail!(
            "Invalid agent ID '{}': must contain only alphanumeric characters, hyphens, and underscores",
            args.agent_id
        );
    }

    // Determine database path
    let agentfs_dir = agentfs_dir();
    let db_path = agentfs_dir.join(format!("{}.duckdb", args.agent_id));

    // Create .agentfs directory if needed
    if !agentfs_dir.exists() {
        std::fs::create_dir_all(agentfs_dir)
            .with_context(|| format!("Failed to create directory: {agentfs_dir:?}"))?;
    }

    // Check if database already exists
    if db_path.exists() && !args.force {
        bail!(
            "Database already exists: {:?}\nUse --force to overwrite",
            db_path
        );
    }

    // Remove existing database if --force is used
    if db_path.exists() && args.force {
        std::fs::remove_file(&db_path)
            .with_context(|| format!("Failed to remove existing database: {:?}", db_path))?;
    }

    // Create and initialize the database
    let config = DuckAgentFSConfig {
        path: db_path.to_string_lossy().to_string(),
        enable_vss: args.vss,
        enable_pgq: args.pgq,
        ..Default::default()
    };

    let _fs = DuckAgentFS::open(config)
        .await
        .context("Failed to create DuckAgentFS database")?;

    // Print success message
    println!("Created DuckDB database: {:?}", db_path);
    if args.vss {
        println!("  VSS extension: enabled");
    }
    if args.pgq {
        println!("  PGQ extension: enabled");
    }

    Ok(())
}

/// Validate agent ID: alphanumeric, hyphens, and underscores only
fn is_valid_agent_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// Open or create a DuckDB database for the given agent ID or path.
/// Returns the database and path, along with whether it was auto-created.
pub async fn open_or_create_duckdb(
    id_or_path: &str,
    enable_vss: bool,
    enable_pgq: bool,
) -> Result<(DuckAgentFS, PathBuf, bool)> {
    let (path, auto_created) = resolve_db_path(id_or_path)?;

    let config = DuckAgentFSConfig {
        path: path.to_string_lossy().to_string(),
        enable_vss,
        enable_pgq,
        ..Default::default()
    };

    let fs = DuckAgentFS::open(config)
        .await
        .context("Failed to open DuckAgentFS database")?;

    Ok((fs, path, auto_created))
}

/// Resolve an agent ID or path to a database path.
/// Creates the .agentfs directory and returns whether the path is new.
fn resolve_db_path(id_or_path: &str) -> Result<(PathBuf, bool)> {
    // Handle :memory: special case
    if id_or_path == ":memory:" {
        return Ok((PathBuf::from(":memory:"), false));
    }

    // Check if it's an existing file path
    let path = std::path::Path::new(id_or_path);
    if path.exists() {
        return Ok((path.to_path_buf(), false));
    }

    // Treat as agent ID
    if !is_valid_agent_id(id_or_path) {
        bail!(
            "Invalid agent ID '{}': must contain only alphanumeric characters, hyphens, and underscores",
            id_or_path
        );
    }

    let agentfs_dir = agentfs_dir();
    let db_path = agentfs_dir.join(format!("{}.duckdb", id_or_path));

    // Check if database exists
    if db_path.exists() {
        return Ok((db_path, false));
    }

    // Database doesn't exist - will be auto-created
    // Ensure .agentfs directory exists
    if !agentfs_dir.exists() {
        std::fs::create_dir_all(agentfs_dir)
            .with_context(|| format!("Failed to create directory: {agentfs_dir:?}"))?;
    }

    Ok((db_path, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_valid_agent_id() {
        assert!(is_valid_agent_id("my-agent"));
        assert!(is_valid_agent_id("my_agent"));
        assert!(is_valid_agent_id("agent123"));
        assert!(is_valid_agent_id("Agent-123_test"));
    }

    #[test]
    fn test_invalid_agent_id() {
        assert!(!is_valid_agent_id(""));
        assert!(!is_valid_agent_id("my agent"));
        assert!(!is_valid_agent_id("my/agent"));
        assert!(!is_valid_agent_id("my.agent"));
        assert!(!is_valid_agent_id("../escape"));
    }

    #[tokio::test]
    async fn test_duckdb_init_creates_database() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.duckdb");

        // Create database using DuckAgentFS::open directly (simulates init)
        let config = DuckAgentFSConfig {
            path: db_path.to_string_lossy().to_string(),
            ..Default::default()
        };
        let _fs = DuckAgentFS::open(config).await.unwrap();

        // Verify file was created
        assert!(db_path.exists());
    }

    #[tokio::test]
    async fn test_duckdb_init_with_vss_pgq() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test-extensions.duckdb");

        let config = DuckAgentFSConfig {
            path: db_path.to_string_lossy().to_string(),
            enable_vss: true,
            enable_pgq: true,
            ..Default::default()
        };
        let _fs = DuckAgentFS::open(config).await.unwrap();

        assert!(db_path.exists());
    }

    #[tokio::test]
    async fn test_duckdb_init_force_overwrites() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test-force.duckdb");

        // Create initial database
        let config = DuckAgentFSConfig {
            path: db_path.to_string_lossy().to_string(),
            ..Default::default()
        };
        let _fs = DuckAgentFS::open(config).await.unwrap();
        drop(_fs);

        // Get initial modification time
        let initial_mtime = std::fs::metadata(&db_path).unwrap().modified().unwrap();

        // Wait a bit to ensure time difference
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Remove and recreate (simulates --force)
        std::fs::remove_file(&db_path).unwrap();
        let config = DuckAgentFSConfig {
            path: db_path.to_string_lossy().to_string(),
            ..Default::default()
        };
        let _fs = DuckAgentFS::open(config).await.unwrap();

        // Verify file was recreated
        let new_mtime = std::fs::metadata(&db_path).unwrap().modified().unwrap();
        assert!(new_mtime > initial_mtime);
    }

    #[test]
    fn test_resolve_db_path_memory() {
        let result = resolve_db_path(":memory:").unwrap();
        assert_eq!(result.0, PathBuf::from(":memory:"));
        assert!(!result.1); // Not auto-created
    }

    #[test]
    fn test_resolve_db_path_invalid_id() {
        let result = resolve_db_path("invalid agent");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid agent ID"));
    }
}
