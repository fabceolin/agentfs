# STORY-7.1: Non-Blocking Write with Background Conformance Trigger

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-7.1 |
| **Epic** | EPIC-FUSE-CONFORMANCE-001 |
| **Phase** | 7 - FUSE Write-Time Conformance |
| **Status** | Done |
| **Priority** | High |
| **File** | `cli/src/handler.rs`, `cli/src/fuse.rs` |
| **Dependencies** | STORY-4.3 (GraphDocsHandler), STORY-2.1.3 (Template Conformance) |
| **Blocks** | STORY-7.2, STORY-7.3 |

## User Story

**As a** user writing documents in template-controlled directories
**I want** my writes to complete immediately without waiting for conformance
**So that** I have a responsive editing experience while conformance runs in the background

## Story Context

**Background:** The original blocking design would make users wait for TEA transformation on every save. This non-blocking approach writes immediately to `.source` and triggers background conformance, similar to the existing `.source` pattern for markdown files.

**Architecture Overview:**
```
WRITE → Save to .source immediately (non-blocking)
      → If template-controlled directory:
          → Mark existing .conformant as stale (if exists)
          → Trigger background conformance process
      → Return success immediately
```

**Existing System Integration:**
- Integrates with: `cli/src/handler.rs` (HandlerRegistry, FileHandler trait)
- Uses: `sdk/rust/src/graphdocs/conformance.rs` (TemplateManager)
- Pattern: Follows existing `.source` file handling in FUSE
- Technology: Rust, FUSE, Tokio async, background tasks

**What's Being Added:**
- `ConformanceWriteHandler` at priority 25
- Non-blocking write that saves to `.source`
- Stale marking of existing `.conformant` files
- Background task spawn for conformance process

## Acceptance Criteria

- [x] `ConformanceWriteHandler` registered at priority 25
- [x] `can_handle()` returns `true` for `.md` files in directories with templates
- [x] `can_handle()` returns `false` for `.gd.md`, `.source`, `.conformant*` files
- [x] Write saves to `.source` file immediately (non-blocking)
- [x] If `.conformant` exists, rename to `.conformant.stale.{timestamp}`
- [x] Background conformance task spawned after write
- [x] Write returns success immediately (does not wait for conformance)
- [x] Non-template directories pass through to default handler
- [x] Template detection results cached per directory

## Tasks / Subtasks

- [x] Task 1: Create ConformanceConfig struct (AC: 1)
  - [x] Define `agents_dir: PathBuf`
  - [x] Define `overlay: Option<PathBuf>`
  - [x] Define `model_path: Option<PathBuf>`
  - [x] Define `timeout_secs: u64` (default 30)
  - [x] Implement `Default` trait

- [x] Task 2: Create ConformanceWriteHandler struct (AC: 1, 8)
  - [x] Define `pool: DuckConnectionPool`
  - [x] Define `fs: Arc<dyn FileSystem>`
  - [x] Define `config: ConformanceConfig`
  - [x] Define `template_cache: Mutex<HashMap<PathBuf, Option<PathBuf>>>`
  - [x] Define `runtime: Handle` for spawning background tasks

- [x] Task 3: Implement helper methods (AC: 2, 3, 9)
  - [x] `is_conformance_file(path: &str) -> bool` - check for `.source`, `.conformant*`
  - [x] `get_source_path(path: &str) -> String` - add `.source` suffix
  - [x] `get_conformant_path(path: &str) -> String` - add `.conformant` suffix
  - [x] `has_template(&self, dir: &Path) -> Option<PathBuf>` - cached template detection

- [x] Task 4: Implement write handler (AC: 4, 5, 6, 7)
  - [x] Save content to `.source` file
  - [x] Check for existing `.conformant` file
  - [x] If exists, rename to `.conformant.stale.{timestamp}`
  - [x] Spawn background conformance task
  - [x] Return success immediately

- [x] Task 5: Add unit tests
  - [x] Test `is_conformance_file()` for various paths
  - [x] Test `get_source_path()` and `get_conformant_path()`
  - [x] Test `conformance_config_default()`

## Technical Specification

### File Naming Convention

```
document.md           # Logical filename (user sees this)
document.md.source    # Raw user content (hidden)
document.md.conformant     # Transformed content (hidden)
document.md.conformant.failed  # Error info if conformance failed (hidden)
document.md.conformant.stale.1705536000  # Previous conformant, pending deletion (hidden)
```

### Handler Priority Placement

```
Priority 5:   GraphDocsDirInjector (injects .graphdocs in root readdir)
Priority 25:  ConformanceWriteHandler (intercepts writes, triggers background) <- NEW
Priority 50:  GraphDocsHandler (handles /.graphdocs reads)
Priority 100: DefaultHandler (pass-through to DuckAgentFS)
```

### ConformanceWriteHandler Structure

```rust
pub struct ConformanceWriteHandler {
    pool: DuckConnectionPool,
    fs: Arc<dyn FileSystem>,
    config: ConformanceConfig,
    template_cache: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
    runtime: tokio::runtime::Handle,
}
```

### Write Implementation

```rust
#[async_trait]
impl FileHandler for ConformanceWriteHandler {
    fn name(&self) -> &str { "conformance-write" }
    fn priority(&self) -> u32 { 25 }

    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        // Skip conformance-related files
        if Self::is_conformance_file(path) {
            return false;
        }
        // Only handle markdown files
        if !path.ends_with(".md") || path.ends_with(".gd.md") {
            return false;
        }
        // Check if parent has template
        if let Some(parent) = Self::parent_dir(path) {
            self.has_template(&parent).is_some()
        } else {
            false
        }
    }

    async fn write(&self, path: &str, offset: u64, data: &[u8]) -> HandlerResult<usize> {
        let source_path = Self::get_source_path(path);

        // Write to .source immediately
        let file = self.fs.open(&source_path).await
            .or_else(|_| self.fs.create(&source_path).await)?;
        file.pwrite(offset, data).await?;

        // Mark existing .conformant as stale
        self.mark_conformant_stale(path).await;

        // Spawn background conformance task
        let pool = self.pool.clone();
        let fs = self.fs.clone();
        let config = self.config.clone();
        let path = path.to_string();
        let template_path = self.has_template(&Self::parent_dir(&path).unwrap()).unwrap();

        self.runtime.spawn(async move {
            if let Err(e) = run_background_conformance(
                pool, fs, config, path, template_path
            ).await {
                tracing::error!("Background conformance failed: {}", e);
            }
        });

        Ok(Some(data.len()))
    }

    async fn truncate(&self, path: &str, size: u64) -> HandlerResult<()> {
        let source_path = Self::get_source_path(path);

        // Truncate .source
        let file = self.fs.open(&source_path).await?;
        file.truncate(size).await?;

        // Mark existing .conformant as stale
        self.mark_conformant_stale(path).await;

        Ok(Some(()))
    }
}
```

### Stale File Management

```rust
impl ConformanceWriteHandler {
    async fn mark_conformant_stale(&self, path: &str) {
        let conformant_path = Self::get_conformant_path(path);

        // Check if .conformant exists
        if self.fs.stat(&conformant_path).await.ok().flatten().is_some() {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let stale_path = format!("{}.stale.{}", conformant_path, timestamp);

            // Rename to stale
            if let Err(e) = self.fs.rename(&conformant_path, &stale_path).await {
                tracing::warn!("Failed to mark conformant as stale: {}", e);
            } else {
                tracing::debug!("Marked {} as stale: {}", conformant_path, stale_path);
            }
        }
    }

    fn is_conformance_file(path: &str) -> bool {
        path.ends_with(".source") ||
        path.ends_with(".conformant") ||
        path.contains(".conformant.failed") ||
        path.contains(".conformant.stale.")
    }

    fn get_source_path(path: &str) -> String {
        format!("{}.source", path)
    }

    fn get_conformant_path(path: &str) -> String {
        format!("{}.conformant", path)
    }
}
```

## Dev Notes

### Non-Blocking Semantics

The key insight is that users don't need to wait for conformance. The `.source` file contains their raw content immediately. The `.conformant` file will be created asynchronously, and reads will resolve to it once ready.

### Race Condition Handling

If user saves again before conformance completes:
1. New write saves to `.source` (overwrites)
2. Existing `.conformant` (if any) marked stale
3. New background conformance spawned
4. Previous conformance task will fail to create `.conformant` (stale check)

### Background Task Isolation

Each conformance task should check if the source has been modified since it started:
```rust
async fn run_background_conformance(...) {
    // Read .source content
    let content = fs.read_file(&source_path).await?;
    let source_mtime = fs.stat(&source_path).await?.mtime;

    // ... run conformance ...

    // Before writing .conformant, verify source unchanged
    let current_mtime = fs.stat(&source_path).await?.mtime;
    if current_mtime != source_mtime {
        tracing::info!("Source modified during conformance, skipping write");
        return Ok(()); // Don't write stale conformant
    }

    // Write .conformant
    fs.write_file(&conformant_path, &transformed).await?;
}
```

### Source Tree Reference

```
cli/src/
├── handler.rs        # ConformanceWriteHandler implementation
└── fuse.rs           # FUSE operations (unchanged for writes)

sdk/rust/src/graphdocs/
└── conformance.rs    # TemplateManager::detect_template()
```

## Integration Tests

### Test Document: Real Story Format

Use the story-tmpl.yaml template and a real story document for testing:

```markdown
# STORY-WRITE-001: Test Write Handler

## Status
Draft

## Story
**As a** developer,
**I want** to test the write handler,
**so that** writes save to .source correctly

## Acceptance Criteria
1. Write saves to .source immediately
2. Existing .conformant is marked stale
3. Background conformance is triggered

## Tasks / Subtasks
- [ ] Task 1: Implement write handler
- [ ] Task 2: Add stale marking

## Dev Notes
Testing non-blocking write behavior.

## Change Log
| Date | Version | Description | Author |
|------|---------|-------------|--------|
| 2026-01-18 | 1.0 | Created | Test |
```

### Integration Test: Write Handler Flow

```rust
#[tokio::test]
async fn test_write_handler_saves_to_source() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());
    let runtime = tokio::runtime::Handle::current();

    // Create template in directory
    let template = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template.as_bytes()).await.unwrap();

    // Create handler with config
    let config = ConformanceConfig::default();
    let handler = ConformanceWriteHandler::new(
        pool.clone(),
        fs.clone(),
        config,
        runtime.clone(),
    );

    // Test can_handle
    assert!(handler.can_handle("/docs/stories/STORY-001.md", None), "Should handle .md in template dir");
    assert!(!handler.can_handle("/docs/stories/STORY-001.md.source", None), "Should NOT handle .source");
    assert!(!handler.can_handle("/docs/stories/STORY-001.md.conformant", None), "Should NOT handle .conformant");
    assert!(!handler.can_handle("/docs/stories/template-story.gd.md", None), "Should NOT handle .gd.md");
    assert!(!handler.can_handle("/other/README.md", None), "Should NOT handle non-template dir");

    // Test write saves to .source
    let content = b"# STORY-001: Test\n\n## Status\nDraft";
    let result = handler.write("/docs/stories/STORY-001.md", 0, content).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some(content.len()));

    // Verify .source was created
    let source_content = fs.read_file("/docs/stories/STORY-001.md.source").await;
    assert!(source_content.is_ok(), "Should create .source file");
    assert_eq!(source_content.unwrap(), content.to_vec());
}

#[tokio::test]
async fn test_write_handler_marks_conformant_stale() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());
    let runtime = tokio::runtime::Handle::current();

    // Create template
    let template = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template.as_bytes()).await.unwrap();

    // Create existing .conformant file
    fs.write_file("/docs/stories/STORY-001.md.conformant", b"old conformant content").await.unwrap();

    let config = ConformanceConfig::default();
    let handler = ConformanceWriteHandler::new(pool, fs.clone(), config, runtime);

    // Write new content
    let content = b"# STORY-001: Updated\n\n## Status\nInProgress";
    handler.write("/docs/stories/STORY-001.md", 0, content).await.unwrap();

    // Verify .conformant is renamed to .stale.*
    let conformant = fs.read_file("/docs/stories/STORY-001.md.conformant").await;
    assert!(conformant.is_err(), "Original .conformant should be renamed");

    // Find stale file
    let entries = fs.readdir("/docs/stories").await.unwrap().unwrap();
    let stale_files: Vec<_> = entries.iter()
        .filter(|e| e.contains(".conformant.stale."))
        .collect();
    assert!(!stale_files.is_empty(), "Should create .stale file");
}

#[tokio::test]
async fn test_write_handler_triggers_background_conformance() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());
    let runtime = tokio::runtime::Handle::current();

    // Create template
    let template = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template.as_bytes()).await.unwrap();

    let config = ConformanceConfig::default();
    let handler = ConformanceWriteHandler::new(pool.clone(), fs.clone(), config, runtime);

    // Write content
    let content = r#"# STORY-BG-001: Background Test

## Status
Draft

## Story
**As a** tester,
**I want** to verify background tasks,
**so that** conformance runs asynchronously
"#;
    handler.write("/docs/stories/STORY-BG-001.md", 0, content.as_bytes()).await.unwrap();

    // Wait for background task
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Check if .conformant was created (or .failed)
    let conformant = fs.read_file("/docs/stories/STORY-BG-001.md.conformant").await;
    let failed = fs.read_file("/docs/stories/STORY-BG-001.md.conformant.failed").await;

    assert!(
        conformant.is_ok() || failed.is_ok(),
        "Background task should create .conformant or .failed"
    );
}
```

## Risk Assessment

**Primary Risk:** Background task overwhelming system with many saves
**Mitigation:** Debounce or queue conformance tasks per file

**Secondary Risk:** Stale files accumulating
**Mitigation:** STORY-7.4 handles cleanup when new .conformant ready

**Tertiary Risk:** User confusion about which version they're editing
**Mitigation:** Reads always show .conformant if available (STORY-7.3)

## Definition of Done

- [x] `ConformanceWriteHandler` registered at priority 25
- [x] Writes save to `.source` immediately
- [x] Existing `.conformant` marked as stale on write
- [x] Background conformance task spawned
- [x] Write returns immediately (non-blocking)
- [x] Unit tests pass (140 tests, including 11 conformance tests)
- [x] Clippy clean (no new warnings introduced)

---

## Dev Agent Record

### Agent Model Used
Claude Opus 4.5 (claude-opus-4-5-20251101)

### Debug Log References
- N/A - Implementation completed without major debugging issues

### Completion Notes
- ConformanceWriteHandler implemented at cli/src/handler.rs:1147-1315
- ConformanceConfig struct at cli/src/handler.rs:1096-1129
- Helper methods (is_conformance_file, get_source_path, get_conformant_path) at cli/src/handler.rs:1169-1199
- Unit tests at cli/src/handler.rs:2671-2716
- All 140 tests pass, clippy clean

### File List
| File | Action | Description |
|------|--------|-------------|
| cli/src/handler.rs | Modified | Added ConformanceConfig, ConformanceWriteHandler, helper methods, unit tests |
| cli/src/parser.rs | Modified | Added TEA conformance CLI arguments |
| cli/src/cmd/mount.rs | Modified | Added MountArgs fields, wired to ConformanceConfig |
| cli/src/main.rs | Modified | Wired new CLI arguments to MountArgs |

---

## Change Log

| Date | Change | Reason |
|------|--------|--------|
| 2026-01-17 | Story created | EPIC-FUSE-CONFORMANCE-001 planning |
| 2026-01-18 | Revised for non-blocking architecture | User feedback - prefer async conformance |
| 2026-01-18 | Implementation complete | All tasks and acceptance criteria completed |

---

## QA Results

### Review Date: 2026-01-18

### Reviewed By: Quinn (Test Architect)

### Code Quality Assessment

Implementation is well-structured and follows the FileHandler trait pattern consistently. The ConformanceWriteHandler at priority 25 correctly intercepts writes to template-controlled directories, saves to `.source` files immediately, marks existing `.conformant` files as stale with timestamps, and spawns background conformance tasks. Code is clean and well-documented.

Key implementation locations:
- `ConformanceConfig` struct: `cli/src/handler.rs:1086-1107`
- `ConformanceWriteHandler` struct: `cli/src/handler.rs:1123-1129`
- Helper methods: `cli/src/handler.rs:1148-1208`
- Write handler impl: `cli/src/handler.rs:1211-1303`

### Refactoring Performed

None required - implementation is clean.

### Compliance Check

- Coding Standards: ✓ Follows Rust conventions, proper error handling
- Project Structure: ✓ Handler in appropriate module
- Testing Strategy: ✓ 25 unit tests covering conformance handlers
- All ACs Met: ✓ All 9 acceptance criteria verified

### Improvements Checklist

- [x] ConformanceWriteHandler at priority 25
- [x] `can_handle()` returns true for .md files in template directories
- [x] `can_handle()` returns false for .gd.md, .source, .conformant* files
- [x] Write saves to .source file immediately
- [x] Existing .conformant renamed to .conformant.stale.{timestamp}
- [x] Background conformance task spawned after write
- [x] Write returns success immediately (non-blocking)
- [x] Non-template directories pass through to default handler
- [x] Template detection results cached per directory
- [ ] Consider implementing task debouncing for rapid saves (PERF-001)
- [ ] Document handler priority allocation scheme (TECH-001)

### Security Review

No security concerns. File operations delegate to underlying DuckAgentFS. No external input validation required - content is user-controlled.

### Performance Considerations

**PERF-001 (Medium)**: Background task spawning without debounce may overwhelm system on rapid autosaves. Consider implementing per-file debouncing (500ms delay after last write) or bounded concurrency.

**PERF-002 (Low)**: Template detection I/O on `can_handle()` is mitigated by caching.

### Files Modified During Review

None - implementation is complete and clean.

### Gate Status

Gate: **PASS** → docs/qa/gates/7.1-template-aware-write-handler.yml
Risk profile: docs/qa/assessments/7.1-risk-20260117.md

### Recommended Status

✓ Ready for Done - All acceptance criteria met, 25 tests pass, implementation is clean. Minor performance improvements can be addressed in future iteration.
