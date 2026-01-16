# Technology Stack

This document describes the technologies, frameworks, and tools used in the AgentFS project.

## Core Technologies

### Storage Backends

| Technology | Version | Purpose |
|------------|---------|---------|
| **SQLite** (via Turso) | 0.4.3-pre.2 | Primary storage backend for AgentFS |
| **DuckDB** | 1.1 | Analytical backend for DuckAgentFS |

**SQLite (Turso)**: The default storage backend. Provides OLTP-optimized storage with embedded sync support for remote Turso databases.

**DuckDB**: New analytical backend (DuckAgentFS) providing:
- Append-only journal model for time-travel
- VSS extension for vector similarity search
- DuckPGQ extension for code dependency graphs
- Columnar storage for analytics performance

### Languages

| Language | Version | Components |
|----------|---------|------------|
| **Rust** | 2021 Edition | CLI, Rust SDK, Sandbox |
| **TypeScript** | 5.3+ | TypeScript SDK |
| **Python** | 3.9+ | Python SDK |

## CLI Stack (`cli/`)

### Build System
- **Cargo**: Rust package manager and build system
- **cargo-dist**: Release builds and distribution

### Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio` | 1.x | Async runtime |
| `clap` | 4.x | CLI argument parsing |
| `anyhow` | 1.0 | Error handling |
| `turso` | 0.4.3-pre.2 | SQLite with sync |
| `serde` / `serde_json` | 1.0 | Serialization |
| `tracing` | 0.1 | Logging and diagnostics |
| `nfsserve` | 0.10 | NFS server implementation |
| `uuid` | 1.x | UUID generation |

### Platform-Specific

**Linux**:
- `libc`: System calls
- `nix`: Unix utilities
- Vendored FUSE implementation (pure Rust)
- `reverie`: ptrace-based syscall interception (optional sandbox)

**macOS**:
- `aegis`: Pure Rust crypto (arm64 compatibility)
- NFS-based filesystem mounting (no FUSE)

## Rust SDK Stack (`sdk/rust/`)

### Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `turso` | 0.4.3-pre.2 | SQLite database access |
| `duckdb` | 1.1 | DuckDB database access |
| `tokio` | 1.x | Async runtime |
| `async-trait` | 0.1 | Async trait support |
| `thiserror` | 1.0 | Error type definitions |
| `lru` | 0.12 | LRU cache for DentryCache |
| `tracing` | 0.1 | Structured logging |

### Dev Dependencies
- `tempfile`: Temporary files for testing
- `proptest`: Property-based testing
- `criterion`: Benchmarking framework

## TypeScript SDK Stack (`sdk/typescript/`)

### Runtime Support
- **Node.js**: Primary runtime via `@tursodatabase/database`
- **Browser**: WebAssembly via `@tursodatabase/database-wasm`

### Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `@tursodatabase/database` | 0.4.0-pre.18 | SQLite for Node.js |
| `@tursodatabase/database-wasm` | 0.4.0-pre.18 | SQLite for browsers |
| `buffer` | 6.0.3 | Buffer polyfill |

### Dev Dependencies
- `typescript`: TypeScript compiler
- `vitest`: Test framework
- `@vitest/browser`: Browser testing
- `playwright`: Browser automation for tests
- `just-bash`: Bash execution integration

## Python SDK Stack (`sdk/python/`)

### Package Manager
- **uv**: Fast Python package installer

### Dependencies
- Python standard library only (minimal dependencies)

### Dev Dependencies
- `pytest`: Test framework
- `ruff`: Linting and formatting

## Sandbox Stack (`sandbox/`)

Linux-specific syscall interception for sandboxed execution.

### Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `libc` | - | System call interface |
| `reverie` | git | Facebook's ptrace framework |
| `reverie-ptrace` | git | ptrace implementation |
| `reverie-process` | git | Process management |

## Build Prerequisites (Ubuntu/Debian)

### Required System Packages

```bash
sudo apt-get install -y \
    pkg-config \
    libssl-dev \
    liblzma-dev
```

### OpenSSL Header Symlinks

On Ubuntu, OpenSSL headers are installed in a platform-specific directory. The Rust toolchain's linker may not find them without symlinks:

```bash
# Create symlinks for OpenSSL headers
sudo ln -sf /usr/include/x86_64-linux-gnu/openssl/opensslconf.h /usr/include/openssl/opensslconf.h
sudo ln -sf /usr/include/x86_64-linux-gnu/openssl/configuration.h /usr/include/openssl/configuration.h
```

### Linker Configuration

The Rust nightly toolchain uses `rust-lld` which may not find system libraries. Set `RUSTFLAGS` to add the library search path:

```bash
# Option 1: Environment variable (per-command)
RUSTFLAGS="-L /usr/lib/x86_64-linux-gnu" cargo build
RUSTFLAGS="-L /usr/lib/x86_64-linux-gnu" cargo test

# Option 2: Add to .cargo/config.toml (permanent)
[build]
rustflags = ["-L", "/usr/lib/x86_64-linux-gnu"]
```

### Sandbox Feature Dependencies (Optional)

If building with sandbox support (`--features sandbox`), additional dependencies are required:

```bash
sudo apt-get install -y libunwind-dev
```

## Development Tools

### Build & Test
- **Cargo**: Rust builds and tests
- **npm**: TypeScript builds and tests
- **uv**: Python dependency management and tests

### Quality
- **rustfmt**: Rust formatting
- **clippy**: Rust linting
- **ruff**: Python formatting and linting
- **TypeScript strict mode**: Type checking

### Testing Frameworks
- **Rust**: Built-in test framework + proptest + criterion
- **TypeScript**: Vitest + Playwright
- **Python**: pytest

### CI/CD
- **GitHub Actions**: Continuous integration
- **cargo-dist**: Release automation

## Infrastructure

### Database Schema
- `schema/`: SQL schema definitions
  - SQLite schema for AgentFS
  - DuckDB schema for DuckAgentFS

### Environment Variables
| Variable | Purpose |
|----------|---------|
| `AGENTFS` | Set to `1` inside sandbox |
| `AGENTFS_SANDBOX` | Sandbox type identifier |
| `AGENTFS_SESSION` | Current session ID |
| `RUST_LOG` | Logging configuration |

## Extension Points

### DuckAgentFS Extensions
- **VSS (Vector Similarity Search)**: Semantic search via embeddings
- **DuckPGQ**: Property graphs for code dependency analysis
- **HTTPFS**: Remote file access (planned)

### SDK Integrations
- **just-bash**: Safe bash execution (TypeScript)
- **Cloudflare Workers**: Edge deployment (TypeScript)
