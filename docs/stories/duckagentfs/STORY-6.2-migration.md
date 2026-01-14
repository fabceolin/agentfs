# STORY-6.2: Migration SQLite -> DuckDB

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-6.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 6 - Operations |
| **Status** | Won't Do |
| **Priority** | Low |
| **File** | `cli/src/cmd/migrate.rs` (new) |
| **Dependencies** | STORY-1.1, STORY-1.2 |

## User Story

**As an** operator
**I want** to migrate existing databases
**So that** I can adopt DuckAgentFS

## Acceptance Criteria

- [ ] Migration script from SQLite to DuckDB
- [ ] Validation after migration
- [ ] Rollback capability on error
- [ ] Progress reporting

## Technical Specification

### Migration Strategy

1. **Export from SQLite**: Read all data from AgentFS SQLite tables
2. **Transform**: Convert to append-only journal format
3. **Import to DuckDB**: Insert into DuckAgentFS tables
4. **Validate**: Verify data integrity
5. **Optional**: Generate initial embeddings if VSS enabled

### Schema Mapping

| SQLite (AgentFS) | DuckDB (DuckAgentFS) | Notes |
|------------------|----------------------|-------|
| `fs_inode` | `fs_journal` (create events) | One event per inode |
| `fs_dentry` | `fs_journal` (parent, name) | Merged with inode |
| `fs_data` | `fs_data` | Direct copy |
| `kv_store` | `kv_store` | Direct copy |
| `tool_calls` | `tool_calls` | Direct copy |

### Migration Command

```rust
// cli/src/cmd/migrate.rs

#[derive(Args)]
pub struct MigrateArgs {
    /// Source SQLite database (AgentFS format)
    source: String,

    /// Destination DuckDB database
    destination: String,

    /// Enable VSS and generate embeddings
    #[clap(long)]
    vss: bool,

    /// Enable PGQ and analyze code
    #[clap(long)]
    pgq: bool,

    /// Dry run (validate without writing)
    #[clap(long)]
    dry_run: bool,

    /// Continue on non-fatal errors
    #[clap(long)]
    force: bool,
}

pub async fn handle_migrate_command(args: MigrateArgs) -> Result<()> {
    println!("Migrating from {} to {}", args.source, args.destination);

    // Validate source
    if !Path::new(&args.source).exists() {
        return Err(anyhow::anyhow!("Source database not found"));
    }

    // Check destination doesn't exist (or use --force)
    if Path::new(&args.destination).exists() && !args.force {
        return Err(anyhow::anyhow!(
            "Destination already exists. Use --force to overwrite."
        ));
    }

    // Open source SQLite
    let source = turso::Connection::open(&args.source).await?;

    // Create destination DuckDB
    let dest_config = DuckAgentFSConfig {
        path: args.destination.clone(),
        enable_vss: args.vss,
        enable_pgq: args.pgq,
        ..Default::default()
    };
    let dest = DuckAgentFS::open(dest_config).await?;
    let dest_conn = dest.pool.get_write_connection().await?;

    // Migration phases
    let phases = [
        ("Inodes", migrate_inodes),
        ("File data", migrate_data),
        ("KV store", migrate_kv),
        ("Tool calls", migrate_tools),
    ];

    for (name, migrate_fn) in phases {
        print!("Migrating {}...", name);
        std::io::stdout().flush()?;

        if args.dry_run {
            let count = count_items(&source, name).await?;
            println!(" {} items (dry run)", count);
        } else {
            let count = migrate_fn(&source, &dest_conn).await?;
            println!(" {} items migrated", count);
        }
    }

    // Validation
    println!("\nValidating migration...");
    let validation = validate_migration(&source, &dest_conn).await?;

    if !validation.is_valid {
        println!("Validation FAILED:");
        for error in validation.errors {
            println!("  - {}", error);
        }
        if !args.force {
            // Rollback
            std::fs::remove_file(&args.destination)?;
            return Err(anyhow::anyhow!("Migration failed validation"));
        }
    } else {
        println!("Validation passed!");
    }

    // Optional: Generate embeddings
    if args.vss && !args.dry_run {
        println!("\nGenerating embeddings (this may take a while)...");
        generate_embeddings(&dest).await?;
    }

    // Optional: Analyze code
    if args.pgq && !args.dry_run {
        println!("\nAnalyzing code for dependency graph...");
        analyze_code(&dest).await?;
    }

    println!("\nMigration complete!");
    Ok(())
}
```

### Migration Functions

```rust
async fn migrate_inodes(source: &Connection, dest: &DuckConnection) -> Result<usize> {
    let mut count = 0;

    // Read all inodes with their dentry info
    let mut stmt = source.prepare(r#"
        SELECT
            i.ino, i.mode, i.uid, i.gid, i.size, i.nlink,
            i.atime, i.mtime, i.ctime,
            d.parent, d.name
        FROM fs_inode i
        LEFT JOIN fs_dentry d ON d.ino = i.ino
        ORDER BY i.ino
    "#).await?;

    let mut rows = stmt.query([]).await?;

    while let Some(row) = rows.next().await? {
        let ino: i64 = row.get(0)?;
        let mode: u32 = row.get(1)?;
        let uid: u32 = row.get(2)?;
        let gid: u32 = row.get(3)?;
        let size: i64 = row.get(4)?;
        let nlink: u32 = row.get(5)?;
        let mtime: i64 = row.get(7)?;
        let parent: Option<i64> = row.get(9)?;
        let name: Option<String> = row.get(10)?;

        // Insert as create event in journal
        dest.execute(r#"
            INSERT INTO fs_journal
            (event_id, inode, event_type, parent, name, mode, uid, gid, size, nlink, event_time)
            VALUES (?, ?, 'create', ?, ?, ?, ?, ?, ?, ?,
                    to_timestamp(?))
        "#, params![
            count + 1, // event_id
            ino,
            parent.unwrap_or(1),
            name.unwrap_or_default(),
            mode,
            uid,
            gid,
            size,
            nlink,
            mtime
        ])?;

        count += 1;
    }

    // Update sequence to continue after migrated events
    dest.execute(
        &format!("ALTER SEQUENCE fs_event_seq RESTART WITH {}", count + 1),
        []
    )?;

    Ok(count)
}

async fn migrate_data(source: &Connection, dest: &DuckConnection) -> Result<usize> {
    let mut count = 0;

    let mut stmt = source.prepare(
        "SELECT ino, chunk_index, data FROM fs_data ORDER BY ino, chunk_index"
    ).await?;

    let mut rows = stmt.query([]).await?;

    while let Some(row) = rows.next().await? {
        let ino: i64 = row.get(0)?;
        let chunk_idx: u32 = row.get(1)?;
        let data: Vec<u8> = row.get(2)?;

        dest.execute(r#"
            INSERT INTO fs_data (inode, chunk_idx, data)
            VALUES (?, ?, ?)
        "#, params![ino, chunk_idx, data])?;

        count += 1;
    }

    Ok(count)
}

async fn migrate_kv(source: &Connection, dest: &DuckConnection) -> Result<usize> {
    let mut count = 0;

    let mut stmt = source.prepare(
        "SELECT key, value, created_at, updated_at FROM kv_store"
    ).await?;

    let mut rows = stmt.query([]).await?;

    while let Some(row) = rows.next().await? {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        let created_at: i64 = row.get(2)?;
        let updated_at: i64 = row.get(3)?;

        dest.execute(r#"
            INSERT INTO kv_store (key, value, created_at, updated_at)
            VALUES (?, ?::JSON, to_timestamp(?), to_timestamp(?))
        "#, params![key, value, created_at, updated_at])?;

        count += 1;
    }

    Ok(count)
}

async fn migrate_tools(source: &Connection, dest: &DuckConnection) -> Result<usize> {
    let mut count = 0;

    let mut stmt = source.prepare(r#"
        SELECT id, name, status, started_at, completed_at,
               duration_ms, parameters, result, error
        FROM tool_calls
    "#).await?;

    let mut rows = stmt.query([]).await?;

    while let Some(row) = rows.next().await? {
        dest.execute(r#"
            INSERT INTO tool_calls
            (id, name, status, started_at, completed_at, duration_ms,
             parameters, result, error)
            VALUES (?, ?, ?, to_timestamp(?), to_timestamp(?), ?, ?::JSON, ?::JSON, ?)
        "#, params![
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<f64>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?
        ])?;

        count += 1;
    }

    Ok(count)
}
```

### Validation

```rust
struct ValidationResult {
    is_valid: bool,
    errors: Vec<String>,
}

async fn validate_migration(
    source: &Connection,
    dest: &DuckConnection
) -> Result<ValidationResult> {
    let mut errors = Vec::new();

    // Check inode count
    let source_inodes: i64 = source.query_row(
        "SELECT COUNT(*) FROM fs_inode", [], |r| r.get(0)
    ).await?;

    let dest_inodes: i64 = dest.query_row(
        "SELECT COUNT(DISTINCT inode) FROM fs_journal", [], |r| r.get(0)
    )?;

    if source_inodes != dest_inodes {
        errors.push(format!(
            "Inode count mismatch: {} vs {}",
            source_inodes, dest_inodes
        ));
    }

    // Check data integrity (sample)
    let source_data_size: i64 = source.query_row(
        "SELECT SUM(LENGTH(data)) FROM fs_data", [], |r| r.get(0)
    ).await?;

    let dest_data_size: i64 = dest.query_row(
        "SELECT SUM(LENGTH(data)) FROM fs_data", [], |r| r.get(0)
    )?;

    if source_data_size != dest_data_size {
        errors.push(format!(
            "Data size mismatch: {} vs {}",
            source_data_size, dest_data_size
        ));
    }

    // Check root directory exists
    let root_exists: bool = dest.query_row(
        "SELECT EXISTS(SELECT 1 FROM fs_current WHERE inode = 1)",
        [], |r| r.get(0)
    )?;

    if !root_exists {
        errors.push("Root directory (inode 1) not found".to_string());
    }

    Ok(ValidationResult {
        is_valid: errors.is_empty(),
        errors,
    })
}
```

### CLI Usage

```bash
# Basic migration
agentfs migrate ./old.db ./new.duckdb

# Migration with VSS (generates embeddings)
agentfs migrate ./old.db ./new.duckdb --vss

# Migration with PGQ (analyzes code)
agentfs migrate ./old.db ./new.duckdb --pgq

# Full migration with all features
agentfs migrate ./old.db ./new.duckdb --vss --pgq

# Dry run (validate only)
agentfs migrate ./old.db ./new.duckdb --dry-run

# Force overwrite existing
agentfs migrate ./old.db ./new.duckdb --force
```

### Output Example

```
Migrating from ./old.db to ./new.duckdb
Migrating Inodes... 1523 items migrated
Migrating File data... 8942 items migrated
Migrating KV store... 156 items migrated
Migrating Tool calls... 2341 items migrated

Validating migration...
Validation passed!

Generating embeddings (this may take a while)...
  [============================] 100% (1523/1523 files)

Migration complete!
```

## Tests

### Test 1: Round-trip Migration
```rust
#[tokio::test]
async fn test_migration_roundtrip() {
    // Create source with test data
    let source_path = tempfile::NamedTempFile::new()?.path();
    let source = AgentFS::open(source_path).await?;
    source.write_file("/test.txt", b"hello").await?;

    // Migrate
    let dest_path = tempfile::NamedTempFile::new()?.path();
    migrate(&source_path, &dest_path, false, false).await?;

    // Verify
    let dest = DuckAgentFS::open(dest_path).await?;
    let content = dest.read_file("/test.txt").await?.unwrap();
    assert_eq!(content, b"hello");
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/cmd/migrate.rs` | Migration command |
| `sdk/rust/src/filesystem/agentfs.rs` | Source schema |
| `schema/duckagentfs.sql` | Destination schema |

## Rollback

If migration fails:

1. Delete the destination file (handled automatically on error)
2. Source database is unchanged
3. User can retry after fixing issues
