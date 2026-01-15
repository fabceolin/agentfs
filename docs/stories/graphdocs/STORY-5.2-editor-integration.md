# STORY-5.2: Editor Integration

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-5.2 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 5 - CLI and Management |
| **Status** | Ready for Development |
| **Priority** | Low |
| **File** | `cli/src/cmd/graphdocs.rs` |
| **Dependencies** | STORY-5.1 |

## User Story

**As an** author
**I want** to edit graphs via a friendly interface
**So that** I don't need to write SQL

## Acceptance Criteria

- [ ] Open text editor with YAML/TOML of document
- [ ] Parse and update graph on save
- [ ] Validate structure

## Technical Specification

### Edit Command

```rust
// cli/src/cmd/graphdocs.rs

#[derive(Args)]
pub struct EditArgs {
    /// Document ID to edit
    pub doc_id: String,

    /// Editor to use (defaults to $EDITOR or vim)
    #[clap(long, short)]
    pub editor: Option<String>,

    /// Output format: yaml or toml
    #[clap(long, default_value = "yaml")]
    pub format: EditFormat,
}

#[derive(clap::ValueEnum, Clone)]
pub enum EditFormat {
    Yaml,
    Toml,
}
```

### Document Serialization Format

```yaml
# Example YAML representation of a GraphDoc

document:
  id: my-readme
  title: My Project README
  description: Documentation for My Project
  base_template: readme-template
  language: en

sections:
  - id: s1
    type: heading
    level: 1
    order: 0
    content: "# {{project_name}}"

  - id: s2
    type: paragraph
    order: 1
    content: "{{description}}"

  - id: s3
    type: heading
    level: 2
    order: 2
    content: "## Installation"

  - id: s4
    type: code
    order: 3
    content: |
      ```bash
      {{install_cmd}}
      ```

variables:
  - name: project_name
    value: "My Project"
    type: string

  - name: description
    value: "A great project"
    type: string

  - name: install_cmd
    value: "npm install my-project"
    type: string

  - name: features
    value:
      - Authentication
      - API
      - Dashboard
    type: array
```

### Edit Handler Implementation

```rust
// cli/src/cmd/graphdocs.rs

use std::io::Write;
use tempfile::NamedTempFile;

/// Serializable document structure
#[derive(Debug, Serialize, Deserialize)]
struct EditableDocument {
    document: DocumentMeta,
    sections: Vec<EditableSection>,
    variables: Vec<EditableVariable>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DocumentMeta {
    id: String,
    title: String,
    description: Option<String>,
    base_template: Option<String>,
    language: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct EditableSection {
    id: String,
    #[serde(rename = "type")]
    section_type: String,
    level: Option<u8>,
    order: i32,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    override_section: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EditableVariable {
    name: String,
    value: serde_json::Value,
    #[serde(rename = "type")]
    var_type: String,
}

pub async fn handle_edit(fs: &DuckAgentFS, args: EditArgs) -> Result<()> {
    let conn = fs.pool.get_read_connection().await?;

    // Load document
    let doc = load_document_for_edit(&conn, &args.doc_id).await?;

    // Serialize to YAML/TOML
    let content = match args.format {
        EditFormat::Yaml => serde_yaml::to_string(&doc)?,
        EditFormat::Toml => toml::to_string_pretty(&doc)?,
    };

    // Write to temp file
    let extension = match args.format {
        EditFormat::Yaml => "yaml",
        EditFormat::Toml => "toml",
    };
    let mut temp_file = NamedTempFile::with_suffix(&format!(".{}", extension))?;
    temp_file.write_all(content.as_bytes())?;
    let temp_path = temp_file.path().to_owned();

    // Get editor
    let editor = args.editor
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vim".to_string());

    // Open editor
    let status = std::process::Command::new(&editor)
        .arg(&temp_path)
        .status()?;

    if !status.success() {
        return Err(anyhow::anyhow!("Editor exited with error"));
    }

    // Read modified content
    let modified_content = std::fs::read_to_string(&temp_path)?;

    // Parse
    let modified_doc: EditableDocument = match args.format {
        EditFormat::Yaml => serde_yaml::from_str(&modified_content)?,
        EditFormat::Toml => toml::from_str(&modified_content)?,
    };

    // Validate
    validate_document(&modified_doc)?;

    // Diff and confirm
    let changes = diff_documents(&doc, &modified_doc);
    if changes.is_empty() {
        println!("No changes made.");
        return Ok(());
    }

    println!("Changes detected:");
    for change in &changes {
        println!("  {}", change);
    }

    print!("Apply changes? [y/N] ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" {
        println!("Cancelled.");
        return Ok(());
    }

    // Apply changes
    apply_changes(fs, &args.doc_id, &doc, &modified_doc).await?;

    println!("Changes applied successfully.");

    Ok(())
}

async fn load_document_for_edit(
    conn: &DuckConnection,
    doc_id: &str,
) -> Result<EditableDocument> {
    // Load document metadata
    let (title, description, base_template, language): (String, Option<String>, Option<String>, String) =
        conn.query_row(
            "SELECT title, description, base_template, language FROM gd_documents WHERE id = ?",
            [doc_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(|_| anyhow::anyhow!("Document '{}' not found", doc_id))?;

    // Load sections
    let mut stmt = conn.prepare(r#"
        SELECT id, section_type, level, order_idx, content, source_section
        FROM gd_sections
        WHERE document_id = ?
        ORDER BY order_idx
    "#)?;

    let sections: Vec<EditableSection> = stmt.query_map([doc_id], |row| {
        Ok(EditableSection {
            id: row.get(0)?,
            section_type: row.get(1)?,
            level: row.get(2)?,
            order: row.get(3)?,
            content: row.get(4)?,
            override_section: row.get(5)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    // Load variables
    let mut stmt = conn.prepare(
        "SELECT name, value, var_type FROM gd_variables WHERE document_id = ?"
    )?;

    let variables: Vec<EditableVariable> = stmt.query_map([doc_id], |row| {
        let value_str: String = row.get(1)?;
        let value: serde_json::Value = serde_json::from_str(&value_str)
            .unwrap_or(serde_json::Value::String(value_str));
        Ok(EditableVariable {
            name: row.get(0)?,
            value,
            var_type: row.get(2)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(EditableDocument {
        document: DocumentMeta {
            id: doc_id.to_string(),
            title,
            description,
            base_template,
            language,
        },
        sections,
        variables,
    })
}

fn validate_document(doc: &EditableDocument) -> Result<()> {
    // Validate section types
    let valid_types = ["heading", "paragraph", "list", "code", "blockquote", "hr", "table"];
    for section in &doc.sections {
        if !valid_types.contains(&section.section_type.as_str()) {
            return Err(anyhow::anyhow!(
                "Invalid section type '{}' in section {}",
                section.section_type, section.id
            ));
        }

        // Validate heading level
        if section.section_type == "heading" {
            if let Some(level) = section.level {
                if level < 1 || level > 6 {
                    return Err(anyhow::anyhow!(
                        "Invalid heading level {} in section {}",
                        level, section.id
                    ));
                }
            }
        }
    }

    // Validate variable types
    let valid_var_types = ["string", "number", "boolean", "array", "object"];
    for var in &doc.variables {
        if !valid_var_types.contains(&var.var_type.as_str()) {
            return Err(anyhow::anyhow!(
                "Invalid variable type '{}' for variable '{}'",
                var.var_type, var.name
            ));
        }
    }

    // Validate unique section IDs
    let mut seen_ids = std::collections::HashSet::new();
    for section in &doc.sections {
        if !seen_ids.insert(&section.id) {
            return Err(anyhow::anyhow!("Duplicate section ID: {}", section.id));
        }
    }

    // Validate unique variable names
    let mut seen_names = std::collections::HashSet::new();
    for var in &doc.variables {
        if !seen_names.insert(&var.name) {
            return Err(anyhow::anyhow!("Duplicate variable name: {}", var.name));
        }
    }

    Ok(())
}

fn diff_documents(old: &EditableDocument, new: &EditableDocument) -> Vec<String> {
    let mut changes = Vec::new();

    // Document metadata changes
    if old.document.title != new.document.title {
        changes.push(format!("Title: '{}' -> '{}'", old.document.title, new.document.title));
    }
    if old.document.description != new.document.description {
        changes.push("Description changed".to_string());
    }
    if old.document.base_template != new.document.base_template {
        changes.push(format!("Template: {:?} -> {:?}",
            old.document.base_template, new.document.base_template));
    }

    // Section changes
    let old_ids: std::collections::HashSet<_> = old.sections.iter().map(|s| &s.id).collect();
    let new_ids: std::collections::HashSet<_> = new.sections.iter().map(|s| &s.id).collect();

    for id in new_ids.difference(&old_ids) {
        changes.push(format!("Add section: {}", id));
    }
    for id in old_ids.difference(&new_ids) {
        changes.push(format!("Remove section: {}", id));
    }
    for id in old_ids.intersection(&new_ids) {
        let old_sec = old.sections.iter().find(|s| &s.id == *id).unwrap();
        let new_sec = new.sections.iter().find(|s| &s.id == *id).unwrap();
        if old_sec.content != new_sec.content {
            changes.push(format!("Modify section: {}", id));
        }
    }

    // Variable changes
    let old_vars: std::collections::HashSet<_> = old.variables.iter().map(|v| &v.name).collect();
    let new_vars: std::collections::HashSet<_> = new.variables.iter().map(|v| &v.name).collect();

    for name in new_vars.difference(&old_vars) {
        changes.push(format!("Add variable: {}", name));
    }
    for name in old_vars.difference(&new_vars) {
        changes.push(format!("Remove variable: {}", name));
    }
    for name in old_vars.intersection(&new_vars) {
        let old_var = old.variables.iter().find(|v| &v.name == *name).unwrap();
        let new_var = new.variables.iter().find(|v| &v.name == *name).unwrap();
        if old_var.value != new_var.value {
            changes.push(format!("Modify variable: {}", name));
        }
    }

    changes
}

async fn apply_changes(
    fs: &DuckAgentFS,
    doc_id: &str,
    old: &EditableDocument,
    new: &EditableDocument,
) -> Result<()> {
    let conn = fs.pool.get_write_connection().await?;

    // Update document metadata
    conn.execute(
        r#"UPDATE gd_documents
           SET title = ?, description = ?, base_template = ?, language = ?, updated_at = CURRENT_TIMESTAMP
           WHERE id = ?"#,
        params![
            new.document.title,
            new.document.description,
            new.document.base_template,
            new.document.language,
            doc_id
        ],
    )?;

    // Handle sections
    let old_ids: std::collections::HashSet<_> = old.sections.iter().map(|s| &s.id).collect();
    let new_ids: std::collections::HashSet<_> = new.sections.iter().map(|s| &s.id).collect();

    // Delete removed sections
    for id in old_ids.difference(&new_ids) {
        conn.execute("DELETE FROM gd_sections WHERE id = ?", [*id])?;
    }

    // Add/update sections
    for section in &new.sections {
        if old_ids.contains(&section.id) {
            // Update
            conn.execute(
                r#"UPDATE gd_sections
                   SET section_type = ?, level = ?, order_idx = ?, content = ?, source_section = ?, updated_at = CURRENT_TIMESTAMP
                   WHERE id = ?"#,
                params![
                    section.section_type,
                    section.level,
                    section.order,
                    section.content,
                    section.override_section,
                    section.id
                ],
            )?;
        } else {
            // Insert
            conn.execute(
                r#"INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content, source_section)
                   VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                params![
                    section.id,
                    doc_id,
                    section.section_type,
                    section.level,
                    section.order,
                    section.content,
                    section.override_section
                ],
            )?;
        }
    }

    // Handle variables
    let old_names: std::collections::HashSet<_> = old.variables.iter().map(|v| &v.name).collect();
    let new_names: std::collections::HashSet<_> = new.variables.iter().map(|v| &v.name).collect();

    // Delete removed variables
    for name in old_names.difference(&new_names) {
        conn.execute(
            "DELETE FROM gd_variables WHERE document_id = ? AND name = ?",
            params![doc_id, *name],
        )?;
    }

    // Add/update variables
    for var in &new.variables {
        let value_str = serde_json::to_string(&var.value)?;

        if old_names.contains(&var.name) {
            conn.execute(
                r#"UPDATE gd_variables
                   SET value = ?, var_type = ?, updated_at = CURRENT_TIMESTAMP
                   WHERE document_id = ? AND name = ?"#,
                params![value_str, var.var_type, doc_id, var.name],
            )?;
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                r#"INSERT INTO gd_variables (id, document_id, name, value, var_type)
                   VALUES (?, ?, ?, ?, ?)"#,
                params![id, doc_id, var.name, value_str, var.var_type],
            )?;
        }
    }

    Ok(())
}
```

### CLI Usage

```bash
# Edit document in default editor (vim)
agentfs graphdocs edit my-readme

# Edit with specific editor
agentfs graphdocs edit my-readme --editor code

# Edit in TOML format
agentfs graphdocs edit my-readme --format toml
```

### Workflow Example

```
$ agentfs graphdocs edit my-readme

# Editor opens with:
# ---
# document:
#   id: my-readme
#   title: My Project README
#   ...
# sections:
#   - id: s1
#     type: heading
#     ...
# variables:
#   - name: project_name
#     value: "My Project"
#     ...

# After editing and saving...

Changes detected:
  Modify section: s2
  Add variable: author
  Modify variable: version

Apply changes? [y/N] y
Changes applied successfully.
```

## Tests

### Test 1: Load Document for Edit
```rust
#[tokio::test]
async fn test_load_document() {
    let fs = setup_fs_with_doc("test", "Test", "# Hello").await;
    let conn = fs.pool.get_read_connection().await.unwrap();

    let doc = load_document_for_edit(&conn, "test").await.unwrap();

    assert_eq!(doc.document.id, "test");
    assert_eq!(doc.document.title, "Test");
    assert!(!doc.sections.is_empty());
}
```

### Test 2: Validate Document
```rust
#[test]
fn test_validate_invalid_section_type() {
    let doc = EditableDocument {
        document: DocumentMeta {
            id: "test".into(),
            title: "Test".into(),
            description: None,
            base_template: None,
            language: "en".into(),
        },
        sections: vec![EditableSection {
            id: "s1".into(),
            section_type: "invalid".into(),
            level: None,
            order: 0,
            content: "test".into(),
            override_section: None,
        }],
        variables: vec![],
    };

    assert!(validate_document(&doc).is_err());
}
```

### Test 3: Diff Documents
```rust
#[test]
fn test_diff_documents() {
    let old = create_test_document("1.0");
    let new = create_test_document("2.0");

    let changes = diff_documents(&old, &new);

    assert!(!changes.is_empty());
    assert!(changes.iter().any(|c| c.contains("version")));
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/cmd/graphdocs.rs` | Edit command |
| `Cargo.toml` | Dependencies (serde_yaml, toml, tempfile) |

## Dependencies

```toml
[dependencies]
serde_yaml = "0.9"
toml = "0.8"
tempfile = "3"
```

## Implementation Notes

1. **Editor Detection**: Uses $EDITOR environment variable or falls back to vim
2. **Atomic Updates**: All changes are applied in a single transaction
3. **Validation**: Strict validation prevents invalid data
4. **Diff Preview**: Shows changes before applying for safety
5. **Cancellation**: User can cancel at the confirmation prompt
