# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

AgentFS is a filesystem explicitly designed for AI agents. It provides storage abstractions backed by a single SQLite database file, enabling auditability, reproducibility, and portability for agent state. The project consists of:

- **CLI** (`cli/`): Command-line tool written in Rust for managing agent filesystems, mounting via FUSE (Linux) or NFS (macOS), running sandboxed programs, and serving MCP protocol.
- **SDKs**: Libraries for programmatic access in TypeScript (`sdk/typescript/`), Python (`sdk/python/`), and Rust (`sdk/rust/`).
- **Sandbox** (`sandbox/`): Linux-specific syscall interception for sandboxed execution using ptrace.

## Build Commands

### CLI (Rust)
```bash
cd cli
cargo build                    # Debug build
cargo build --release          # Release build
cargo test                     # Run tests
cargo clippy                   # Run linter
```

To build without sandbox support (e.g., on macOS):
```bash
cargo build --no-default-features
```

### TypeScript SDK
```bash
cd sdk/typescript
npm install
npm run build                  # Compile TypeScript
npm test                       # Run tests with vitest
npm run test:watch             # Watch mode
npm run test:browser           # Browser tests (Chromium + Firefox)
```

### Python SDK
```bash
cd sdk/python
uv sync --group dev            # Install dependencies
uv run pytest                  # Run tests
uv run ruff format agentfs_sdk tests   # Format code
uv run ruff check agentfs_sdk tests    # Lint
```

### Rust SDK
```bash
cd sdk/rust
cargo test
cargo bench                    # Run benchmarks (overlayfs, workload)
```

## Architecture

### Core Data Model (SQLite Schema)

All agent state is stored in a single `.agentfs/{id}.db` SQLite file with three main components:

1. **Virtual Filesystem** (`fs_*` tables): Unix-like inode design with:
   - `fs_inode`: File/directory metadata (mode, size, timestamps, nlink)
   - `fs_dentry`: Directory entries mapping names to inodes
   - `fs_data`: File content in fixed-size chunks (default 4KB)
   - `fs_symlink`: Symbolic link targets
   - `fs_config`: Filesystem configuration (chunk_size)

2. **Key-Value Store** (`kv_store` table): JSON-serialized values with timestamps.

3. **Tool Call Audit Log** (`tool_calls` table): Insert-only log tracking tool invocations with parameters, results, and timing.

### Overlay Filesystem

When initialized with `--base <PATH>`, AgentFS operates as a copy-on-write overlay:
- `fs_whiteout`: Tracks deleted paths from base layer
- `fs_origin`: Maps delta inodes to original base inodes (for copy-up operations)
- `fs_overlay_config`: Stores base path configuration

### CLI Structure (`cli/src/`)

- `main.rs`: Entry point with command dispatch
- `parser.rs`: Clap argument definitions
- `cmd/`: Command implementations (init, run, mount, fs, timeline, sync, mcp_server, nfs)
- `fuse.rs`: FUSE filesystem implementation (Linux)
- `nfs.rs`: NFS server implementation (cross-platform)
- `sandbox/`: Platform-specific sandbox implementations
  - `linux.rs`: User namespace + FUSE overlay
  - `linux_ptrace.rs`: Experimental ptrace-based syscall interception
  - `darwin.rs`: macOS sandbox-exec + NFS

### SDK Structure

Each SDK exposes three main interfaces:
- `AgentFS`: Main entry point with `kv`, `fs`, and `tools` properties
- `KvStore`: Key-value operations (get, set, delete, list)
- `FileSystem`/`AgentFS`: File operations (read_file, write_file, mkdir, readdir, stat, etc.)
- `ToolCalls`: Audit log (start, success, error, record, get_stats)

TypeScript SDK also includes:
- `integrations/just-bash/`: Safe bash execution integration
- `integrations/cloudflare/`: Cloudflare Workers/Durable Objects support

## Key Concepts

### Agent ID Resolution

When resolving an agent ID/path:
1. `:memory:` → ephemeral in-memory database
2. Valid agent ID with existing `.agentfs/{id}.db` → uses that agent
3. Existing file path → uses that path directly

Valid agent IDs: alphanumeric characters, hyphens, and underscores only.

### Connection Pool

The Rust SDK uses a connection pool (`connection_pool.rs`) that supports both local and sync databases. Use `get_connection().await` to acquire a pooled connection with RAII-style transaction management.

### Sync Support

AgentFS databases can sync with remote Turso databases:
- `AgentFSOptions.sync.remote_url`: Remote database URL
- `AgentFSOptions.sync.auth_token`: Authentication token
- `pull()`, `push()`, `checkpoint()`: Sync operations

## Testing

### FUSE Testing with xfstests
See `TESTING.md` for xfstests configuration. Requires installing the CLI and `mount.fuse.agentfs` helper.

### Syscall Tests
```bash
cd cli/tests/syscall
make                           # Build syscall test binary
```

## Environment Variables

- `AGENTFS=1`: Set inside AgentFS sandbox
- `AGENTFS_SANDBOX`: Sandbox type (`macos-sandbox` or `linux-namespace`)
- `AGENTFS_SESSION`: Current session ID
- `RUST_LOG=agentfs=debug`: Enable debug logging for CLI
