# Coding Standards

This document defines the coding standards and conventions for the AgentFS project.

## Language-Specific Standards

### Rust (CLI, SDK, Sandbox)

**Edition**: Rust 2021

**Formatting**:
- Use `rustfmt` with default settings
- Run `cargo fmt` before committing

**Linting**:
- Run `cargo clippy` and address all warnings
- Enable `#![warn(clippy::all)]` in library crates

**Error Handling**:
- Use `thiserror` for library error types (sdk/rust)
- Use `anyhow` for application errors (cli)
- Prefer `Result<T, E>` over panics
- Document error conditions in public APIs

**Async Patterns**:
- Use `tokio` runtime for async code
- Use `async-trait` for async trait methods
- Prefer `async fn` over manual `Future` implementations

**Naming Conventions**:
- Types: `PascalCase`
- Functions/methods: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Modules: `snake_case`
- Traits: `PascalCase`, descriptive nouns (e.g., `FileSystem`, `EmbeddingGenerator`)

**Documentation**:
- Document all public items with `///` doc comments
- Include examples in doc comments for complex APIs
- Use `//!` for module-level documentation

### TypeScript (SDK)

**Version**: TypeScript 5.3+

**Module System**: ESM (`"type": "module"`)

**Formatting**:
- Use consistent indentation (2 spaces)
- Use semicolons

**Type Safety**:
- Enable strict mode in `tsconfig.json`
- Avoid `any` type; prefer `unknown` for untyped values
- Define explicit return types for public functions

**Exports**:
- Use named exports over default exports
- Re-export public APIs from `index.ts`

**Testing**:
- Use Vitest for unit and integration tests
- Browser tests use Playwright via `@vitest/browser`

### Python (SDK)

**Version**: Python 3.9+

**Package Manager**: `uv`

**Formatting**:
- Use `ruff format` for code formatting
- Use `ruff check` for linting

**Type Hints**:
- Use type hints for all public functions
- Use `from __future__ import annotations` for forward references

**Docstrings**:
- Use Google-style docstrings for public APIs

## Project-Wide Conventions

### Git Workflow

**Branches**:
- `main`: Production-ready code
- Feature branches: `feature/<description>` or `<issue-number>-<description>`

**Commits**:
- Use conventional commit format: `type(scope): description`
- Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`
- Keep commits atomic and focused

### File Organization

**Module Structure**:
- Keep related code together in modules
- Separate interface definitions from implementations
- Use `mod.rs` or `index.ts` for module re-exports

**Test Organization**:
- Unit tests: Inline in Rust (`#[cfg(test)]`) or adjacent `*.test.ts`/`test_*.py`
- Integration tests: Separate `tests/` directory
- Benchmarks: `benches/` directory (Rust)

### API Design

**Consistency**:
- All SDKs expose the same three main interfaces:
  - `AgentFS`: Main entry point
  - `KvStore`: Key-value operations
  - `FileSystem`: File operations
  - `ToolCalls`: Audit log operations

**Naming**:
- Use consistent method names across SDKs:
  - `read_file`, `write_file`, `mkdir`, `readdir`, `stat`
  - `get`, `set`, `delete`, `list` (for KV store)
  - `start`, `success`, `error`, `record` (for tool calls)

**Error Messages**:
- Be descriptive and actionable
- Include relevant context (file paths, operation attempted)

### Security

**Input Validation**:
- Validate all external inputs
- Sanitize file paths to prevent directory traversal
- Use parameterized queries for database operations

**Agent ID Validation**:
- Only allow alphanumeric characters, hyphens, and underscores
- Reject paths that could escape the AgentFS directory

### Performance

**Connection Pooling**:
- Use connection pools for database access
- Implement RAII-style transaction management

**Caching**:
- Use `DentryCache` for directory entry lookups
- Cache configuration values where appropriate

**Lazy Loading**:
- Initialize heavy resources only when needed
- Use async initialization where possible
