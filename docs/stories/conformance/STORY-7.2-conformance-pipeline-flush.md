# STORY-7.2: Background Conformance Process with Database Sync

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-7.2 |
| **Epic** | EPIC-FUSE-CONFORMANCE-001 |
| **Phase** | 7 - FUSE Write-Time Conformance |
| **Status** | Done |
| **Priority** | High |
| **File** | `cli/src/handler.rs` |
| **Dependencies** | STORY-7.1, STORY-2.1.4 (Agent Transformation), STORY-2.1.2 (Variable Detection) |
| **Blocks** | STORY-7.3 |

## User Story

**As a** system processing documents in the background
**I want** to transform documents to template conformance and sync to the database
**So that** the `.conformant` file and database are always in sync with the template structure

## Story Context

**Background:** STORY-7.1 triggers a background conformance task when writes occur. This story implements that background process which:
1. Reads `.source` content
2. Checks conformance against template
3. Transforms if needed via TEA
4. Writes `.conformant` (or `.conformant.failed`)
5. **Syncs to database** (`gd_documents`, `gd_sections`, `gd_variables`)
6. Cleans up stale files

**Architecture Overview:**
```
BACKGROUND TASK:
  → Read .source content
  → Check source mtime (for race detection)
  → Load template, check conformance
  → If non-conformant: transform via TEA
  → Verify source unchanged (mtime check)
  → On success:
      → Write .conformant
      → Sync to gd_documents, gd_sections, gd_variables
      → Delete .conformant.stale.* files
  → On failure:
      → Write .conformant.failed with error details
```

**Existing System Integration:**
- Uses: `sdk/rust/src/graphdocs/agent_transformer.rs` (AgentTransformer)
- Uses: `sdk/rust/src/graphdocs/conformance.rs` (TemplateManager)
- Uses: `sdk/rust/src/graphdocs/variable_types.rs` (type inference)
- Uses: `sdk/rust/src/graphdocs/parser.rs` (MarkdownParser)
- Database: DuckDB with GraphDocs schema

## Acceptance Criteria

- [x] Background task reads `.source` content
- [x] Conformance checked against directory template
- [x] Non-conformant documents transformed via TEA (or rule-based fallback)
- [x] Conformant documents written to `.conformant` unchanged
- [x] On success: `.conformant` file created
- [x] On success: `gd_documents` upserted with title
- [x] On success: `gd_sections` populated with all sections
- [x] On success: `gd_variables` populated with typed values
- [x] On success: `gd_journal` entries created for time-travel
- [x] On success: `.conformant.stale.*` files deleted
- [x] On failure: `.conformant.failed` created with error info
- [x] Race detection: skip if `.source` modified during processing
- [x] TEA errors handled gracefully (rule-based fallback)

## Tasks / Subtasks

- [x] Task 1: Implement run_background_conformance function (AC: 1, 2, 3, 4, 12)
  - [x] Read `.source` content
  - [x] Record source mtime for race detection
  - [x] Load template (YAML or markdown)
  - [x] Check conformance
  - [x] Transform if needed (TEA or rule-based)
  - [x] Verify source unchanged before write

- [x] Task 2: Write .conformant on success (AC: 5, 10)
  - [x] Create `.conformant` file with transformed content
  - [x] Delete `.conformant.stale.*` files after successful write

- [x] Task 3: Write .conformant.failed on failure (AC: 11)
  - [x] Create `.conformant.failed` with:
    - Error type
    - Error message
    - Timestamp
    - Template path
  - [x] Log error for debugging

- [x] Task 4: Database synchronization (AC: 6, 7, 8, 9)
  - [x] Upsert document to `gd_documents`
  - [x] Delete old sections, insert new to `gd_sections`
  - [x] Extract variables from heading+content pairs
  - [x] Insert typed variables to `gd_variables`
  - [x] Record journal entries

- [x] Task 5: Add unit tests
  - [x] Test path_to_doc_id()
  - [x] Test heading_to_var_name()

## Technical Specification

### Background Conformance Function

```rust
async fn run_background_conformance(
    pool: DuckConnectionPool,
    fs: Arc<dyn FileSystem>,
    config: ConformanceConfig,
    logical_path: String,
    template_path: PathBuf,
) -> Result<()> {
    let source_path = format!("{}.source", logical_path);
    let conformant_path = format!("{}.conformant", logical_path);
    let failed_path = format!("{}.conformant.failed", logical_path);

    tracing::info!("Starting background conformance for {}", logical_path);

    // Read source content and mtime
    let content = fs.read_file(&source_path).await?;
    let source_mtime = fs.stat(&source_path).await?.ok_or_else(|| {
        Error::Custom("Source file disappeared".to_string())
    })?.mtime;

    // Run conformance pipeline
    let result = run_conformance_pipeline(
        &content,
        &template_path,
        &config,
    ).await;

    // Check if source was modified during processing
    let current_mtime = fs.stat(&source_path).await?.map(|s| s.mtime);
    if current_mtime != Some(source_mtime) {
        tracing::info!("Source modified during conformance, aborting");
        return Ok(()); // Don't write stale conformant
    }

    match result {
        Ok(transformed) => {
            // Write .conformant file
            fs.write_file(&conformant_path, transformed.as_bytes()).await?;

            // Sync to database
            sync_to_database(&pool, &logical_path, &transformed).await?;

            // Delete stale files
            delete_stale_files(&fs, &conformant_path).await;

            // Delete .failed file if it exists
            let _ = fs.unlink(&failed_path).await;

            tracing::info!("Conformance completed for {}", logical_path);
        }
        Err(e) => {
            // Write .conformant.failed
            let error_content = serde_json::json!({
                "error_type": "conformance_failed",
                "message": e.to_string(),
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "template_path": template_path.display().to_string(),
            });
            fs.write_file(&failed_path, error_content.to_string().as_bytes()).await?;

            tracing::error!("Conformance failed for {}: {}", logical_path, e);
        }
    }

    Ok(())
}
```

### Conformance Pipeline

```rust
async fn run_conformance_pipeline(
    content: &str,
    template_path: &Path,
    config: &ConformanceConfig,
) -> Result<String> {
    use agentfs_sdk::graphdocs::agent_transformer::{AgentTransformer, ConformanceResult};
    use agentfs_sdk::graphdocs::template_schema::BmadTemplate;
    use agentfs_sdk::graphdocs::parser::MarkdownParser;

    // Parse document
    let parser = MarkdownParser::new();
    let doc = parser.parse(content)?;

    // Load template
    let is_yaml = TemplateManager::is_yaml_template(template_path);
    let (template_doc, bmad_template) = if is_yaml {
        let tmpl_content = std::fs::read_to_string(template_path)?;
        let bmad = BmadTemplate::from_yaml(&tmpl_content)?;
        (bmad.to_parsed_document(), Some(bmad))
    } else {
        let tmpl_content = std::fs::read_to_string(template_path)?;
        (parser.parse(&tmpl_content)?, None)
    };

    // Check conformance
    let mut manager = TemplateManager::new();
    let (missing_sections, type_mismatches, is_conformant) = if let Some(ref bmad) = bmad_template {
        manager.load_bmad_template_sync(template_path)?;
        let result = manager.check_bmad_conformance(&doc, bmad, template_path);
        (
            result.missing_sections.iter().map(|s| s.section_title.clone()).collect(),
            result.type_violations.iter().map(|v| v.section_title.clone()).collect(),
            result.is_conformant,
        )
    } else {
        manager.load_markdown_template_sync(template_path)?;
        let result = manager.check_markdown_conformance(&doc, &template_doc, template_path);
        (result.missing_sections, vec![], result.is_conformant)
    };

    // If conformant, return original
    if is_conformant {
        return Ok(content.to_string());
    }

    // Transform via TEA or rule-based
    let mut transformer = AgentTransformer::new(config.agents_dir.clone());
    if let Some(ref overlay) = config.overlay {
        transformer = transformer.with_overlay(overlay.clone());
    }

    let conformance_result = ConformanceResult {
        file_path: String::new(),
        template_path: Some(template_path.display().to_string()),
        is_conformant: false,
        missing_sections,
        extra_sections: vec![],
        type_mismatches,
        suggestions: vec![],
    };

    // Try TEA, fallback to rule-based
    if transformer.check_tea_available().await.unwrap_or(false) {
        match transformer.transform_to_template(&doc, &template_doc, &conformance_result).await {
            Ok(content) => Ok(content),
            Err(e) => {
                tracing::warn!("TEA failed, using rule-based: {}", e);
                transformer.transform_rule_based(&doc, &template_doc, &conformance_result)
            }
        }
    } else {
        transformer.transform_rule_based(&doc, &template_doc, &conformance_result)
    }
}
```

### Database Synchronization

```rust
async fn sync_to_database(
    pool: &DuckConnectionPool,
    logical_path: &str,
    content: &str,
) -> Result<()> {
    let pool = pool.clone();
    let path = logical_path.to_string();
    let content = content.to_string();

    tokio::task::spawn_blocking(move || {
        let conn = pool.get_write_connection()?;
        let doc_id = path_to_doc_id(&path);

        tracing::debug!("Syncing {} (id: {}) to database", path, doc_id);

        // Parse document
        let parser = MarkdownParser::new();
        let doc = parser.parse(&content)?;

        // Extract title
        let title = doc.title.clone()
            .unwrap_or_else(|| doc_id.to_uppercase().replace('-', " "));

        // === UPSERT DOCUMENT ===
        conn.execute(
            r#"
            INSERT INTO gd_documents (id, title, updated_at)
            VALUES (?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT (id) DO UPDATE SET
                title = EXCLUDED.title,
                updated_at = CURRENT_TIMESTAMP
            "#,
            params![doc_id, title],
        )?;

        // Journal entry
        let doc_data = serde_json::json!({ "id": doc_id, "title": title });
        conn.execute(
            r#"INSERT INTO gd_journal (event_type, table_name, record_id, new_data)
               VALUES ('upsert', 'gd_documents', ?, ?)"#,
            params![doc_id, doc_data.to_string()],
        )?;

        // === DELETE OLD SECTIONS ===
        conn.execute("DELETE FROM gd_sections WHERE document_id = ?", params![doc_id])?;

        // === INSERT SECTIONS ===
        for (idx, section) in doc.sections.iter().enumerate() {
            let section_id = format!("{}-s{}", doc_id, idx);
            conn.execute(
                r#"
                INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content)
                VALUES (?, ?, ?, ?, ?, ?)
                "#,
                params![section_id, doc_id, section.section_type.as_str(),
                        section.level.map(|l| l as i32), section.order_idx, section.content],
            )?;

            // Journal
            let section_data = serde_json::json!({
                "id": section_id, "document_id": doc_id,
                "section_type": section.section_type.as_str(),
                "order_idx": section.order_idx
            });
            conn.execute(
                r#"INSERT INTO gd_journal (event_type, table_name, record_id, new_data)
                   VALUES ('create', 'gd_sections', ?, ?)"#,
                params![section_id, section_data.to_string()],
            )?;
        }

        // === DELETE OLD VARIABLES ===
        conn.execute("DELETE FROM gd_variables WHERE document_id = ?", params![doc_id])?;

        // === EXTRACT AND INSERT VARIABLES ===
        // From {{name}} placeholders
        for (name, value) in &doc.variables {
            insert_variable_with_inference(&conn, &doc_id, name, value)?;
        }

        // From heading+content pairs
        let mut current_heading: Option<&str> = None;
        for section in &doc.sections {
            if section.section_type == SectionType::Heading {
                current_heading = Some(&section.content);
            } else if let Some(heading) = current_heading {
                let var_name = heading_to_var_name(heading);
                if !var_name.is_empty() {
                    insert_variable_with_inference(&conn, &doc_id, &var_name, section.content.trim())?;
                }
                current_heading = None;
            }
        }

        tracing::info!("Synced {} sections and variables for {}", doc.sections.len(), doc_id);
        Ok(())
    })
    .await
    .map_err(|e| Error::Custom(format!("spawn_blocking error: {}", e)))?
}

fn insert_variable_with_inference(
    conn: &MutexGuard<'_, Connection>,
    doc_id: &str,
    name: &str,
    value: &str,
) -> Result<()> {
    use agentfs_sdk::graphdocs::variable_types::{infer_variable_type, VariableType};

    let parsed = infer_variable_type(name);
    let (json_value, var_type) = match parsed.var_type {
        VariableType::Bool => {
            let v = value.to_lowercase();
            let b = v == "true" || v == "yes" || v == "1" || v == "done";
            (serde_json::Value::Bool(b), "bool")
        }
        VariableType::Number => {
            match value.parse::<f64>() {
                Ok(n) => (serde_json::json!(n), "number"),
                Err(_) => (serde_json::Value::String(value.to_string()), "string"),
            }
        }
        VariableType::Enum => (serde_json::Value::String(value.to_string()), "enum"),
        VariableType::StringArray => {
            let items: Vec<&str> = value.lines()
                .filter_map(|line| {
                    let t = line.trim();
                    if t.starts_with("- ") || t.starts_with("* ") {
                        Some(t[2..].trim())
                    } else {
                        None
                    }
                })
                .collect();
            if items.is_empty() {
                (serde_json::Value::String(value.to_string()), "string")
            } else {
                (serde_json::json!(items), "string[]")
            }
        }
        _ => (serde_json::Value::String(value.to_string()), "string"),
    };

    let var_id = format!("{}-v-{}", doc_id, name);
    conn.execute(
        r#"
        INSERT INTO gd_variables (id, document_id, name, value, var_type)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT (id) DO UPDATE SET
            value = EXCLUDED.value, var_type = EXCLUDED.var_type, updated_at = CURRENT_TIMESTAMP
        "#,
        params![var_id, doc_id, name, json_value.to_string(), var_type],
    )?;

    // Journal
    let var_data = serde_json::json!({
        "id": var_id, "document_id": doc_id, "name": name,
        "value": json_value, "var_type": var_type
    });
    conn.execute(
        r#"INSERT INTO gd_journal (event_type, table_name, record_id, new_data)
           VALUES ('upsert', 'gd_variables', ?, ?)"#,
        params![var_id, var_data.to_string()],
    )?;

    Ok(())
}
```

### Stale File Cleanup

```rust
async fn delete_stale_files(fs: &Arc<dyn FileSystem>, conformant_path: &str) {
    // Find all .conformant.stale.* files
    let parent = Path::new(conformant_path).parent().unwrap_or(Path::new("/"));
    let filename = Path::new(conformant_path).file_name().unwrap().to_str().unwrap();

    if let Ok(entries) = fs.readdir(parent.to_str().unwrap()).await {
        for entry in entries.iter().flatten() {
            if entry.starts_with(&format!("{}.stale.", filename)) {
                let stale_path = format!("{}/{}", parent.display(), entry);
                if let Err(e) = fs.unlink(&stale_path).await {
                    tracing::warn!("Failed to delete stale file {}: {}", stale_path, e);
                } else {
                    tracing::debug!("Deleted stale file: {}", stale_path);
                }
            }
        }
    }
}
```

## Dev Notes

### Database Sync Timing

The database sync happens AFTER `.conformant` is written successfully. This ensures:
1. File is available for reads immediately
2. Database reflects the persisted content
3. If DB sync fails, file is still valid (can retry sync)

### Variable Type Inference

Uses existing `variable_types.rs` module which recognizes patterns:
- `is_*`, `has_*`, `*_enabled` → Bool
- `*_count`, `*_num`, `*_size` → Number
- `status`, `priority`, `state` → Enum
- `tags`, `items`, `*_list` → StringArray

### Error File Format

`.conformant.failed` contains JSON:
```json
{
  "error_type": "conformance_failed",
  "message": "TEA subprocess timed out after 30s",
  "timestamp": "2026-01-18T10:30:00Z",
  "template_path": "/docs/templates/story-tmpl.yaml"
}
```

### Source Tree Reference

```
cli/src/
└── handler.rs        # run_background_conformance, sync_to_database

sdk/rust/src/graphdocs/
├── variable_types.rs # infer_variable_type()
├── parser.rs         # MarkdownParser
└── agent_transformer.rs # TEA transformation
```

## Integration Tests

### Test Document: Non-Conformant Story

Use a real story format that needs conformance transformation:

```markdown
# STORY-TEST-001: Integration Test Story

## Status
InProgress

## Story
**As a** developer,
**I want** to test conformance,
**so that** documents are properly transformed

## Acceptance Criteria
1. Document is detected as non-conformant
2. TEA transforms the document
3. Database is updated with all fields

## Tasks
- [ ] Task 1: Implement feature
- [ ] Task 2: Add tests

## Notes
This section is NOT in the template - should be removed or transformed.
```

### Template: story-tmpl.yaml

Use the actual template from `.bmad-core/templates/story-tmpl.yaml`:
- Status: choice type with [Draft, Approved, InProgress, Review, Done]
- Story: template-text with As a/I want/So that format
- Acceptance Criteria: numbered-list
- Tasks / Subtasks: bullet-list
- Dev Notes: paragraphs
- Change Log: table

### Expected Conformance Result

After conformance transformation:
1. "Notes" section removed (not in template)
2. All template sections present with correct types
3. Missing sections added with empty content

### Expected Database State

**gd_documents:**
```sql
SELECT id, title FROM gd_documents WHERE id = 'story-test-001';
-- id: 'story-test-001'
-- title: 'Integration Test Story'
```

**gd_variables:**
```sql
SELECT name, value, var_type FROM gd_variables
WHERE document_id = 'story-test-001';
-- name: 'status', value: '"InProgress"', var_type: 'enum'
-- name: 'role', value: '"developer"', var_type: 'string'
-- name: 'action', value: '"to test conformance"', var_type: 'string'
-- name: 'benefit', value: '"documents are properly transformed"', var_type: 'string'
```

**gd_sections:**
```sql
SELECT section_type, level, order_idx FROM gd_sections
WHERE document_id = 'story-test-001' ORDER BY order_idx;
-- section_type: 'heading', level: 1, order_idx: 0  (title)
-- section_type: 'heading', level: 2, order_idx: 1  (Status)
-- section_type: 'paragraph', level: null, order_idx: 2  (InProgress)
-- ... etc for each section
```

### Integration Test Code

```rust
#[tokio::test]
async fn test_background_conformance_with_real_story() {
    // Setup DuckDB with GraphDocs schema
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());

    // Create template-controlled directory
    let template_content = include_str!("../../../.bmad-core/templates/story-tmpl.yaml");
    fs.write_file("/docs/stories/story-tmpl.yaml", template_content.as_bytes()).await.unwrap();

    // Write non-conformant story to .source
    let story_content = r#"# STORY-TEST-001: Integration Test Story

## Status
InProgress

## Story
**As a** developer,
**I want** to test conformance,
**so that** documents are properly transformed

## Acceptance Criteria
1. Document is detected as non-conformant
2. TEA transforms the document
3. Database is updated with all fields

## Tasks
- [ ] Task 1: Implement feature
- [ ] Task 2: Add tests

## Extra Section Not In Template
This should be handled by conformance.
"#;
    fs.write_file("/docs/stories/STORY-TEST-001.md.source", story_content.as_bytes()).await.unwrap();

    // Run background conformance
    let config = ConformanceConfig {
        agents_dir: PathBuf::from("agents"),
        overlay: None,
        model_path: None,
        timeout_secs: 30,
    };

    run_background_conformance(
        pool.clone(),
        fs.clone(),
        config,
        "/docs/stories/STORY-TEST-001.md".to_string(),
        PathBuf::from("/docs/stories/story-tmpl.yaml"),
    ).await.expect("Conformance should succeed");

    // Verify .conformant file exists
    let conformant = fs.read_file("/docs/stories/STORY-TEST-001.md.conformant").await;
    assert!(conformant.is_ok(), "Should create .conformant file");

    // Verify database sync
    let conn = pool.get_read_connection().unwrap();

    // Check document
    let doc_count: i32 = conn.query_row(
        "SELECT COUNT(*) FROM gd_documents WHERE id = 'story-test-001'",
        [],
        |row| row.get(0)
    ).unwrap();
    assert_eq!(doc_count, 1, "Document should be inserted");

    // Check variables
    let status: String = conn.query_row(
        "SELECT value FROM gd_variables WHERE document_id = 'story-test-001' AND name = 'status'",
        [],
        |row| row.get(0)
    ).unwrap();
    assert_eq!(status, "\"InProgress\"", "Status should be stored as enum");

    // Check sections
    let section_count: i32 = conn.query_row(
        "SELECT COUNT(*) FROM gd_sections WHERE document_id = 'story-test-001'",
        [],
        |row| row.get(0)
    ).unwrap();
    assert!(section_count >= 5, "Should have at least 5 sections");

    // Check journal entries
    let journal_count: i32 = conn.query_row(
        "SELECT COUNT(*) FROM gd_journal WHERE record_id LIKE 'story-test-001%'",
        [],
        |row| row.get(0)
    ).unwrap();
    assert!(journal_count >= 1, "Should have journal entries for time-travel");
}

#[tokio::test]
async fn test_conformance_failure_creates_failed_file() {
    let pool = create_test_pool().await;
    let fs = Arc::new(MemoryFileSystem::new());

    // Write invalid story content
    fs.write_file("/docs/stories/STORY-BAD.md.source", b"Not valid markdown structure").await.unwrap();

    // No template file - will fail
    let config = ConformanceConfig::default();

    let result = run_background_conformance(
        pool.clone(),
        fs.clone(),
        config,
        "/docs/stories/STORY-BAD.md".to_string(),
        PathBuf::from("/docs/stories/nonexistent-tmpl.yaml"),
    ).await;

    // Should create .conformant.failed
    let failed = fs.read_file("/docs/stories/STORY-BAD.md.conformant.failed").await;
    assert!(failed.is_ok(), "Should create .conformant.failed file");

    let failed_content = String::from_utf8(failed.unwrap()).unwrap();
    let error: serde_json::Value = serde_json::from_str(&failed_content).unwrap();
    assert_eq!(error["error_type"], "conformance_failed");
}
```

## Risk Assessment

**Primary Risk:** Database sync failure leaves data inconsistent
**Mitigation:** File is written first; DB sync can be retried on next edit

**Secondary Risk:** Long-running TEA blocks other conformance tasks
**Mitigation:** Timeout (30s default), task isolation per file

**Tertiary Risk:** Race condition detection too aggressive
**Mitigation:** Only check mtime, not content hash (faster)

## Definition of Done

- [ ] Background conformance task implemented
- [ ] `.conformant` created on success
- [ ] `.conformant.failed` created on failure
- [ ] Database sync: `gd_documents`, `gd_sections`, `gd_variables`
- [ ] Journal entries for time-travel
- [ ] Stale files cleaned up on success
- [ ] Race detection prevents stale writes
- [ ] Unit tests pass
- [ ] Clippy clean

---

## Dev Agent Record

### Agent Model Used
Claude Opus 4.5 (claude-opus-4-5-20251101)

### Debug Log References
- N/A - Implementation completed without major debugging issues

### Completion Notes
- run_background_conformance implemented at cli/src/handler.rs:1593-1677
- run_conformance_pipeline implemented at cli/src/handler.rs:1680-1779
- sync_to_database implemented at cli/src/handler.rs:1782-1889
- Helper functions (path_to_doc_id, heading_to_var_name, insert_variable, delete_stale_files) at cli/src/handler.rs:1892-1957
- Unit tests at cli/src/handler.rs:2683-2693

### File List
| File | Action | Description |
|------|--------|-------------|
| cli/src/handler.rs | Modified | Added run_background_conformance, run_conformance_pipeline, sync_to_database, helper functions |

---

## Change Log

| Date | Change | Reason |
|------|--------|--------|
| 2026-01-17 | Story created | EPIC-FUSE-CONFORMANCE-001 planning |
| 2026-01-18 | Revised for non-blocking + DB sync | User feedback - async conformance with DB maintenance |
| 2026-01-18 | Implementation complete | All tasks and acceptance criteria completed |

---

## QA Results

### Review Date: 2026-01-18

### Reviewed By: Quinn (Test Architect)

### Code Quality Assessment

Background conformance pipeline is well-implemented with clear separation of concerns: `run_background_conformance` orchestrates the flow, `run_conformance_pipeline` handles TEA/rule-based transformation, and `sync_to_database` manages gd_documents/gd_sections/gd_variables sync. Race detection via mtime comparison prevents stale writes. Error handling creates `.conformant.failed` with structured JSON.

Key implementation locations:
- `run_background_conformance`: `cli/src/handler.rs:1597-1673`
- `run_conformance_pipeline`: `cli/src/handler.rs:1676-1779`
- `sync_to_database`: `cli/src/handler.rs:1782-1889`
- Helper functions: `cli/src/handler.rs:1891-1957`

### Refactoring Performed

None required - implementation is clean.

### Compliance Check

- Coding Standards: ✓ Proper async/spawn_blocking for DB operations
- Project Structure: ✓ Uses existing graphdocs modules (parser, template_schema, agent_transformer)
- Testing Strategy: ✓ Unit tests for path_to_doc_id, heading_to_var_name
- All ACs Met: ✓ All 12 acceptance criteria verified

### Improvements Checklist

- [x] Background task reads .source content
- [x] Conformance checked against directory template
- [x] Non-conformant documents transformed via TEA (or rule-based fallback)
- [x] Conformant documents returned unchanged
- [x] On success: .conformant file created
- [x] On success: gd_documents upserted
- [x] On success: gd_sections populated
- [x] On success: gd_variables populated
- [x] On success: gd_journal entries created
- [x] On success: .conformant.stale.* files deleted
- [x] On failure: .conformant.failed created with error info
- [x] Race detection: skip if .source modified during processing
- [ ] Add retry logic for transient DB failures (DATA-001)
- [ ] Consider accumulating multi-paragraph content under headings (DATA-002)

### Security Review

No security concerns. Content transformation operates on user-controlled markdown files. Database operations are parameterized (no SQL injection risk).

### Performance Considerations

**Background Processing**: Non-blocking architecture means user writes return immediately - major UX improvement over original blocking design.

**DB Sync Latency**: Database operations run in spawn_blocking to avoid blocking Tokio runtime. Eventual consistency model is acceptable.

**Journal Growth (PERF-002)**: Consider implementing journal compaction for long-running databases.

### Files Modified During Review

None - implementation is complete and clean.

### Gate Status

Gate: **PASS** → docs/qa/gates/7.2-conformance-pipeline-flush.yml
Risk profile: docs/qa/assessments/7.2-risk-20260117.md

### Recommended Status

✓ Ready for Done - All acceptance criteria met, implementation is clean. Critical blocking UX issue from original design is fully resolved by non-blocking architecture.
