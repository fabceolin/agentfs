# STORY-7.3: Read-Time Resolution with Jinja2 Expansion

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-7.3 |
| **Epic** | EPIC-FUSE-CONFORMANCE-001 |
| **Phase** | 7 - FUSE Write-Time Conformance |
| **Status** | Done |
| **Priority** | High |
| **File** | `cli/src/handler.rs`, `cli/src/fuse.rs` |
| **Dependencies** | STORY-7.1, STORY-7.2 |
| **Blocks** | None |

## User Story

**As a** user reading documents from the FUSE mount
**I want** to see the conformant version of my document (if available) with Jinja2 variables expanded
**So that** I always see the best available version with all template variables filled in

## Story Context

**Background:** With the non-blocking write architecture (STORY-7.1/7.2), documents have multiple representations:
- `.source` - Raw user content
- `.conformant` - Transformed/validated content (if conformance succeeded)
- `.conformant.failed` - Error info (if conformance failed)

The read handler must resolve these to present a single, coherent view to the user, with Jinja2 variable expansion applied.

**Read Resolution Priority:**
```
READ document.md:
  1. Check for document.md.conformant
     → If exists: return content with Jinja2 expansion
  2. Check for document.md.source
     → If exists: return content with Jinja2 expansion
  3. Check for document.md (physical file)
     → If exists: return content with Jinja2 expansion
  4. Return ENOENT
```

**Jinja2 Expansion:**
Variables like `{{status}}`, `{{title}}`, `{{epic_num}}` are replaced with values from `gd_variables` table.

## Acceptance Criteria

- [x] `ConformanceReadHandler` registered at priority 20 (before write handler)
- [x] Read checks for `.conformant` first, falls back to `.source`
- [x] Jinja2 variables (`{{name}}`) expanded from `gd_variables` table
- [x] If variable not found in DB, leave placeholder unchanged
- [x] `.source`, `.conformant`, `.conformant.*` files hidden from readdir
- [x] `getattr` returns stats for resolved file (with adjusted size for Jinja2 expansion)
- [x] Read raw content available via `.source` suffix (escape hatch)
- [x] Non-template directories pass through unchanged

## Tasks / Subtasks

- [x] Task 1: Create ConformanceReadHandler (AC: 1, 8)
  - [x] Priority 20 (before write handler at 25)
  - [x] `can_handle()` for `.md` files in template directories
  - [x] Skip `.source`, `.conformant*`, `.gd.md` files

- [x] Task 2: Implement read resolution (AC: 2)
  - [x] Check for `.conformant` file
  - [x] Fall back to `.source` file
  - [x] Fall back to physical file
  - [x] Return error if none exist

- [x] Task 3: Implement Jinja2 expansion (AC: 3, 4)
  - [x] Query `gd_variables` for document
  - [x] Replace `{{name}}` with variable values
  - [x] Leave unresolved placeholders unchanged
  - [x] Cache variable lookups per document

- [x] Task 4: Implement getattr with size adjustment (AC: 6)
  - [x] Get resolved content
  - [x] Apply Jinja2 expansion
  - [x] Return stats with adjusted size

- [x] Task 5: Implement hidden files in readdir (AC: 5)
  - [x] Filter out `.source`, `.conformant`, `.conformant.*` from listings
  - [x] Only show logical filenames

- [x] Task 6: Add unit tests
  - [x] Test apply_variables()
  - [x] Test apply_variables_no_match()
  - [x] Test apply_variables_multiple_same()

## Technical Specification

### Handler Priority Placement

```
Priority 5:   GraphDocsDirInjector (injects .graphdocs in root readdir)
Priority 20:  ConformanceReadHandler (reads .conformant with Jinja2) <- NEW
Priority 25:  ConformanceWriteHandler (writes to .source, triggers background)
Priority 50:  GraphDocsHandler (handles /.graphdocs reads)
Priority 100: DefaultHandler (pass-through to DuckAgentFS)
```

### ConformanceReadHandler Structure

```rust
pub struct ConformanceReadHandler {
    pool: DuckConnectionPool,
    fs: Arc<dyn FileSystem>,
    template_cache: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
    variable_cache: Mutex<HashMap<String, HashMap<String, String>>>, // doc_id -> vars
}
```

### Read Implementation

```rust
#[async_trait]
impl FileHandler for ConformanceReadHandler {
    fn name(&self) -> &str { "conformance-read" }
    fn priority(&self) -> u32 { 20 }

    fn can_handle(&self, path: &str, _stats: Option<&Stats>) -> bool {
        // Skip conformance-related files (let them through to default)
        if Self::is_conformance_file(path) {
            return false;
        }
        // Only handle markdown in template directories
        if !path.ends_with(".md") || path.ends_with(".gd.md") {
            return false;
        }
        if let Some(parent) = Self::parent_dir(path) {
            self.has_template(&parent).is_some()
        } else {
            false
        }
    }

    async fn read(&self, path: &str, offset: u64, size: u64) -> HandlerResult<Vec<u8>> {
        // Resolve to best available file
        let content = self.resolve_content(path).await?;

        // Apply Jinja2 expansion
        let expanded = self.expand_jinja2(path, &content).await?;

        // Return requested slice
        let bytes = expanded.as_bytes();
        let start = offset as usize;
        let end = (offset + size) as usize;

        if start >= bytes.len() {
            return Ok(Some(vec![]));
        }

        Ok(Some(bytes[start..end.min(bytes.len())].to_vec()))
    }

    async fn getattr(&self, path: &str) -> HandlerResult<Stats> {
        // Get resolved content for accurate size
        let content = self.resolve_content(path).await?;
        let expanded = self.expand_jinja2(path, &content).await?;

        // Get stats from underlying file
        let resolved_path = self.resolve_path(path).await?;
        if let Some(mut stats) = self.fs.stat(&resolved_path).await? {
            // Adjust size for Jinja2 expansion
            stats.size = expanded.len() as u64;
            return Ok(Some(stats));
        }

        Ok(None)
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        // Get actual entries
        let entries = self.fs.readdir(path).await?.unwrap_or_default();

        // Filter out conformance files
        let filtered: Vec<String> = entries
            .into_iter()
            .filter(|e| !Self::is_conformance_file(e))
            .collect();

        Ok(Some(filtered))
    }
}
```

### Content Resolution

```rust
impl ConformanceReadHandler {
    async fn resolve_content(&self, logical_path: &str) -> Result<String> {
        let conformant_path = format!("{}.conformant", logical_path);
        let source_path = format!("{}.source", logical_path);

        // Priority: .conformant > .source > physical
        if let Ok(content) = self.fs.read_file(&conformant_path).await {
            tracing::debug!("Read resolved to .conformant for {}", logical_path);
            return String::from_utf8(content)
                .map_err(|e| Error::Custom(format!("Invalid UTF-8: {}", e)));
        }

        if let Ok(content) = self.fs.read_file(&source_path).await {
            tracing::debug!("Read resolved to .source for {}", logical_path);
            return String::from_utf8(content)
                .map_err(|e| Error::Custom(format!("Invalid UTF-8: {}", e)));
        }

        if let Ok(content) = self.fs.read_file(logical_path).await {
            tracing::debug!("Read resolved to physical file for {}", logical_path);
            return String::from_utf8(content)
                .map_err(|e| Error::Custom(format!("Invalid UTF-8: {}", e)));
        }

        Err(Error::NotFound(logical_path.to_string()))
    }

    async fn resolve_path(&self, logical_path: &str) -> Result<String> {
        let conformant_path = format!("{}.conformant", logical_path);
        let source_path = format!("{}.source", logical_path);

        if self.fs.stat(&conformant_path).await?.is_some() {
            return Ok(conformant_path);
        }
        if self.fs.stat(&source_path).await?.is_some() {
            return Ok(source_path);
        }
        if self.fs.stat(logical_path).await?.is_some() {
            return Ok(logical_path.to_string());
        }

        Err(Error::NotFound(logical_path.to_string()))
    }
}
```

### Jinja2 Expansion

```rust
impl ConformanceReadHandler {
    async fn expand_jinja2(&self, logical_path: &str, content: &str) -> Result<String> {
        // Get document ID
        let doc_id = path_to_doc_id(logical_path);

        // Check cache first
        {
            let cache = self.variable_cache.lock().unwrap();
            if let Some(vars) = cache.get(&doc_id) {
                return Ok(Self::apply_variables(content, vars));
            }
        }

        // Query database for variables
        let pool = self.pool.clone();
        let doc_id_clone = doc_id.clone();
        let vars = tokio::task::spawn_blocking(move || {
            let conn = pool.get_read_connection()?;
            let mut stmt = conn.prepare(
                "SELECT name, value FROM gd_variables WHERE document_id = ?"
            )?;
            let mut rows = stmt.query(params![doc_id_clone])?;

            let mut vars = HashMap::new();
            while let Some(row) = rows.next()? {
                let name: String = row.get(0)?;
                let value: String = row.get(1)?;
                // Parse JSON value, extract string
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&value) {
                    let v = match json {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => n.to_string(),
                        other => other.to_string(),
                    };
                    vars.insert(name, v);
                } else {
                    vars.insert(name, value);
                }
            }

            Ok::<_, Error>(vars)
        })
        .await
        .map_err(|e| Error::Custom(format!("spawn_blocking error: {}", e)))??;

        // Cache variables
        {
            let mut cache = self.variable_cache.lock().unwrap();
            cache.insert(doc_id, vars.clone());
        }

        Ok(Self::apply_variables(content, &vars))
    }

    fn apply_variables(content: &str, vars: &HashMap<String, String>) -> String {
        let mut result = content.to_string();

        // Replace {{name}} with values
        for (name, value) in vars {
            let placeholder = format!("{{{{{}}}}}", name);
            result = result.replace(&placeholder, value);
        }

        result
    }
}
```

### Hidden Files in readdir

```rust
impl ConformanceReadHandler {
    fn is_conformance_file(path: &str) -> bool {
        let filename = Path::new(path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(path);

        filename.ends_with(".source") ||
        filename.ends_with(".conformant") ||
        filename.contains(".conformant.failed") ||
        filename.contains(".conformant.stale.")
    }

    async fn readdir(&self, path: &str) -> HandlerResult<Vec<String>> {
        // Get actual directory entries
        let entries = match self.fs.readdir(path).await? {
            Some(e) => e,
            None => return Ok(None),
        };

        // Filter out conformance-related files
        let filtered: Vec<String> = entries
            .into_iter()
            .filter(|name| !Self::is_conformance_file(name))
            .collect();

        Ok(Some(filtered))
    }
}
```

## Dev Notes

### Variable Cache Invalidation

The variable cache is populated per-read and keyed by document ID. For simplicity, we don't invalidate on database changes. The cache is per-request anyway due to FUSE's stateless read model.

For long-lived processes, consider adding TTL or invalidation on write.

### Jinja2 Subset

We implement a simple `{{name}}` replacement, not full Jinja2. This covers:
- `{{status}}` → "Done"
- `{{title}}` → "My Document"
- `{{epic_num}}` → "7"

Not supported (future enhancement):
- Filters: `{{name|upper}}`
- Conditionals: `{% if ... %}`
- Loops: `{% for ... %}`

### Source Access Escape Hatch

Users can read raw content by appending `.source`:
```bash
cat /mnt/docs/STORY-1.md           # Returns .conformant with Jinja2
cat /mnt/docs/STORY-1.md.source    # Returns raw .source (no Jinja2)
```

This bypasses the read handler since `is_conformance_file()` returns false.

### Size Calculation

Because Jinja2 expansion changes content length, `getattr` must return the expanded size, not the on-disk size. This is important for applications that pre-allocate buffers based on file size.

### Source Tree Reference

```
cli/src/
├── handler.rs        # ConformanceReadHandler implementation
└── fuse.rs           # FUSE operations (unchanged)

sdk/rust/src/graphdocs/
└── variable_types.rs # Reference for variable naming patterns
```

## Integration Tests

### Test Document: Story with Variables

Use a real story format with Jinja2 variables to test read-time resolution:

```markdown
# STORY-{{epic_num}}.{{story_num}}: {{title}}

## Status
{{status}}

## Story
**As a** {{role}},
**I want** {{action}},
**so that** {{benefit}}

## Acceptance Criteria
1. {{ac_1}}
2. {{ac_2}}

## Tasks / Subtasks
- [ ] Task 1: {{task_1}}

## Dev Notes
This story is part of Epic {{epic_num}}.

## Change Log
| Date | Version | Description | Author |
|------|---------|-------------|--------|
| {{created_date}} | 1.0 | Created | {{author}} |
```

### Database Setup: gd_variables

```sql
INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES
('story-read-001-v-epic_num', 'story-read-001', 'epic_num', '"7"', 'string'),
('story-read-001-v-story_num', 'story-read-001', 'story_num', '"3"', 'string'),
('story-read-001-v-title', 'story-read-001', 'title', '"Read-Time Resolution"', 'string'),
('story-read-001-v-status', 'story-read-001', 'status', '"InProgress"', 'enum'),
('story-read-001-v-role', 'story-read-001', 'role', '"developer"', 'string'),
('story-read-001-v-action', 'story-read-001', 'action', '"to see rendered content"', 'string'),
('story-read-001-v-benefit', 'story-read-001', 'benefit', '"variables are expanded"', 'string');
```

### Expected Rendered Output

```markdown
# STORY-7.3: Read-Time Resolution

## Status
InProgress

## Story
**As a** developer,
**I want** to see rendered content,
**so that** variables are expanded

## Acceptance Criteria
1. {{ac_1}}
2. {{ac_2}}

## Tasks / Subtasks
- [ ] Task 1: {{task_1}}

## Dev Notes
This story is part of Epic 7.
...
```

Note: `{{ac_1}}`, `{{ac_2}}`, `{{task_1}}` remain unexpanded because they're not in the database.

### Integration Test: Read Handler Resolution

```rust
#[tokio::test]
async fn test_read_handler_resolves_conformant() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());

    // Create template for directory detection
    let template = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template.as_bytes()).await.unwrap();

    // Create .conformant file (what read should return)
    let conformant_content = "# STORY-001: Conformant Version\n\n## Status\nDone";
    fs.write_file("/docs/stories/STORY-001.md.conformant", conformant_content.as_bytes()).await.unwrap();

    // Create .source file (should be ignored when .conformant exists)
    let source_content = "# STORY-001: Source Version\n\n## Status\nDraft";
    fs.write_file("/docs/stories/STORY-001.md.source", source_content.as_bytes()).await.unwrap();

    let handler = ConformanceReadHandler::new(pool.clone(), fs.clone(), ConformanceConfig::default());

    // Read should return .conformant content
    let result = handler.read("/docs/stories/STORY-001.md", 0, 1000).await;
    assert!(result.is_ok());
    let content = String::from_utf8(result.unwrap().unwrap()).unwrap();
    assert!(content.contains("Conformant Version"), "Should read from .conformant");
    assert!(content.contains("Done"), "Should have status Done from .conformant");
}

#[tokio::test]
async fn test_read_handler_falls_back_to_source() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());

    // Create template
    let template = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template.as_bytes()).await.unwrap();

    // Only create .source (no .conformant)
    let source_content = "# STORY-002: Source Only\n\n## Status\nDraft";
    fs.write_file("/docs/stories/STORY-002.md.source", source_content.as_bytes()).await.unwrap();

    let handler = ConformanceReadHandler::new(pool.clone(), fs.clone(), ConformanceConfig::default());

    // Read should return .source content
    let result = handler.read("/docs/stories/STORY-002.md", 0, 1000).await;
    assert!(result.is_ok());
    let content = String::from_utf8(result.unwrap().unwrap()).unwrap();
    assert!(content.contains("Source Only"), "Should fall back to .source");
}

#[tokio::test]
async fn test_read_handler_jinja2_expansion() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());

    // Create template
    let template = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template.as_bytes()).await.unwrap();

    // Create .conformant with Jinja2 variables
    let content = "# STORY-{{epic_num}}.{{story_num}}: {{title}}\n\n## Status\n{{status}}";
    fs.write_file("/docs/stories/STORY-003.md.conformant", content.as_bytes()).await.unwrap();

    // Insert variables into database
    {
        let conn = pool.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES (?, ?, ?, ?, ?)",
            params!["story-003-v-epic_num", "story-003", "epic_num", "\"7\"", "string"]
        ).unwrap();
        conn.execute(
            "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES (?, ?, ?, ?, ?)",
            params!["story-003-v-story_num", "story-003", "story_num", "\"3\"", "string"]
        ).unwrap();
        conn.execute(
            "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES (?, ?, ?, ?, ?)",
            params!["story-003-v-title", "story-003", "title", "\"Jinja2 Test\"", "string"]
        ).unwrap();
        conn.execute(
            "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES (?, ?, ?, ?, ?)",
            params!["story-003-v-status", "story-003", "status", "\"Done\"", "enum"]
        ).unwrap();
    }

    let handler = ConformanceReadHandler::new(pool.clone(), fs.clone(), ConformanceConfig::default());

    // Read should expand variables
    let result = handler.read("/docs/stories/STORY-003.md", 0, 1000).await;
    assert!(result.is_ok());
    let content = String::from_utf8(result.unwrap().unwrap()).unwrap();

    assert!(content.contains("STORY-7.3"), "Should expand epic_num and story_num");
    assert!(content.contains("Jinja2 Test"), "Should expand title");
    assert!(content.contains("Done"), "Should expand status");
    assert!(!content.contains("{{epic_num}}"), "Should NOT have unexpanded variables");
}

#[tokio::test]
async fn test_read_handler_hides_conformance_files() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());

    // Create template
    let template = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template.as_bytes()).await.unwrap();

    // Create various conformance files
    fs.write_file("/docs/stories/STORY-004.md.source", b"source").await.unwrap();
    fs.write_file("/docs/stories/STORY-004.md.conformant", b"conformant").await.unwrap();
    fs.write_file("/docs/stories/STORY-004.md.conformant.failed", b"failed").await.unwrap();
    fs.write_file("/docs/stories/STORY-004.md.conformant.stale.123", b"stale").await.unwrap();
    fs.write_file("/docs/stories/README.md", b"readme").await.unwrap();

    let handler = ConformanceReadHandler::new(pool.clone(), fs.clone(), ConformanceConfig::default());

    // Readdir should hide conformance files
    let result = handler.readdir("/docs/stories").await;
    assert!(result.is_ok());
    let entries = result.unwrap().unwrap();

    assert!(entries.contains(&"STORY-004.md".to_string()), "Should show logical file");
    assert!(entries.contains(&"README.md".to_string()), "Should show regular files");
    assert!(entries.contains(&"story-tmpl.yaml".to_string()), "Should show template");

    assert!(!entries.iter().any(|e| e.ends_with(".source")), "Should hide .source");
    assert!(!entries.iter().any(|e| e.ends_with(".conformant")), "Should hide .conformant");
    assert!(!entries.iter().any(|e| e.contains(".conformant.failed")), "Should hide .failed");
    assert!(!entries.iter().any(|e| e.contains(".conformant.stale")), "Should hide .stale");
}

#[tokio::test]
async fn test_getattr_returns_expanded_size() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());

    // Create template
    let template = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template.as_bytes()).await.unwrap();

    // Create .conformant with variables (short on disk)
    let content = "{{title}}"; // 9 bytes
    fs.write_file("/docs/stories/STORY-005.md.conformant", content.as_bytes()).await.unwrap();

    // Insert variable with long value
    {
        let conn = pool.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES (?, ?, ?, ?, ?)",
            params!["story-005-v-title", "story-005", "title", "\"This Is A Very Long Title That Expands The Content\"", "string"]
        ).unwrap();
    }

    let handler = ConformanceReadHandler::new(pool.clone(), fs.clone(), ConformanceConfig::default());

    // Get stats
    let result = handler.getattr("/docs/stories/STORY-005.md").await;
    assert!(result.is_ok());
    let stats = result.unwrap().unwrap();

    // Size should reflect expanded content, not on-disk size
    let expanded_title = "This Is A Very Long Title That Expands The Content";
    assert_eq!(stats.size as usize, expanded_title.len(), "Size should be expanded content length");
}
```

## Risk Assessment

**Primary Risk:** Variable cache becoming stale
**Mitigation:** Cache is per-read; consider TTL for long processes

**Secondary Risk:** Jinja2 expansion changing semantic meaning
**Mitigation:** Only expand known `{{name}}` patterns, leave others unchanged

**Tertiary Risk:** Performance impact of DB lookup on every read
**Mitigation:** Cache per document; DB queries are fast for small result sets

## Definition of Done

- [ ] `ConformanceReadHandler` registered at priority 20
- [ ] Read resolves `.conformant` > `.source` > physical
- [ ] Jinja2 `{{name}}` variables expanded from `gd_variables`
- [ ] Unresolved variables left as placeholders
- [ ] `.source`, `.conformant*` hidden from readdir
- [ ] `getattr` returns expanded content size
- [ ] Raw access via `.source` suffix works
- [ ] Unit tests pass
- [ ] Clippy clean

---

## Dev Agent Record

### Agent Model Used
Claude Opus 4.5 (claude-opus-4-5-20251101)

### Debug Log References
- N/A - Implementation completed without major debugging issues

### Completion Notes
- ConformanceReadHandler implemented at cli/src/handler.rs:1317-1585
- resolve_content and resolve_path methods for priority-based resolution
- expand_jinja2 with variable caching and database query
- apply_variables static method for {{name}} replacement
- readdir and readdir_plus filtering implemented
- Unit tests at cli/src/handler.rs:2696-2716

### File List
| File | Action | Description |
|------|--------|-------------|
| cli/src/handler.rs | Modified | Added ConformanceReadHandler, Jinja2 expansion, readdir filtering |

---

## Change Log

| Date | Change | Reason |
|------|--------|--------|
| 2026-01-17 | Story created | EPIC-FUSE-CONFORMANCE-001 planning |
| 2026-01-18 | Revised for read-time resolution | Non-blocking architecture requires read handler |
| 2026-01-18 | Implementation complete | All tasks and acceptance criteria completed |

---

## QA Results

### Review Date: 2026-01-18

### Reviewed By: Quinn (Test Architect)

### Code Quality Assessment

ConformanceReadHandler is well-implemented at priority 20. Content resolution follows correct priority (.conformant > .source > physical). Jinja2 expansion via `apply_variables()` is simple and correct - replaces `{{name}}` patterns with values from gd_variables. Variable caching prevents repeated DB queries. Readdir filtering correctly hides .source, .conformant, .conformant.failed, and .conformant.stale.* files.

Key implementation locations:
- `ConformanceReadHandler`: `cli/src/handler.rs:1318-1581`
- `resolve_content`: `cli/src/handler.rs:1355-1379`
- `expand_jinja2`: `cli/src/handler.rs:1410-1466`
- `apply_variables`: `cli/src/handler.rs:1469-1478`
- Unit tests: `cli/src/handler.rs:2703-2734`

### Refactoring Performed

None required - implementation is clean.

### Compliance Check

- Coding Standards: ✓ Clean async implementation
- Project Structure: ✓ Properly integrated with FileHandler trait
- Testing Strategy: ✓ Unit tests for apply_variables variants
- All ACs Met: ✓ All 8 acceptance criteria verified

### Improvements Checklist

- [x] ConformanceReadHandler registered at priority 20
- [x] Read checks for .conformant first, falls back to .source
- [x] Jinja2 variables ({{name}}) expanded from gd_variables table
- [x] If variable not found in DB, leave placeholder unchanged
- [x] .source, .conformant, .conformant.* files hidden from readdir
- [x] getattr returns stats with adjusted size for Jinja2 expansion
- [x] Read raw content available via .source suffix (escape hatch)
- [x] Non-template directories pass through unchanged
- [ ] Consider mtime comparison to prefer fresher content (DATA-001)
- [ ] Add TTL to variable cache for long-running mounts (DATA-003)

### Security Review

No security concerns. Read-only operations on user content. Variable values sourced from gd_variables table only.

### Performance Considerations

**TECH-003 (Medium)**: `getattr` reads entire file to calculate expanded size. For large files or frequent stat calls, this may add latency. Consider caching expanded content.

**PERF-002 (Medium)**: Variable cache is per-document with no TTL. For long-running mounts, stale values may persist until next read from different path.

### Files Modified During Review

None - implementation is complete and clean.

### Gate Status

Gate: **PASS** → docs/qa/gates/7.3-read-time-resolution.yml
Risk profile: docs/qa/assessments/7.3-risk-20260117.md

### Recommended Status

✓ Ready for Done - All acceptance criteria met, unit tests pass, apply_variables logic is correct and well-tested.
