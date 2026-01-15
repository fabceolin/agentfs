# STORY-6.1: CLI Commands

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-6.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 6 - Operations |
| **Status** | Ready for Development |
| **Priority** | Medium |
| **File** | `cli/src/parser.rs`, `cli/src/cmd/` |
| **Dependencies** | All previous stories |

## User Story

**As an** operator
**I want** CLI commands for DuckAgentFS
**So that** I can manage databases

## Acceptance Criteria

- [ ] `agentfs init --backend duckdb`
- [ ] `agentfs search <query>`
- [ ] `agentfs snapshot list <agent>` - List available event IDs
- [ ] `agentfs snapshot <agent> cat --at <id> <path>` - Read file at snapshot
- [ ] `agentfs snapshot <agent> ls --at <id> <path>` - List directory at snapshot
- [ ] `agentfs snapshot <agent> diff --from <X> --to <Y>` - Show diff between snapshots
- [ ] `agentfs graph deps <file>`
- [ ] Help text and examples

> **Note**: Snapshot CLI commands consolidated from STORY-1.4 per SCP-2026-01-14
> **Note**: Search CLI command consolidated from STORY-2.3 per SCP-2026-01-15

## Technical Specification

### Parser Updates

```rust
// cli/src/parser.rs

#[derive(Parser)]
pub enum Command {
    /// Initialize a new agent filesystem
    Init {
        #[clap(long)]
        id: Option<String>,

        /// Backend type: sqlite (default) or duckdb
        #[clap(long, default_value = "sqlite")]
        backend: Backend,

        /// Enable VSS extension (DuckDB only)
        #[clap(long)]
        vss: bool,

        /// Enable PGQ extension (DuckDB only)
        #[clap(long)]
        pgq: bool,
    },

    /// Search files semantically (DuckDB with VSS only)
    Search {
        /// Agent ID or database path
        id_or_path: String,

        /// Search query
        query: String,

        /// Maximum results
        #[clap(long, short, default_value = "10")]
        limit: usize,

        /// Filter by directory
        #[clap(long, short)]
        dir: Option<String>,

        /// Filter by file extension (can be repeated)
        #[clap(long, short)]
        ext: Vec<String>,

        /// Search at chunk level
        #[clap(long)]
        chunks: bool,
    },

    /// Time-travel snapshot operations
    Snapshot {
        /// Agent ID or database path
        id_or_path: String,

        #[clap(subcommand)]
        command: SnapshotCommand,
    },

    /// Code graph operations
    Graph {
        /// Agent ID or database path
        id_or_path: String,

        #[clap(subcommand)]
        command: GraphCommand,
    },
}

#[derive(Subcommand)]
pub enum SnapshotCommand {
    /// List recent events
    List {
        #[clap(long, default_value = "20")]
        limit: usize,
    },

    /// Show file at specific event
    Cat {
        #[clap(long)]
        at: i64,
        path: String,
    },

    /// List directory at specific event
    Ls {
        #[clap(long)]
        at: i64,
        path: String,
    },

    /// Show diff between events
    Diff {
        #[clap(long)]
        from: i64,
        #[clap(long)]
        to: i64,
    },
}

#[derive(Subcommand)]
pub enum GraphCommand {
    /// Show callers of a symbol
    Callers {
        symbol_id: String,
    },

    /// Show callees of a symbol
    Callees {
        symbol_id: String,
    },

    /// Show file dependencies
    Deps {
        path: String,
    },

    /// Analyze change impact
    Impact {
        symbol_id: String,
        #[clap(long, default_value = "5")]
        depth: usize,
    },

    /// Find unreferenced code
    DeadCode,

    /// Re-analyze codebase
    Analyze {
        #[clap(long)]
        all: bool,
        #[clap(long)]
        path: Option<String>,
    },
}

#[derive(ValueEnum, Clone)]
pub enum Backend {
    Sqlite,
    Duckdb,
}
```

### Command Handlers

```rust
// cli/src/cmd/search.rs

pub async fn handle_search_command(
    id_or_path: String,
    query: String,
    limit: usize,
    dir: Option<String>,
    ext: Vec<String>,
    chunks: bool,
) -> Result<()> {
    let fs = open_duckagentfs(&id_or_path).await?;

    if !fs.config.enable_vss {
        eprintln!("Error: VSS not enabled. Use --vss flag when initializing.");
        std::process::exit(1);
    }

    let options = SearchOptions {
        limit,
        directory: dir,
        extensions: if ext.is_empty() { None } else { Some(ext) },
        search_chunks: chunks,
        ..Default::default()
    };

    let results = if chunks {
        fs.search_chunks(&query, options).await?
    } else {
        fs.search(&query, options).await?
    };

    if results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    println!("Found {} results:\n", results.len());

    for (i, result) in results.iter().enumerate() {
        println!("{}. {} (score: {:.3})", i + 1, result.path, result.score);

        if !result.preview.is_empty() {
            let preview: String = result.preview
                .chars()
                .take(80)
                .collect();
            println!("   {}", preview.replace('\n', " "));
        }

        if let Some(chunk) = &result.chunk {
            println!("   [chunk {} @ bytes {}..{}]",
                chunk.index, chunk.start_offset, chunk.end_offset);
        }
        println!();
    }

    Ok(())
}
```

```rust
// cli/src/cmd/snapshot.rs

pub async fn handle_snapshot_command(
    id_or_path: String,
    command: SnapshotCommand,
) -> Result<()> {
    let fs = open_duckagentfs(&id_or_path).await?;

    match command {
        SnapshotCommand::List { limit } => {
            let events = fs.list_events(limit, 0).await?;

            println!("{:<10} {:<10} {:<20} {:<30}",
                "EVENT_ID", "TYPE", "TIME", "NAME");
            println!("{}", "-".repeat(70));

            for event in events {
                println!("{:<10} {:<10} {:<20} {:<30}",
                    event.event_id,
                    event.event_type,
                    event.event_time,
                    event.name.unwrap_or_default()
                );
            }
        }

        SnapshotCommand::Cat { at, path } => {
            let snapshot = fs.snapshot_at(at).await?;
            let content = snapshot.read_file(&path).await?
                .ok_or_else(|| anyhow::anyhow!("File not found at event {}", at))?;

            std::io::stdout().write_all(&content)?;
        }

        SnapshotCommand::Ls { at, path } => {
            let snapshot = fs.snapshot_at(at).await?;
            let entries = snapshot.readdir(&path).await?
                .ok_or_else(|| anyhow::anyhow!("Directory not found at event {}", at))?;

            for entry in entries {
                println!("{}", entry);
            }
        }

        SnapshotCommand::Diff { from, to } => {
            let diffs = fs.diff(from, to).await?;

            for diff in diffs {
                let prefix = match &diff.change_type {
                    ChangeType::Added => "+",
                    ChangeType::Deleted => "-",
                    ChangeType::Modified => "~",
                    ChangeType::Renamed { from } => {
                        println!("R {} -> {}", from, diff.path);
                        continue;
                    }
                };
                println!("{} {}", prefix, diff.path);
            }
        }
    }

    Ok(())
}
```

### Help Examples

```bash
# Initialize with DuckDB backend and extensions
agentfs init my-agent --backend duckdb --vss --pgq

# Semantic search
agentfs search my-agent "error handling"
agentfs search my-agent "database connection" --limit 5 --dir /src --ext .rs

# Time-travel
agentfs snapshot my-agent list
agentfs snapshot my-agent cat --at 100 /src/main.rs
agentfs snapshot my-agent diff --from 50 --to 100

# Code graph
agentfs graph my-agent callers "src/lib.rs:process"
agentfs graph my-agent deps /src/main.rs
agentfs graph my-agent impact "src/db.rs:Connection" --depth 3
agentfs graph my-agent dead-code
agentfs graph my-agent analyze --all
```

## Output Formatting

### Search Results
```
Found 3 results:

1. /src/db/connection.rs (score: 0.892)
   impl Connection { pub fn new() -> Result<Self> { let pool = ...

2. /src/handlers/error.rs (score: 0.845)
   pub fn handle_error(e: Error) -> Response { match e { Error::D...

3. /docs/api.md (score: 0.721)
   ## Error Handling The API returns standard HTTP error codes ...
```

### Snapshot List
```
EVENT_ID   TYPE       TIME                 NAME
----------------------------------------------------------------------
150        update     2024-01-15 10:30:00  main.rs
149        create     2024-01-15 10:29:45  config.rs
148        delete     2024-01-15 10:25:00  old_file.rs
```

### Graph Impact
```
Impact analysis for: src/db.rs:Connection

Affected: 12 symbols in 5 files

Distance 1:
  - src/db.rs:query (function)
  - src/db.rs:execute (function)

Distance 2:
  - src/handlers/user.rs:get_user (function)
  - src/handlers/user.rs:create_user (function)
```

## Tests

### Test 1: Search Command
```rust
#[tokio::test]
async fn test_search_command() {
    // Setup test database with files
    let id = setup_test_db_with_files().await;

    // Run search
    let result = handle_search_command(
        id,
        "test query".to_string(),
        10,
        None,
        vec![],
        false,
    ).await;

    assert!(result.is_ok());
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/parser.rs` | Argument parsing |
| `cli/src/cmd/search.rs` | Search command |
| `cli/src/cmd/snapshot.rs` | Snapshot commands |
| `cli/src/cmd/graph.rs` | Graph commands |
