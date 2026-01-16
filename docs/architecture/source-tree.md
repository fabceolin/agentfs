# Source Tree

This document describes the directory structure and organization of the AgentFS codebase.

## Repository Root

```
agentfs/
├── cli/                    # Command-line interface (Rust)
├── sdk/                    # Software Development Kits
│   ├── rust/               # Rust SDK
│   ├── typescript/         # TypeScript SDK
│   └── python/             # Python SDK
├── sandbox/                # Linux syscall sandbox (Rust)
├── schema/                 # Database schema definitions
├── docs/                   # Documentation
├── examples/               # Example applications
├── scripts/                # Build and utility scripts
├── licenses/               # License files
└── web-bundles/            # Web-based agent bundles
```

## CLI (`cli/`)

Command-line tool for managing AgentFS filesystems.

```
cli/
├── Cargo.toml              # Package manifest
├── src/
│   ├── main.rs             # Entry point, command dispatch
│   ├── lib.rs              # Library exports
│   ├── parser.rs           # Clap argument definitions
│   ├── fuse.rs             # FUSE filesystem (Linux)
│   ├── nfs.rs              # NFS server (cross-platform)
│   ├── handler.rs          # FileHandler trait and registry
│   ├── cmd/                # Command implementations
│   │   ├── init.rs         # `agentfs init`
│   │   ├── run.rs          # `agentfs run`
│   │   ├── mount.rs        # `agentfs mount`
│   │   ├── fs.rs           # `agentfs fs` subcommands
│   │   ├── timeline.rs     # `agentfs timeline`
│   │   ├── sync.rs         # `agentfs sync`
│   │   ├── mcp_server.rs   # `agentfs mcp-server`
│   │   └── nfs.rs          # `agentfs nfs`
│   ├── fuser/              # Vendored pure-Rust FUSE implementation
│   └── sandbox/            # Platform-specific sandboxing
│       ├── linux.rs        # User namespace + FUSE overlay
│       ├── linux_ptrace.rs # ptrace-based syscall interception
│       └── darwin.rs       # macOS sandbox-exec + NFS
├── tests/
│   └── syscall/            # Syscall test suite
├── perf/
│   └── syscall/            # Syscall performance tests
└── scripts/                # CLI-specific scripts
```

## Rust SDK (`sdk/rust/`)

Core library implementing filesystem and storage abstractions.

```
sdk/rust/
├── Cargo.toml              # Package manifest
├── src/
│   ├── lib.rs              # Public API exports
│   ├── error.rs            # Error type definitions
│   ├── connection_pool.rs  # Database connection pooling
│   ├── kvstore.rs          # Key-value store implementation
│   ├── toolcalls.rs        # Tool call audit logging
│   ├── embedding.rs        # Embedding generator trait
│   └── filesystem/
│       ├── mod.rs          # FileSystem trait definition
│       ├── agentfs.rs      # SQLite-backed filesystem
│       ├── duckagentfs.rs  # DuckDB-backed filesystem
│       ├── hostfs.rs       # Host filesystem passthrough
│       └── overlayfs.rs    # Copy-on-write overlay
└── benches/
    ├── overlayfs.rs        # Overlay filesystem benchmarks
    └── workload.rs         # General workload benchmarks
```

## TypeScript SDK (`sdk/typescript/`)

TypeScript/JavaScript library for Node.js and browsers.

```
sdk/typescript/
├── package.json            # Package manifest
├── tsconfig.json           # TypeScript configuration
├── vitest.config.ts        # Test configuration
├── vitest.browser.config.ts
├── src/
│   ├── index_node.ts       # Node.js entry point
│   ├── index_browser.ts    # Browser entry point
│   ├── agentfs.ts          # Main AgentFS class
│   ├── kvstore.ts          # Key-value store
│   ├── toolcalls.ts        # Tool call tracking
│   ├── errors.ts           # Error definitions
│   ├── guards.ts           # Type guards
│   ├── filesystem/
│   │   ├── index.ts        # Filesystem exports
│   │   ├── interface.ts    # FileSystem interface
│   │   └── agentfs.ts      # SQLite filesystem implementation
│   └── integrations/
│       ├── just-bash/      # just-bash integration
│       │   ├── index.ts
│       │   └── AgentFs.ts
│       └── cloudflare/     # Cloudflare Workers integration
│           ├── index.ts
│           └── agentfs.ts
├── tests/                  # Node.js tests
├── tests_browser/          # Browser-specific tests
└── examples/               # Usage examples
    ├── filesystem/
    ├── kvstore/
    └── toolcalls/
```

## Python SDK (`sdk/python/`)

Python library for AgentFS access.

```
sdk/python/
├── pyproject.toml          # Package manifest
├── agentfs_sdk/
│   ├── __init__.py         # Package exports
│   ├── agentfs.py          # Main AgentFS class
│   ├── filesystem.py       # Filesystem operations
│   ├── kvstore.py          # Key-value store
│   ├── toolcalls.py        # Tool call tracking
│   ├── errors.py           # Error definitions
│   ├── guards.py           # Validation helpers
│   └── constants.py        # Shared constants
├── tests/                  # Test suite
└── examples/               # Usage examples
```

## Sandbox (`sandbox/`)

Linux-specific syscall interception for secure execution.

```
sandbox/
├── Cargo.toml              # Package manifest
└── src/
    ├── lib.rs              # Library exports
    ├── sandbox/
    │   └── mod.rs          # Sandbox orchestration
    ├── syscall/            # Syscall handlers
    │   ├── mod.rs
    │   ├── file.rs         # File operation syscalls
    │   ├── stat.rs         # Stat syscalls
    │   ├── xattr.rs        # Extended attribute syscalls
    │   └── process.rs      # Process syscalls
    └── vfs/                # Virtual filesystem
        ├── mod.rs
        ├── file.rs         # File abstraction
        ├── fdtable.rs      # File descriptor table
        ├── mount.rs        # Mount management
        ├── bind.rs         # Bind mounts
        └── sqlite.rs       # SQLite VFS
```

## Schema (`schema/`)

Database schema definitions.

```
schema/
├── agentfs.sql             # SQLite schema for AgentFS
└── duckagentfs.sql         # DuckDB schema for DuckAgentFS
```

## Documentation (`docs/`)

Project documentation following BMAD methodology.

```
docs/
├── architecture/           # Architecture documentation (sharded)
│   ├── coding-standards.md
│   ├── tech-stack.md
│   └── source-tree.md
├── epics/                  # Epic specifications
│   ├── EPIC-DUCKAGENTFS-001.md
│   └── EPIC-GRAPHDOCS-001.md
├── stories/                # User stories
│   ├── duckagentfs/        # DuckAgentFS implementation stories
│   └── graphdocs/          # GraphDocs implementation stories
├── qa/                     # Quality assurance
│   ├── assessments/        # Test design assessments
│   └── gates/              # QA gate results
└── fuse-handler-integration.md
```

## Examples (`examples/`)

Reference implementations and integrations.

```
examples/
├── ai-sdk-just-bash/       # Vercel AI SDK + just-bash
├── claude-agent/           # Anthropic Claude agent
│   └── research-assistant/
├── mastra/                 # Mastra framework
│   └── research-assistant/
├── openai-agents/          # OpenAI Agents SDK
│   └── research-assistant/
├── cloudflare/             # Cloudflare Workers deployment
└── firecracker/            # Firecracker microVM
```

## Key Files

| File | Purpose |
|------|---------|
| `CLAUDE.md` | Claude Code configuration |
| `README.md` | Project overview |
| `TESTING.md` | Test configuration guide |
| `Cargo.toml` (root) | Workspace definition |
| `.github/` | GitHub Actions workflows |
