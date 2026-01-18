//! GraphDocs CLI commands for managing graph-based documents
//!
//! This module provides CLI handlers for:
//! - `agentfs graphdocs list` - List all GraphDocs documents
//! - `agentfs graphdocs create <doc_id> --title "Title"` - Create a new document
//! - `agentfs graphdocs show <doc_id>` - Show document details
//! - `agentfs graphdocs delete <doc_id>` - Delete a document
//! - `agentfs graphdocs add-section <doc_id> --type heading --content "..."` - Add a section
//! - `agentfs graphdocs remove-section <doc_id> <section_id>` - Remove a section
//! - `agentfs graphdocs set-var <doc_id> <name> <value>` - Set a variable
//! - `agentfs graphdocs get-var <doc_id> <name>` - Get a variable value
//! - `agentfs graphdocs list-vars <doc_id>` - List variables
//! - `agentfs graphdocs render <doc_id>` - Render document to Markdown
//! - `agentfs graphdocs import <file.md>` - Import a single Markdown file
//! - `agentfs graphdocs import-dir <dir>` - Import all Markdown files from a directory
//! - `agentfs graphdocs export <doc_id>` - Export a GraphDoc to Markdown
//! - `agentfs graphdocs history <doc_id>` - Show document history
//! - `agentfs graphdocs conform <dir>` - Check and fix document conformance using TEA

use agentfs_sdk::filesystem::duckagentfs::{DuckAgentFS, DuckAgentFSConfig};
use agentfs_sdk::graphdocs::{
    batch_transform, ConformArgs as SdkConformArgs, GraphDocsEngine, LLMConverter, MarkdownParser,
    OpenAIClient,
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

/// GraphDocs subcommands
#[derive(Args, Debug)]
pub struct GraphDocsArgs {
    /// Agent ID or database path
    pub id_or_path: String,

    #[clap(subcommand)]
    pub command: GraphDocsCommand,
}

#[derive(Subcommand, Debug)]
pub enum GraphDocsCommand {
    /// List all GraphDocs documents
    List(ListArgs),

    /// Create a new document
    Create(CreateArgs),

    /// Delete a document
    Delete(DeleteArgs),

    /// Show document details
    Show(ShowArgs),

    /// Add a section to a document
    AddSection(AddSectionArgs),

    /// Remove a section from a document
    RemoveSection(RemoveSectionArgs),

    /// Set a variable value
    SetVar(SetVarArgs),

    /// Get a variable value
    GetVar(GetVarArgs),

    /// List variables for a document
    ListVars(ListVarsArgs),

    /// Import a Markdown file as GraphDoc
    Import(ImportArgs),

    /// Import all Markdown files from a directory
    ImportDir(ImportDirArgs),

    /// Export a GraphDoc to Markdown file
    Export(ExportArgs),

    /// Render a GraphDoc to Markdown (with time-travel support)
    Render(RenderArgs),

    /// Show document history (journal events)
    History(HistoryArgs),

    /// Check and fix document conformance using TEA agents
    Conform(ConformArgs),

    /// Check document conformance against a template (no LLM required)
    Check(CheckArgs),

    /// Edit a document in a text editor (YAML/TOML format)
    Edit(EditArgs),
}

/// Output format for list command
#[derive(ValueEnum, Clone, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Csv,
}

/// Section type for add-section command
#[derive(ValueEnum, Clone, Debug)]
pub enum SectionType {
    Heading,
    Paragraph,
    List,
    Code,
    Blockquote,
    Hr,
}

impl SectionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SectionType::Heading => "heading",
            SectionType::Paragraph => "paragraph",
            SectionType::List => "list",
            SectionType::Code => "code",
            SectionType::Blockquote => "blockquote",
            SectionType::Hr => "hr",
        }
    }
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Show only documents matching pattern
    #[clap(long)]
    pub filter: Option<String>,

    /// Output format: table, json, csv
    #[clap(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Document ID
    pub doc_id: String,

    /// Document title
    #[clap(long, short)]
    pub title: String,

    /// Base template to inherit from
    #[clap(long)]
    pub template: Option<String>,

    /// Document language
    #[clap(long, default_value = "en")]
    pub language: String,

    /// Document description
    #[clap(long)]
    pub description: Option<String>,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Document ID to delete
    pub doc_id: String,

    /// Skip confirmation
    #[clap(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Document ID
    pub doc_id: String,

    /// Show in JSON format
    #[clap(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct AddSectionArgs {
    /// Document ID
    pub doc_id: String,

    /// Section type: heading, paragraph, list, code, blockquote, hr
    #[clap(long, short = 't', value_enum)]
    pub section_type: SectionType,

    /// Section content
    #[clap(long, short)]
    pub content: String,

    /// Heading level (1-6, for heading type)
    #[clap(long, short)]
    pub level: Option<u8>,

    /// Position (0-based index, or "end")
    #[clap(long, short, default_value = "end")]
    pub position: String,

    /// Section to override (for inheritance)
    #[clap(long)]
    pub override_section: Option<String>,
}

#[derive(Args, Debug)]
pub struct RemoveSectionArgs {
    /// Document ID
    pub doc_id: String,

    /// Section ID to remove
    pub section_id: String,
}

#[derive(Args, Debug)]
pub struct SetVarArgs {
    /// Document ID
    pub doc_id: String,

    /// Variable name
    pub name: String,

    /// Variable value (JSON or plain string)
    pub value: String,
}

#[derive(Args, Debug)]
pub struct GetVarArgs {
    /// Document ID
    pub doc_id: String,

    /// Variable name
    pub name: String,
}

#[derive(Args, Debug)]
pub struct ListVarsArgs {
    /// Document ID
    pub doc_id: String,

    /// Include inherited variables
    #[clap(long)]
    pub inherited: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ImportArgs {
    /// Path to Markdown file
    pub file: PathBuf,

    /// Document ID (defaults to filename without extension)
    #[clap(long, short)]
    pub id: Option<String>,

    /// Document title (defaults to first H1 or filename)
    #[clap(long, short)]
    pub title: Option<String>,

    /// Base template to inherit from
    #[clap(long)]
    pub template: Option<String>,

    /// Use LLM for intelligent structure extraction
    #[clap(long)]
    pub llm: bool,

    /// LLM model to use (default: gpt-4)
    #[clap(long, default_value = "gpt-4")]
    pub model: String,

    /// Dry run - show what would be imported without writing
    #[clap(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct ImportDirArgs {
    /// Directory containing Markdown files
    pub dir: PathBuf,

    /// File pattern to match (default: *.md)
    #[clap(long, default_value = "*.md")]
    pub pattern: String,

    /// Recursive search
    #[clap(long, short)]
    pub recursive: bool,

    /// Base template for all imported documents
    #[clap(long)]
    pub template: Option<String>,

    /// Use LLM for intelligent structure extraction
    #[clap(long)]
    pub llm: bool,

    /// LLM model to use
    #[clap(long, default_value = "gpt-4")]
    pub model: String,

    /// Dry run
    #[clap(long)]
    pub dry_run: bool,

    /// Continue on errors
    #[clap(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Document ID to export
    pub doc_id: String,

    /// Output file path
    #[clap(long, short)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct RenderArgs {
    /// Document ID to render
    pub doc_id: String,

    /// Render at specific event ID (time-travel)
    #[clap(long)]
    pub at: Option<i64>,

    /// Output file (stdout if not specified)
    #[clap(long, short)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct HistoryArgs {
    /// Document ID
    pub doc_id: String,

    /// Number of events to show
    #[clap(long, default_value = "20")]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct ConformArgs {
    /// Directory to scan for documents
    pub dir: PathBuf,

    /// Path to GGUF model (default: ~/.cache/tea/models/gemma-3n-E4B-it-Q4_K_M.gguf)
    #[clap(long)]
    pub model_path: Option<PathBuf>,

    /// Directory containing agent YAML files (default: ./agents)
    #[clap(long)]
    pub agents_dir: Option<PathBuf>,

    /// Preview changes without writing
    #[clap(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Directory to scan for documents
    pub dir: PathBuf,

    /// Template to check against (required)
    #[clap(long, short)]
    pub template: String,

    /// Output format: table, json
    #[clap(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

/// Output format for edit command
#[derive(ValueEnum, Clone, Debug, Default)]
pub enum EditFormat {
    #[default]
    Yaml,
    Toml,
}

#[derive(Args, Debug)]
pub struct EditArgs {
    /// Document ID to edit
    pub doc_id: String,

    /// Editor to use (defaults to $EDITOR or vim)
    #[clap(long, short)]
    pub editor: Option<String>,

    /// Output format: yaml or toml
    #[clap(long, value_enum, default_value_t = EditFormat::Yaml)]
    pub format: EditFormat,
}

// =============================================================================
// Editable Document Structs for YAML/TOML Serialization
// =============================================================================

/// Serializable document structure for editing
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct EditableDocument {
    pub document: DocumentMeta,
    pub sections: Vec<EditableSection>,
    pub variables: Vec<EditableVariable>,
}

/// Document metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DocumentMeta {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_template: Option<String>,
    pub language: String,
}

/// Editable section
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct EditableSection {
    pub id: String,
    #[serde(rename = "type")]
    pub section_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    pub order: i32,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_section: Option<String>,
}

/// Editable variable
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct EditableVariable {
    pub name: String,
    pub value: serde_json::Value,
    #[serde(rename = "type")]
    pub var_type: String,
}

// =============================================================================
// Command Handlers - List, Create, Delete, Show, AddSection, RemoveSection,
//                   SetVar, GetVar, ListVars
// =============================================================================

/// Handle the graphdocs list command - list all documents
pub async fn handle_list(fs: &DuckAgentFS, args: ListArgs) -> Result<()> {
    let conn = fs.get_connection()?;

    // Build query based on filter
    let query = match &args.filter {
        Some(pattern) => {
            // Use parameterized LIKE pattern for safety
            format!(
                "SELECT id, title, base_template, updated_at FROM gd_documents WHERE id LIKE '%{}%' OR title LIKE '%{}%' ORDER BY id",
                pattern.replace('\'', "''"),
                pattern.replace('\'', "''")
            )
        }
        None => {
            "SELECT id, title, base_template, updated_at FROM gd_documents ORDER BY id".to_string()
        }
    };

    let mut stmt = conn.prepare(&query)?;
    let docs: Vec<(String, String, Option<String>, String)> = stmt
        .query_map([], |row: &duckdb::Row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;

    match args.format {
        OutputFormat::Table => {
            println!(
                "{:<20} {:<30} {:<20} {:<20}",
                "ID", "TITLE", "TEMPLATE", "UPDATED"
            );
            println!("{}", "-".repeat(90));
            for (id, title, template, updated) in docs {
                println!(
                    "{:<20} {:<30} {:<20} {:<20}",
                    truncate(&id, 18),
                    truncate(&title, 28),
                    template.as_deref().unwrap_or("-"),
                    truncate(&updated, 18)
                );
            }
        }
        OutputFormat::Json => {
            let json: Vec<serde_json::Value> = docs
                .iter()
                .map(|(id, title, template, updated)| {
                    serde_json::json!({
                        "id": id,
                        "title": title,
                        "template": template,
                        "updated": updated
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        OutputFormat::Csv => {
            println!("id,title,template,updated");
            for (id, title, template, updated) in docs {
                println!(
                    "{},{},{},{}",
                    id,
                    title.replace(',', "\\,"),
                    template.unwrap_or_default(),
                    updated
                );
            }
        }
    }

    Ok(())
}

/// Handle the graphdocs create command - create a new document
pub async fn handle_create(fs: &DuckAgentFS, args: CreateArgs) -> Result<()> {
    let conn = fs.get_write_connection()?;

    // Check if already exists
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM gd_documents WHERE id = ?)",
            [&args.doc_id],
            |r: &duckdb::Row| r.get(0),
        )
        .unwrap_or(false);

    if exists {
        anyhow::bail!("Document '{}' already exists", args.doc_id);
    }

    // Validate template exists if specified
    if let Some(ref template) = args.template {
        let template_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM gd_documents WHERE id = ?)",
                [template],
                |r: &duckdb::Row| r.get(0),
            )
            .unwrap_or(false);

        if !template_exists {
            anyhow::bail!("Template '{}' not found", template);
        }
    }

    // Note: gd_documents doesn't have description column, use metadata JSON instead
    let metadata = args
        .description
        .map(|d| serde_json::json!({"description": d}));

    conn.execute(
        r#"INSERT INTO gd_documents (id, title, base_template, language, metadata)
           VALUES (?, ?, ?, ?, ?)"#,
        duckdb::params![
            args.doc_id,
            args.title,
            args.template,
            args.language,
            metadata.map(|m| m.to_string())
        ],
    )
    .context("Failed to insert document")?;

    println!("Created document '{}'", args.doc_id);

    if let Some(template) = args.template {
        println!("  Inheriting from template '{}'", template);
    }

    Ok(())
}

/// Handle the graphdocs delete command - delete a document
pub async fn handle_delete(fs: &DuckAgentFS, args: DeleteArgs) -> Result<()> {
    let conn = fs.get_write_connection()?;

    // Check if document exists
    let doc_info: Option<(String, i64, i64)> = conn
        .query_row(
            r#"SELECT d.title,
                      (SELECT COUNT(*) FROM gd_sections WHERE document_id = d.id),
                      (SELECT COUNT(*) FROM gd_variables WHERE document_id = d.id)
               FROM gd_documents d WHERE d.id = ?"#,
            [&args.doc_id],
            |row: &duckdb::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    let Some((title, section_count, var_count)) = doc_info else {
        anyhow::bail!("Document '{}' not found", args.doc_id);
    };

    if !args.force {
        println!("About to delete document '{}' ({}):", args.doc_id, title);
        println!("  Sections: {}", section_count);
        println!("  Variables: {}", var_count);
        println!("\nUse --force to confirm deletion.");
        return Ok(());
    }

    // Delete in order: edges, variables, sections, document
    conn.execute(
        "DELETE FROM gd_edges WHERE source_id IN (SELECT id FROM gd_sections WHERE document_id = ?) OR target_id IN (SELECT id FROM gd_sections WHERE document_id = ?)",
        duckdb::params![args.doc_id, args.doc_id],
    )?;
    conn.execute(
        "DELETE FROM gd_variables WHERE document_id = ?",
        [&args.doc_id],
    )?;
    conn.execute(
        "DELETE FROM gd_sections WHERE document_id = ?",
        [&args.doc_id],
    )?;
    conn.execute("DELETE FROM gd_documents WHERE id = ?", [&args.doc_id])?;

    println!("Deleted document '{}' ({})", args.doc_id, title);
    println!(
        "  Removed {} sections, {} variables",
        section_count, var_count
    );

    Ok(())
}

/// Handle the graphdocs show command - show document details
pub async fn handle_show(fs: &DuckAgentFS, args: ShowArgs) -> Result<()> {
    let conn = fs.get_connection()?;

    // Load document
    let doc: (String, String, Option<String>, String, i64) = conn
        .query_row(
            "SELECT id, title, base_template, language, version FROM gd_documents WHERE id = ?",
            [&args.doc_id],
            |row: &duckdb::Row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| anyhow::anyhow!("Document '{}' not found", args.doc_id))?;

    // Load sections
    let mut stmt = conn.prepare(
        "SELECT id, section_type, level, order_idx FROM gd_sections WHERE document_id = ? ORDER BY order_idx",
    )?;
    let sections: Vec<(String, String, Option<i64>, i64)> = stmt
        .query_map([&args.doc_id], |row: &duckdb::Row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;

    // Load variables
    let mut var_stmt =
        conn.prepare("SELECT name, var_type FROM gd_variables WHERE document_id = ?")?;
    let vars: Vec<(String, String)> = var_stmt
        .query_map([&args.doc_id], |row: &duckdb::Row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;

    if args.json {
        let json = serde_json::json!({
            "id": doc.0,
            "title": doc.1,
            "base_template": doc.2,
            "language": doc.3,
            "version": doc.4,
            "sections": sections.iter().map(|(id, t, l, o)| {
                serde_json::json!({"id": id, "type": t, "level": l, "order": o})
            }).collect::<Vec<_>>(),
            "variables": vars.iter().map(|(n, t)| {
                serde_json::json!({"name": n, "type": t})
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("Document: {}", doc.0);
        println!("Title: {}", doc.1);
        println!("Template: {}", doc.2.as_deref().unwrap_or("-"));
        println!("Language: {}", doc.3);
        println!("Version: {}", doc.4);
        println!();
        println!("Sections ({}):", sections.len());
        for (id, section_type, level, order) in &sections {
            let level_str = level.map(|l| format!(" H{}", l)).unwrap_or_default();
            let id_preview = if id.len() > 8 { &id[..8] } else { id };
            println!(
                "  [{}] {}{} - {}",
                order, section_type, level_str, id_preview
            );
        }
        println!();
        println!("Variables ({}):", vars.len());
        for (name, var_type) in &vars {
            println!("  {} ({})", name, var_type);
        }
    }

    Ok(())
}

/// Handle the graphdocs add-section command - add a section to a document
pub async fn handle_add_section(fs: &DuckAgentFS, args: AddSectionArgs) -> Result<()> {
    let conn = fs.get_write_connection()?;

    // Validate document exists
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM gd_documents WHERE id = ?)",
            [&args.doc_id],
            |r: &duckdb::Row| r.get(0),
        )
        .unwrap_or(false);

    if !exists {
        anyhow::bail!("Document '{}' not found", args.doc_id);
    }

    // Determine order_idx
    let order_idx: i64 = if args.position == "end" {
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(order_idx) FROM gd_sections WHERE document_id = ?",
                [&args.doc_id],
                |r: &duckdb::Row| r.get(0),
            )
            .unwrap_or(None);
        max.unwrap_or(-1) + 1
    } else {
        args.position
            .parse::<i64>()
            .context("Position must be a number or 'end'")?
    };

    // Validate heading level
    let level: Option<i64> = match args.section_type {
        SectionType::Heading => {
            let l = args.level.unwrap_or(1);
            if !(1..=6).contains(&l) {
                anyhow::bail!("Heading level must be 1-6");
            }
            Some(l as i64)
        }
        _ => None,
    };

    let section_id = uuid::Uuid::new_v4().to_string();

    conn.execute(
        r#"INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content, source_section)
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        duckdb::params![
            section_id,
            args.doc_id,
            args.section_type.as_str(),
            level,
            order_idx,
            args.content,
            args.override_section
        ],
    )
    .context("Failed to insert section")?;

    println!(
        "Added {} section at position {}",
        args.section_type.as_str(),
        order_idx
    );
    println!("Section ID: {}", section_id);

    Ok(())
}

/// Handle the graphdocs remove-section command - remove a section from a document
pub async fn handle_remove_section(fs: &DuckAgentFS, args: RemoveSectionArgs) -> Result<()> {
    let conn = fs.get_write_connection()?;

    // Check if section exists
    let section_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM gd_sections WHERE id = ? AND document_id = ?)",
            duckdb::params![args.section_id, args.doc_id],
            |r: &duckdb::Row| r.get(0),
        )
        .unwrap_or(false);

    if !section_exists {
        anyhow::bail!(
            "Section '{}' not found in document '{}'",
            args.section_id,
            args.doc_id
        );
    }

    // Delete edges referencing this section
    conn.execute(
        "DELETE FROM gd_edges WHERE source_id = ? OR target_id = ?",
        duckdb::params![args.section_id, args.section_id],
    )?;

    // Delete the section
    conn.execute("DELETE FROM gd_sections WHERE id = ?", [&args.section_id])?;

    println!(
        "Removed section '{}' from document '{}'",
        args.section_id, args.doc_id
    );

    Ok(())
}

/// Handle the graphdocs set-var command - set a variable value
pub async fn handle_set_var(fs: &DuckAgentFS, args: SetVarArgs) -> Result<()> {
    let engine = GraphDocsEngine::new(fs.pool());

    // Parse value as JSON, falling back to string
    let value: serde_json::Value = serde_json::from_str(&args.value)
        .unwrap_or_else(|_| serde_json::Value::String(args.value.clone()));

    engine
        .set_variable(&args.doc_id, &args.name, value.clone())
        .await?;

    println!("Set {} = {}", args.name, value);

    Ok(())
}

/// Handle the graphdocs get-var command - get a variable value
pub async fn handle_get_var(fs: &DuckAgentFS, args: GetVarArgs) -> Result<()> {
    let engine = GraphDocsEngine::new(fs.pool());
    let variables = engine.get_variables(&args.doc_id).await?;

    match variables.get(&args.name) {
        Some(value) => {
            // Output just the value (without quotes for strings)
            match value {
                serde_json::Value::String(s) => println!("{}", s),
                _ => println!("{}", value),
            }
        }
        None => {
            eprintln!("Variable '{}' not found", args.name);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Handle the graphdocs list-vars command - list variables for a document
pub async fn handle_list_vars(fs: &DuckAgentFS, args: ListVarsArgs) -> Result<()> {
    if args.inherited {
        // Use engine to get merged variables (including inherited)
        let engine = GraphDocsEngine::new(fs.pool());
        let vars = engine.get_variables(&args.doc_id).await?;

        println!("{:<20} {:<40} {:<10}", "NAME", "VALUE", "SOURCE");
        println!("{}", "-".repeat(70));

        for (name, value) in vars {
            println!(
                "{:<20} {:<40} merged",
                name,
                truncate(&value.to_string(), 38)
            );
        }
    } else {
        // Get only direct variables for this document
        let conn = fs.get_connection()?;

        let mut stmt = conn.prepare(
            "SELECT name, value, var_type FROM gd_variables WHERE document_id = ? ORDER BY name",
        )?;
        let vars: Vec<(String, String, String)> = stmt
            .query_map([&args.doc_id], |row: &duckdb::Row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<duckdb::Result<Vec<_>>>()?;

        println!("{:<20} {:<40} {:<10}", "NAME", "VALUE", "TYPE");
        println!("{}", "-".repeat(70));

        for (name, value, var_type) in vars {
            println!("{:<20} {:<40} {:<10}", name, truncate(&value, 38), var_type);
        }
    }

    Ok(())
}

/// Truncate a string to max characters
fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

// =============================================================================
// Command Handlers - Import, Export, Render, History, Conform
// =============================================================================

/// Handle the graphdocs import command
pub async fn handle_import(fs: &DuckAgentFS, args: ImportArgs) -> Result<()> {
    // Read file
    let content = std::fs::read_to_string(&args.file)
        .with_context(|| format!("Failed to read file: {:?}", args.file))?;

    // Determine document ID
    let doc_id = args.id.unwrap_or_else(|| {
        args.file
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string())
    });

    println!("Importing {} as '{}'...", args.file.display(), doc_id);

    // Parse document
    let parsed = if args.llm {
        let api_key =
            std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY not set for --llm mode")?;
        let client = OpenAIClient::new(api_key);
        let converter = LLMConverter::new(Box::new(client), args.model);
        converter.convert(&content).await?
    } else {
        let parser = MarkdownParser::new();
        parser.parse(&content)?
    };

    // Determine title
    let title = args
        .title
        .or(parsed.title.clone())
        .unwrap_or_else(|| doc_id.clone());

    if args.dry_run {
        println!("\nDry run - would import:");
        println!("  Document ID: {}", doc_id);
        println!("  Title: {}", title);
        println!("  Sections: {}", parsed.sections.len());
        println!("  Variables: {:?}", parsed.variables);
        if let Some(ref template) = args.template {
            println!("  Base template: {}", template);
        }
        println!("\nSections:");
        for section in &parsed.sections {
            println!(
                "  [{}] {} - {} chars",
                section.order_idx,
                section.section_type.as_str(),
                section.content.len()
            );
        }
        return Ok(());
    }

    // Get connection and perform database operations
    let conn = fs.get_write_connection()?;

    // Check if document already exists
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM gd_documents WHERE id = ?)",
            [&doc_id],
            |r: &duckdb::Row| -> duckdb::Result<bool> { r.get(0) },
        )
        .unwrap_or(false);

    if exists {
        anyhow::bail!(
            "Document '{}' already exists. Use --id to specify a different ID.",
            doc_id
        );
    }

    // Insert document
    conn.execute(
        "INSERT INTO gd_documents (id, title, base_template) VALUES (?, ?, ?)",
        duckdb::params![doc_id, title, args.template],
    )
    .context("Failed to insert document")?;

    // Insert sections
    for section in &parsed.sections {
        conn.execute(
            r#"INSERT INTO gd_sections
               (id, document_id, section_type, level, order_idx, content)
               VALUES (?, ?, ?, ?, ?, ?)"#,
            duckdb::params![
                section.id,
                doc_id,
                section.section_type.as_str(),
                section.level.map(|l| l as i64),
                section.order_idx as i64,
                section.content,
            ],
        )
        .context("Failed to insert section")?;
    }

    // Insert placeholder variables
    for var_name in &parsed.variables {
        let var_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            r#"INSERT INTO gd_variables (id, document_id, name, value, var_type)
               VALUES (?, ?, ?, '""', 'string')"#,
            duckdb::params![var_id, doc_id, var_name],
        )
        .context("Failed to insert variable")?;
    }

    // Insert edges (follows relationships between sections)
    for edge in &parsed.edges {
        let edge_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            r#"INSERT INTO gd_edges (source_id, target_id, edge_type)
               VALUES (?, ?, 'follows')"#,
            duckdb::params![
                parsed.sections[edge.source_idx].id,
                parsed.sections[edge.target_idx].id,
            ],
        )
        .context("Failed to insert edge")?;

        // Avoid unused variable warning
        let _ = edge_id;
    }

    println!("Imported successfully!");
    println!("  Sections: {}", parsed.sections.len());
    println!("  Variables: {}", parsed.variables.len());
    println!("  Edges: {}", parsed.edges.len());

    if !parsed.variables.is_empty() {
        println!("\nSet variable values with:");
        for var in &parsed.variables {
            println!("  agentfs graphdocs set-var {} {} <value>", doc_id, var);
        }
    }

    Ok(())
}

/// Handle the graphdocs import-dir command
pub async fn handle_import_dir(fs: &DuckAgentFS, args: ImportDirArgs) -> Result<()> {
    let pattern = if args.recursive {
        format!("{}/**/{}", args.dir.display(), args.pattern)
    } else {
        format!("{}/{}", args.dir.display(), args.pattern)
    };

    let files: Vec<PathBuf> = glob::glob(&pattern)?.filter_map(|r| r.ok()).collect();

    if files.is_empty() {
        println!("No files found matching pattern: {}", pattern);
        return Ok(());
    }

    println!("Found {} files to import", files.len());

    let mut success = 0;
    let mut failed = 0;

    for file in &files {
        let import_args = ImportArgs {
            file: file.clone(),
            id: None,
            title: None,
            template: args.template.clone(),
            llm: args.llm,
            model: args.model.clone(),
            dry_run: args.dry_run,
        };

        match handle_import(fs, import_args).await {
            Ok(_) => {
                success += 1;
            }
            Err(e) => {
                eprintln!("Error importing {:?}: {}", file, e);
                failed += 1;
                if !args.force {
                    return Err(e);
                }
            }
        }
    }

    println!("\nImport complete:");
    println!("  Success: {}", success);
    println!("  Failed: {}", failed);

    Ok(())
}

/// Handle the graphdocs export command
pub async fn handle_export(fs: &DuckAgentFS, args: ExportArgs) -> Result<()> {
    let conn = fs.get_connection()?;

    // Get document info
    let (title, base_template): (String, Option<String>) = conn
        .query_row(
            "SELECT title, base_template FROM gd_documents WHERE id = ?",
            [&args.doc_id],
            |row: &duckdb::Row| -> duckdb::Result<(String, Option<String>)> {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            },
        )
        .with_context(|| format!("Document '{}' not found", args.doc_id))?;

    // Get sections ordered by order_idx
    let mut stmt = conn.prepare(
        r#"SELECT section_type, level, content
           FROM gd_sections
           WHERE document_id = ?
           ORDER BY order_idx"#,
    )?;

    let sections: Vec<(String, Option<i64>, String)> = stmt
        .query_map(
            [&args.doc_id],
            |row: &duckdb::Row| -> duckdb::Result<(String, Option<i64>, String)> {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?
        .collect::<duckdb::Result<Vec<(String, Option<i64>, String)>>>()?;

    // Get variables for substitution
    let mut var_stmt =
        conn.prepare("SELECT name, value FROM gd_variables WHERE document_id = ?")?;

    let variables: std::collections::HashMap<String, String> = var_stmt
        .query_map(
            [&args.doc_id],
            |row: &duckdb::Row| -> duckdb::Result<(String, String)> {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            },
        )?
        .filter_map(|r: duckdb::Result<(String, String)>| r.ok())
        .collect();

    // Build markdown
    let mut markdown = String::new();

    // Add metadata comment if there's a base template
    if let Some(template) = base_template {
        markdown.push_str(&format!("<!-- Base template: {} -->\n\n", template));
    }

    for (section_type, level, content) in sections.into_iter() {
        let section_type: String = section_type;
        let level: Option<i64> = level;
        let content: String = content;
        let rendered_content = substitute_variables(&content, &variables);

        match section_type.as_str() {
            "heading" => {
                let heading_level = level.unwrap_or(1) as usize;
                let prefix = "#".repeat(heading_level);
                markdown.push_str(&format!("{} {}\n\n", prefix, rendered_content));
            }
            "paragraph" => {
                markdown.push_str(&format!("{}\n\n", rendered_content));
            }
            "code" => {
                markdown.push_str(&format!("```\n{}\n```\n\n", rendered_content));
            }
            "list" => {
                // Assume each line is a list item
                for line in rendered_content.lines() {
                    markdown.push_str(&format!("- {}\n", line));
                }
                markdown.push('\n');
            }
            "blockquote" => {
                for line in rendered_content.lines() {
                    markdown.push_str(&format!("> {}\n", line));
                }
                markdown.push('\n');
            }
            "hr" => {
                markdown.push_str("---\n\n");
            }
            "table" => {
                markdown.push_str(&format!("{}\n\n", rendered_content));
            }
            "checklist" => {
                for line in rendered_content.lines() {
                    markdown.push_str(&format!("- [ ] {}\n", line));
                }
                markdown.push('\n');
            }
            _ => {
                markdown.push_str(&format!("{}\n\n", rendered_content));
            }
        }
    }

    // Output
    match args.output {
        Some(path) => {
            std::fs::write(&path, &markdown)?;
            println!("Exported '{}' ({}) to {:?}", args.doc_id, title, path);
        }
        None => {
            println!("{}", markdown);
        }
    }

    Ok(())
}

/// Handle the graphdocs render command - render with optional time-travel
pub async fn handle_render(fs: &DuckAgentFS, args: RenderArgs) -> Result<()> {
    let engine = GraphDocsEngine::new(fs.pool());

    let markdown = match args.at {
        Some(event_id) => {
            println!("Rendering '{}' at event {}", args.doc_id, event_id);
            engine.render_at(&args.doc_id, event_id).await?
        }
        None => engine.render(&args.doc_id).await?,
    };

    match args.output {
        Some(path) => {
            std::fs::write(&path, &markdown)?;
            println!("Written to {:?}", path);
        }
        None => {
            println!("{}", markdown);
        }
    }

    Ok(())
}

/// Handle the graphdocs history command - show journal events for a document
pub async fn handle_history(fs: &DuckAgentFS, args: HistoryArgs) -> Result<()> {
    let engine = GraphDocsEngine::new(fs.pool());
    let events = engine.list_events(&args.doc_id, args.limit).await?;

    if events.is_empty() {
        println!("No events found for document '{}'", args.doc_id);
        return Ok(());
    }

    println!(
        "{:<12} {:<10} {:<20} {:<25}",
        "EVENT_ID", "TYPE", "TABLE", "TIME"
    );
    println!("{}", "-".repeat(67));

    for event in events {
        println!(
            "{:<12} {:<10} {:<20} {:<25}",
            event.event_id,
            event.event_type,
            event.table_name,
            event.event_time.format("%Y-%m-%d %H:%M:%S"),
        );
    }

    Ok(())
}

/// Handle the graphdocs conform command - check and fix document conformance using TEA
pub async fn handle_conform(args: ConformArgs) -> Result<()> {
    println!("Scanning directory: {:?}", args.dir);
    if args.dry_run {
        println!("(dry-run mode - no files will be modified)\n");
    }

    let sdk_args = SdkConformArgs {
        dir: args.dir,
        model_path: args.model_path,
        agents_dir: args.agents_dir,
        dry_run: args.dry_run,
    };

    let results = batch_transform(&sdk_args).await?;

    for result in &results {
        if result.dry_run {
            println!(
                "[DRY-RUN] Would transform: {} ({} issues)",
                result.file_path, result.original_issues
            );
            if let Some(ref content) = result.new_content {
                println!("--- Preview ---");
                // Show first 500 chars of transformed content
                let preview: String = content.chars().take(500).collect();
                println!("{}", preview);
                if content.len() > 500 {
                    println!("... ({} more chars)", content.len() - 500);
                }
                println!("--- End Preview ---\n");
            }
        } else {
            println!(
                "Transformed: {} ({} issues fixed)",
                result.file_path, result.original_issues
            );
        }
    }

    println!("\nTotal: {} files processed", results.len());
    Ok(())
}

/// Substitute {{variable}} placeholders with values
fn substitute_variables(
    content: &str,
    variables: &std::collections::HashMap<String, String>,
) -> String {
    let mut result = content.to_string();
    for (name, value) in variables {
        let placeholder = format!("{{{{{}}}}}", name);
        // Try to parse the JSON value, falling back to raw string
        let actual_value = serde_json::from_str::<serde_json::Value>(value)
            .map(|v| match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            })
            .unwrap_or_else(|_| value.clone());
        result = result.replace(&placeholder, &actual_value);
    }
    result
}

// =============================================================================
// Edit Command Handler and Supporting Functions
// =============================================================================

/// Handle the graphdocs edit command - open document in editor
pub async fn handle_edit(fs: &DuckAgentFS, args: EditArgs) -> Result<()> {
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Load document
    let doc = load_document_for_edit(fs, &args.doc_id)?;

    // Serialize to YAML/TOML
    let content = match args.format {
        EditFormat::Yaml => {
            serde_yaml::to_string(&doc).context("Failed to serialize document to YAML")?
        }
        EditFormat::Toml => {
            toml::to_string_pretty(&doc).context("Failed to serialize document to TOML")?
        }
    };

    // Write to temp file
    let extension = match args.format {
        EditFormat::Yaml => "yaml",
        EditFormat::Toml => "toml",
    };
    let mut temp_file = NamedTempFile::with_suffix(format!(".{}", extension))
        .context("Failed to create temp file")?;
    temp_file
        .write_all(content.as_bytes())
        .context("Failed to write temp file")?;
    let temp_path = temp_file.path().to_owned();

    // Get editor
    let editor = args
        .editor
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vim".to_string());

    // Open editor
    let status = std::process::Command::new(&editor)
        .arg(&temp_path)
        .status()
        .context("Failed to launch editor")?;

    if !status.success() {
        anyhow::bail!("Editor exited with error");
    }

    // Read modified content
    let modified_content =
        std::fs::read_to_string(&temp_path).context("Failed to read modified file")?;

    // Parse
    let modified_doc: EditableDocument = match args.format {
        EditFormat::Yaml => {
            serde_yaml::from_str(&modified_content).context("Failed to parse modified YAML")?
        }
        EditFormat::Toml => {
            toml::from_str(&modified_content).context("Failed to parse modified TOML")?
        }
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
    apply_changes(fs, &args.doc_id, &doc, &modified_doc)?;

    println!("Changes applied successfully.");

    Ok(())
}

/// Handle the graphdocs check command - check document conformance without LLM
///
/// Note: This command is a placeholder. Use `agentfs graphdocs conform` for full
/// conformance checking with LLM support.
pub async fn handle_check(_fs: &DuckAgentFS, args: CheckArgs) -> Result<()> {
    // TODO: Implement template-based conformance checking without LLM
    // For now, just validate that the directory exists and contains markdown files
    if !args.dir.exists() {
        anyhow::bail!("Directory not found: {:?}", args.dir);
    }

    let mut count = 0;
    for entry in std::fs::read_dir(&args.dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            count += 1;
            println!("Found: {:?}", path.file_name().unwrap_or_default());
        }
    }

    println!("\nFound {} markdown files in {:?}", count, args.dir);
    println!("Note: Full conformance checking against template '{}' is not yet implemented.", args.template);
    println!("Use `agentfs graphdocs conform` for LLM-based conformance checking.");

    Ok(())
}

/// Load a document for editing
fn load_document_for_edit(fs: &DuckAgentFS, doc_id: &str) -> Result<EditableDocument> {
    let conn = fs.get_connection()?;

    // Load document metadata
    let (title, metadata, base_template, language): (
        String,
        Option<String>,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT title, metadata, base_template, language FROM gd_documents WHERE id = ?",
            [doc_id],
            |row: &duckdb::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| anyhow::anyhow!("Document '{}' not found", doc_id))?;

    // Extract description from metadata JSON if present
    let description = metadata.and_then(|m| {
        serde_json::from_str::<serde_json::Value>(&m)
            .ok()
            .and_then(|v| {
                v.get("description")
                    .and_then(|d| d.as_str().map(String::from))
            })
    });

    // Load sections
    let mut stmt = conn.prepare(
        r#"SELECT id, section_type, level, order_idx, content, source_section
           FROM gd_sections
           WHERE document_id = ?
           ORDER BY order_idx"#,
    )?;

    let sections: Vec<EditableSection> = stmt
        .query_map([doc_id], |row: &duckdb::Row| {
            Ok(EditableSection {
                id: row.get(0)?,
                section_type: row.get(1)?,
                level: row.get::<_, Option<i64>>(2)?.map(|l| l as u8),
                order: row.get::<_, i64>(3)? as i32,
                content: row.get(4)?,
                override_section: row.get(5)?,
            })
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;

    // Load variables
    let mut stmt =
        conn.prepare("SELECT name, value, var_type FROM gd_variables WHERE document_id = ?")?;

    let variables: Vec<EditableVariable> = stmt
        .query_map([doc_id], |row: &duckdb::Row| {
            let value_str: String = row.get(1)?;
            let value: serde_json::Value =
                serde_json::from_str(&value_str).unwrap_or(serde_json::Value::String(value_str));
            Ok(EditableVariable {
                name: row.get(0)?,
                value,
                var_type: row.get(2)?,
            })
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;

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

/// Validate an editable document
fn validate_document(doc: &EditableDocument) -> Result<()> {
    // Validate section types
    let valid_types = [
        "heading",
        "paragraph",
        "list",
        "code",
        "blockquote",
        "hr",
        "table",
        "checklist",
    ];
    for section in &doc.sections {
        if !valid_types.contains(&section.section_type.as_str()) {
            anyhow::bail!(
                "Invalid section type '{}' in section {}",
                section.section_type,
                section.id
            );
        }

        // Validate heading level
        if section.section_type == "heading" {
            if let Some(level) = section.level {
                if !(1..=6).contains(&level) {
                    anyhow::bail!("Invalid heading level {} in section {}", level, section.id);
                }
            }
        }
    }

    // Validate variable types
    let valid_var_types = ["string", "number", "boolean", "array", "object"];
    for var in &doc.variables {
        if !valid_var_types.contains(&var.var_type.as_str()) {
            anyhow::bail!(
                "Invalid variable type '{}' for variable '{}'",
                var.var_type,
                var.name
            );
        }
    }

    // Validate unique section IDs
    let mut seen_ids = std::collections::HashSet::new();
    for section in &doc.sections {
        if !seen_ids.insert(&section.id) {
            anyhow::bail!("Duplicate section ID: {}", section.id);
        }
    }

    // Validate unique variable names
    let mut seen_names = std::collections::HashSet::new();
    for var in &doc.variables {
        if !seen_names.insert(&var.name) {
            anyhow::bail!("Duplicate variable name: {}", var.name);
        }
    }

    Ok(())
}

/// Diff two documents and return a list of change descriptions
fn diff_documents(old: &EditableDocument, new: &EditableDocument) -> Vec<String> {
    let mut changes = Vec::new();

    // Document metadata changes
    if old.document.title != new.document.title {
        changes.push(format!(
            "Title: '{}' -> '{}'",
            old.document.title, new.document.title
        ));
    }
    if old.document.description != new.document.description {
        changes.push("Description changed".to_string());
    }
    if old.document.base_template != new.document.base_template {
        changes.push(format!(
            "Template: {:?} -> {:?}",
            old.document.base_template, new.document.base_template
        ));
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
        if old_sec.content != new_sec.content
            || old_sec.section_type != new_sec.section_type
            || old_sec.level != new_sec.level
            || old_sec.order != new_sec.order
        {
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
        if old_var.value != new_var.value || old_var.var_type != new_var.var_type {
            changes.push(format!("Modify variable: {}", name));
        }
    }

    changes
}

/// Apply changes from edited document to database
fn apply_changes(
    fs: &DuckAgentFS,
    doc_id: &str,
    old: &EditableDocument,
    new: &EditableDocument,
) -> Result<()> {
    let conn = fs.get_write_connection()?;

    // Update document metadata - only update if changed
    // Note: We update fields individually to avoid DuckDB FK constraint issues
    // when updating parent tables that have child records
    if old.document.title != new.document.title {
        conn.execute(
            "UPDATE gd_documents SET title = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            duckdb::params![new.document.title, doc_id],
        )
        .context("Failed to update document title")?;
    }

    if old.document.description != new.document.description {
        let metadata = new
            .document
            .description
            .as_ref()
            .map(|d| serde_json::json!({"description": d}).to_string());
        conn.execute(
            "UPDATE gd_documents SET metadata = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            duckdb::params![metadata, doc_id],
        )
        .context("Failed to update document metadata")?;
    }

    if old.document.base_template != new.document.base_template {
        conn.execute(
            "UPDATE gd_documents SET base_template = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            duckdb::params![new.document.base_template, doc_id],
        )
        .context("Failed to update document base_template")?;
    }

    if old.document.language != new.document.language {
        conn.execute(
            "UPDATE gd_documents SET language = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            duckdb::params![new.document.language, doc_id],
        )
        .context("Failed to update document language")?;
    }

    // Handle sections
    let old_ids: std::collections::HashSet<_> = old.sections.iter().map(|s| &s.id).collect();
    let new_ids: std::collections::HashSet<_> = new.sections.iter().map(|s| &s.id).collect();

    // Delete removed sections (and their edges)
    for id in old_ids.difference(&new_ids) {
        conn.execute(
            "DELETE FROM gd_edges WHERE source_id = ? OR target_id = ?",
            duckdb::params![*id, *id],
        )
        .context("Failed to delete section edges")?;
        conn.execute("DELETE FROM gd_sections WHERE id = ?", [*id])
            .context("Failed to delete section")?;
    }

    // Add/update sections
    for section in &new.sections {
        if old_ids.contains(&section.id) {
            // Update existing (gd_sections doesn't have updated_at column)
            conn.execute(
                r#"UPDATE gd_sections
                   SET section_type = ?, level = ?, order_idx = ?, content = ?, source_section = ?
                   WHERE id = ?"#,
                duckdb::params![
                    section.section_type,
                    section.level.map(|l| l as i64),
                    section.order as i64,
                    section.content,
                    section.override_section,
                    section.id
                ],
            )
            .context("Failed to update section")?;
        } else {
            // Insert new
            conn.execute(
                r#"INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content, source_section)
                   VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                duckdb::params![
                    section.id,
                    doc_id,
                    section.section_type,
                    section.level.map(|l| l as i64),
                    section.order as i64,
                    section.content,
                    section.override_section
                ],
            )
            .context("Failed to insert section")?;
        }
    }

    // Handle variables
    let old_names: std::collections::HashSet<_> = old.variables.iter().map(|v| &v.name).collect();
    let new_names: std::collections::HashSet<_> = new.variables.iter().map(|v| &v.name).collect();

    // Delete removed variables
    for name in old_names.difference(&new_names) {
        conn.execute(
            "DELETE FROM gd_variables WHERE document_id = ? AND name = ?",
            duckdb::params![doc_id, *name],
        )
        .context("Failed to delete variable")?;
    }

    // Add/update variables
    for var in &new.variables {
        let value_str = serde_json::to_string(&var.value)?;

        if old_names.contains(&var.name) {
            conn.execute(
                r#"UPDATE gd_variables
                   SET value = ?, var_type = ?, updated_at = CURRENT_TIMESTAMP
                   WHERE document_id = ? AND name = ?"#,
                duckdb::params![value_str, var.var_type, doc_id, var.name],
            )
            .context("Failed to update variable")?;
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                r#"INSERT INTO gd_variables (id, document_id, name, value, var_type)
                   VALUES (?, ?, ?, ?, ?)"#,
                duckdb::params![id, doc_id, var.name, value_str, var.var_type],
            )
            .context("Failed to insert variable")?;
        }
    }

    Ok(())
}

/// Open a DuckAgentFS instance from an ID or path, auto-creating if needed
pub async fn open_duckagentfs(id_or_path: &str) -> Result<DuckAgentFS> {
    let (path, auto_created) = if id_or_path == ":memory:" {
        (":memory:".to_string(), false)
    } else if std::path::Path::new(id_or_path).exists() {
        (id_or_path.to_string(), false)
    } else {
        // Validate agent ID: alphanumeric, hyphens, underscores only
        let is_valid_id = !id_or_path.is_empty()
            && id_or_path
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_');

        if !is_valid_id {
            anyhow::bail!(
                "Invalid agent ID '{}': must contain only alphanumeric characters, hyphens, and underscores",
                id_or_path
            );
        }

        // Try as agent ID
        let agentfs_dir = agentfs_sdk::agentfs_dir();
        let db_path = agentfs_dir.join(format!("{}.duckdb", id_or_path));

        if db_path.exists() {
            (db_path.to_string_lossy().to_string(), false)
        } else {
            // Database doesn't exist - will be auto-created
            // Ensure .agentfs directory exists
            if !agentfs_dir.exists() {
                std::fs::create_dir_all(agentfs_dir)
                    .with_context(|| format!("Failed to create directory: {agentfs_dir:?}"))?;
            }
            (db_path.to_string_lossy().to_string(), true)
        }
    };

    if auto_created {
        eprintln!("info: Creating new DuckDB database: {}", path);
    }

    let config = DuckAgentFSConfig {
        path,
        ..Default::default()
    };

    DuckAgentFS::open(config)
        .await
        .context("Failed to open DuckAgentFS database")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    async fn setup_test_fs() -> DuckAgentFS {
        let config = DuckAgentFSConfig {
            path: ":memory:".to_string(),
            ..Default::default()
        };
        DuckAgentFS::open(config)
            .await
            .expect("Failed to open test fs")
    }

    fn write_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write temp file");
        file
    }

    #[tokio::test]
    async fn test_import_single_file() {
        let fs = setup_test_fs().await;
        let temp_file = write_temp_file("# Test\n\nContent");

        let args = ImportArgs {
            file: temp_file.path().to_path_buf(),
            id: Some("test-doc".into()),
            title: None,
            template: None,
            llm: false,
            model: "".into(),
            dry_run: false,
        };

        handle_import(&fs, args).await.unwrap();

        // Verify
        let conn = fs.get_connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM gd_sections WHERE document_id = 'test-doc'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_dry_run() {
        let fs = setup_test_fs().await;
        let temp_file = write_temp_file("# Test\n\nContent");

        let args = ImportArgs {
            file: temp_file.path().to_path_buf(),
            id: Some("test-doc".into()),
            title: None,
            template: None,
            llm: false,
            model: "".into(),
            dry_run: true, // Dry run
        };

        handle_import(&fs, args).await.unwrap();

        // Verify nothing was inserted
        let conn = fs.get_connection().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM gd_documents WHERE id = 'test-doc')",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert!(!exists);
    }

    #[tokio::test]
    async fn test_import_with_template() {
        let fs = setup_test_fs().await;

        // Create template first
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES ('template', 'Template')",
            [],
        )
        .unwrap();
        drop(conn);

        let temp_file = write_temp_file("# Child\n\nContent");

        let args = ImportArgs {
            file: temp_file.path().to_path_buf(),
            id: Some("child".into()),
            title: None,
            template: Some("template".into()),
            llm: false,
            model: "".into(),
            dry_run: false,
        };

        handle_import(&fs, args).await.unwrap();

        // Verify inheritance
        let conn = fs.get_connection().unwrap();
        let base: Option<String> = conn
            .query_row(
                "SELECT base_template FROM gd_documents WHERE id = 'child'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(base, Some("template".to_string()));
    }

    #[tokio::test]
    async fn test_import_extracts_variables() {
        let fs = setup_test_fs().await;
        let temp_file = write_temp_file("# Hello {{name}}\n\nWelcome to {{project}}!");

        let args = ImportArgs {
            file: temp_file.path().to_path_buf(),
            id: Some("var-doc".into()),
            title: None,
            template: None,
            llm: false,
            model: "".into(),
            dry_run: false,
        };

        handle_import(&fs, args).await.unwrap();

        // Verify variables were extracted
        let conn = fs.get_connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM gd_variables WHERE document_id = 'var-doc'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_export_document() {
        let fs = setup_test_fs().await;

        // Create a document with sections
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES ('export-test', 'Export Test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'export-test', 'heading', 1, 0, 'Test Title')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s2', 'export-test', 'paragraph', NULL, 1, 'Test content')",
            [],
        )
        .unwrap();
        drop(conn);

        let args = ExportArgs {
            doc_id: "export-test".into(),
            output: None,
        };

        // This should not error
        handle_export(&fs, args).await.unwrap();
    }

    #[test]
    fn test_substitute_variables() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("name".to_string(), "\"World\"".to_string());
        vars.insert("count".to_string(), "42".to_string());

        let result = substitute_variables("Hello {{name}}! Count: {{count}}", &vars);
        assert_eq!(result, "Hello World! Count: 42");
    }

    #[tokio::test]
    async fn test_import_duplicate_error() {
        let fs = setup_test_fs().await;
        let temp_file = write_temp_file("# Test\n\nContent");

        let args = ImportArgs {
            file: temp_file.path().to_path_buf(),
            id: Some("duplicate-doc".into()),
            title: None,
            template: None,
            llm: false,
            model: "".into(),
            dry_run: false,
        };

        // First import should succeed
        handle_import(&fs, args.clone()).await.unwrap();

        // Create a new temp file for second import
        let temp_file2 = write_temp_file("# Test 2\n\nMore content");
        let args2 = ImportArgs {
            file: temp_file2.path().to_path_buf(),
            id: Some("duplicate-doc".into()),
            title: None,
            template: None,
            llm: false,
            model: "".into(),
            dry_run: false,
        };

        // Second import should fail
        let result = handle_import(&fs, args2).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    // ==========================================================================
    // Tests for new CLI commands: Create, List, Show, Delete, AddSection, etc.
    // ==========================================================================

    #[tokio::test]
    async fn test_create_document() {
        let fs = setup_test_fs().await;

        let args = CreateArgs {
            doc_id: "my-doc".into(),
            title: "My Document".into(),
            template: None,
            language: "en".into(),
            description: Some("A test document".into()),
        };

        handle_create(&fs, args).await.unwrap();

        // Verify document was created
        let conn = fs.get_connection().unwrap();
        let (title, metadata): (String, Option<String>) = conn
            .query_row(
                "SELECT title, metadata FROM gd_documents WHERE id = 'my-doc'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        assert_eq!(title, "My Document");
        // Description is stored in metadata JSON
        assert!(metadata.is_some());
        assert!(metadata.unwrap().contains("A test document"));
    }

    #[tokio::test]
    async fn test_create_document_with_template() {
        let fs = setup_test_fs().await;

        // Create template first
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES ('base-template', 'Base Template')",
            [],
        )
        .unwrap();
        drop(conn);

        let args = CreateArgs {
            doc_id: "child-doc".into(),
            title: "Child Document".into(),
            template: Some("base-template".into()),
            language: "en".into(),
            description: None,
        };

        handle_create(&fs, args).await.unwrap();

        // Verify inheritance
        let conn = fs.get_connection().unwrap();
        let base: Option<String> = conn
            .query_row(
                "SELECT base_template FROM gd_documents WHERE id = 'child-doc'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(base, Some("base-template".to_string()));
    }

    #[tokio::test]
    async fn test_create_document_duplicate_error() {
        let fs = setup_test_fs().await;

        let args = CreateArgs {
            doc_id: "dup-doc".into(),
            title: "First".into(),
            template: None,
            language: "en".into(),
            description: None,
        };

        handle_create(&fs, args).await.unwrap();

        // Try to create same doc again
        let args2 = CreateArgs {
            doc_id: "dup-doc".into(),
            title: "Second".into(),
            template: None,
            language: "en".into(),
            description: None,
        };

        let result = handle_create(&fs, args2).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_add_section() {
        let fs = setup_test_fs().await;

        // Create document first
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES ('section-test', 'Section Test')",
            [],
        )
        .unwrap();
        drop(conn);

        let args = AddSectionArgs {
            doc_id: "section-test".into(),
            section_type: SectionType::Heading,
            content: "Hello World".into(),
            level: Some(1),
            position: "end".into(),
            override_section: None,
        };

        handle_add_section(&fs, args).await.unwrap();

        // Verify section was added
        let conn = fs.get_connection().unwrap();
        let (section_type, level, content): (String, Option<i64>, String) = conn
            .query_row(
                "SELECT section_type, level, content FROM gd_sections WHERE document_id = 'section-test'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();

        assert_eq!(section_type, "heading");
        assert_eq!(level, Some(1));
        assert_eq!(content, "Hello World");
    }

    #[tokio::test]
    async fn test_add_section_multiple_ordered() {
        let fs = setup_test_fs().await;

        // Create document first
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES ('order-test', 'Order Test')",
            [],
        )
        .unwrap();
        drop(conn);

        // Add first section
        handle_add_section(
            &fs,
            AddSectionArgs {
                doc_id: "order-test".into(),
                section_type: SectionType::Heading,
                content: "First".into(),
                level: Some(1),
                position: "end".into(),
                override_section: None,
            },
        )
        .await
        .unwrap();

        // Add second section
        handle_add_section(
            &fs,
            AddSectionArgs {
                doc_id: "order-test".into(),
                section_type: SectionType::Paragraph,
                content: "Second".into(),
                level: None,
                position: "end".into(),
                override_section: None,
            },
        )
        .await
        .unwrap();

        // Verify order
        let conn = fs.get_connection().unwrap();
        let mut stmt = conn
            .prepare("SELECT order_idx, content FROM gd_sections WHERE document_id = 'order-test' ORDER BY order_idx")
            .unwrap();
        let sections: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<duckdb::Result<Vec<_>>>()
            .unwrap();

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0], (0, "First".to_string()));
        assert_eq!(sections[1], (1, "Second".to_string()));
    }

    #[tokio::test]
    async fn test_delete_document() {
        let fs = setup_test_fs().await;

        // Create document with sections
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES ('delete-test', 'Delete Test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gd_sections (id, document_id, section_type, order_idx, content) VALUES ('s1', 'delete-test', 'paragraph', 0, 'content')",
            [],
        )
        .unwrap();
        drop(conn);

        // Delete with force
        let args = DeleteArgs {
            doc_id: "delete-test".into(),
            force: true,
        };

        handle_delete(&fs, args).await.unwrap();

        // Verify deleted
        let conn = fs.get_connection().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM gd_documents WHERE id = 'delete-test')",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert!(!exists);
    }

    #[tokio::test]
    async fn test_set_and_get_variable() {
        let fs = setup_test_fs().await;

        // Create document first
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES ('var-test', 'Var Test')",
            [],
        )
        .unwrap();
        drop(conn);

        // Set a string variable
        handle_set_var(
            &fs,
            SetVarArgs {
                doc_id: "var-test".into(),
                name: "project_name".into(),
                value: "My Project".into(),
            },
        )
        .await
        .unwrap();

        // Set a JSON array variable
        handle_set_var(
            &fs,
            SetVarArgs {
                doc_id: "var-test".into(),
                name: "features".into(),
                value: r#"["auth", "api"]"#.into(),
            },
        )
        .await
        .unwrap();

        // Verify using engine
        let engine = GraphDocsEngine::new(fs.pool());
        let vars = engine.get_variables("var-test").await.unwrap();

        assert_eq!(
            vars.get("project_name"),
            Some(&serde_json::Value::String("My Project".into()))
        );
        assert_eq!(
            vars.get("features"),
            Some(&serde_json::json!(["auth", "api"]))
        );
    }

    #[tokio::test]
    async fn test_remove_section() {
        let fs = setup_test_fs().await;

        // Create document with section
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES ('remove-test', 'Remove Test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gd_sections (id, document_id, section_type, order_idx, content) VALUES ('section-to-remove', 'remove-test', 'paragraph', 0, 'content')",
            [],
        )
        .unwrap();
        drop(conn);

        // Remove section
        let args = RemoveSectionArgs {
            doc_id: "remove-test".into(),
            section_id: "section-to-remove".into(),
        };

        handle_remove_section(&fs, args).await.unwrap();

        // Verify removed
        let conn = fs.get_connection().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM gd_sections WHERE document_id = 'remove-test'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_truncate_function() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
        assert_eq!(truncate("hi", 2), "hi");
        assert_eq!(truncate("hello", 5), "hello");
    }

    // ==========================================================================
    // Tests for Edit command: validate_document, diff_documents, load_document,
    //                         apply_changes
    // ==========================================================================

    fn create_test_editable_doc(title: &str, version: &str) -> EditableDocument {
        EditableDocument {
            document: DocumentMeta {
                id: "test".into(),
                title: title.into(),
                description: Some("Test description".into()),
                base_template: None,
                language: "en".into(),
            },
            sections: vec![EditableSection {
                id: "s1".into(),
                section_type: "heading".into(),
                level: Some(1),
                order: 0,
                content: format!("Version {}", version),
                override_section: None,
            }],
            variables: vec![EditableVariable {
                name: "version".into(),
                value: serde_json::Value::String(version.into()),
                var_type: "string".into(),
            }],
        }
    }

    #[test]
    fn test_validate_document_valid() {
        let doc = create_test_editable_doc("Test", "1.0");
        assert!(validate_document(&doc).is_ok());
    }

    #[test]
    fn test_validate_document_invalid_section_type() {
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
                section_type: "invalid_type".into(),
                level: None,
                order: 0,
                content: "test".into(),
                override_section: None,
            }],
            variables: vec![],
        };

        let result = validate_document(&doc);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid section type"));
    }

    #[test]
    fn test_validate_document_invalid_heading_level() {
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
                section_type: "heading".into(),
                level: Some(7), // Invalid: max is 6
                order: 0,
                content: "test".into(),
                override_section: None,
            }],
            variables: vec![],
        };

        let result = validate_document(&doc);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid heading level"));
    }

    #[test]
    fn test_validate_document_invalid_variable_type() {
        let doc = EditableDocument {
            document: DocumentMeta {
                id: "test".into(),
                title: "Test".into(),
                description: None,
                base_template: None,
                language: "en".into(),
            },
            sections: vec![],
            variables: vec![EditableVariable {
                name: "var1".into(),
                value: serde_json::Value::String("test".into()),
                var_type: "invalid_type".into(),
            }],
        };

        let result = validate_document(&doc);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid variable type"));
    }

    #[test]
    fn test_validate_document_duplicate_section_ids() {
        let doc = EditableDocument {
            document: DocumentMeta {
                id: "test".into(),
                title: "Test".into(),
                description: None,
                base_template: None,
                language: "en".into(),
            },
            sections: vec![
                EditableSection {
                    id: "same-id".into(),
                    section_type: "paragraph".into(),
                    level: None,
                    order: 0,
                    content: "first".into(),
                    override_section: None,
                },
                EditableSection {
                    id: "same-id".into(),
                    section_type: "paragraph".into(),
                    level: None,
                    order: 1,
                    content: "second".into(),
                    override_section: None,
                },
            ],
            variables: vec![],
        };

        let result = validate_document(&doc);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Duplicate section ID"));
    }

    #[test]
    fn test_validate_document_duplicate_variable_names() {
        let doc = EditableDocument {
            document: DocumentMeta {
                id: "test".into(),
                title: "Test".into(),
                description: None,
                base_template: None,
                language: "en".into(),
            },
            sections: vec![],
            variables: vec![
                EditableVariable {
                    name: "same-name".into(),
                    value: serde_json::Value::String("value1".into()),
                    var_type: "string".into(),
                },
                EditableVariable {
                    name: "same-name".into(),
                    value: serde_json::Value::String("value2".into()),
                    var_type: "string".into(),
                },
            ],
        };

        let result = validate_document(&doc);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Duplicate variable name"));
    }

    #[test]
    fn test_diff_documents_no_changes() {
        let doc = create_test_editable_doc("Test", "1.0");
        let changes = diff_documents(&doc, &doc);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_diff_documents_title_changed() {
        let old = create_test_editable_doc("Old Title", "1.0");
        let new = create_test_editable_doc("New Title", "1.0");
        let changes = diff_documents(&old, &new);

        assert!(!changes.is_empty());
        assert!(changes.iter().any(|c| c.contains("Title")));
    }

    #[test]
    fn test_diff_documents_variable_changed() {
        let old = create_test_editable_doc("Test", "1.0");
        let new = create_test_editable_doc("Test", "2.0");
        let changes = diff_documents(&old, &new);

        assert!(!changes.is_empty());
        // Should detect both section content change and variable change
        assert!(changes
            .iter()
            .any(|c| c.contains("version") || c.contains("section")));
    }

    #[test]
    fn test_diff_documents_section_added() {
        let old = create_test_editable_doc("Test", "1.0");
        let mut new = old.clone();
        new.sections.push(EditableSection {
            id: "new-section".into(),
            section_type: "paragraph".into(),
            level: None,
            order: 1,
            content: "New content".into(),
            override_section: None,
        });

        let changes = diff_documents(&old, &new);
        assert!(changes.iter().any(|c| c.contains("Add section")));
    }

    #[test]
    fn test_diff_documents_section_removed() {
        let old = create_test_editable_doc("Test", "1.0");
        let mut new = old.clone();
        new.sections.clear();

        let changes = diff_documents(&old, &new);
        assert!(changes.iter().any(|c| c.contains("Remove section")));
    }

    #[test]
    fn test_diff_documents_variable_added() {
        let old = create_test_editable_doc("Test", "1.0");
        let mut new = old.clone();
        new.variables.push(EditableVariable {
            name: "new_var".into(),
            value: serde_json::Value::String("new_value".into()),
            var_type: "string".into(),
        });

        let changes = diff_documents(&old, &new);
        assert!(changes.iter().any(|c| c.contains("Add variable")));
    }

    #[tokio::test]
    async fn test_load_document_for_edit() {
        let fs = setup_test_fs().await;

        // Create a document with sections and variables
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title, language) VALUES ('edit-test', 'Edit Test', 'en')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gd_sections (id, document_id, section_type, level, order_idx, content) VALUES ('s1', 'edit-test', 'heading', 1, 0, 'Test Heading')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES ('v1', 'edit-test', 'author', '"John"', 'string')"#,
            [],
        )
        .unwrap();
        drop(conn);

        let doc = load_document_for_edit(&fs, "edit-test").unwrap();

        assert_eq!(doc.document.id, "edit-test");
        assert_eq!(doc.document.title, "Edit Test");
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].section_type, "heading");
        assert_eq!(doc.sections[0].level, Some(1));
        assert_eq!(doc.variables.len(), 1);
        assert_eq!(doc.variables[0].name, "author");
    }

    #[tokio::test]
    async fn test_apply_changes() {
        let fs = setup_test_fs().await;

        // Create initial document with a section and variable
        let conn = fs.get_write_connection().unwrap();
        conn.execute(
            "INSERT INTO gd_documents (id, title, language) VALUES ('apply-test', 'Original Title', 'en')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gd_sections (id, document_id, section_type, order_idx, content) VALUES ('s1', 'apply-test', 'paragraph', 0, 'Original content')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO gd_variables (id, document_id, name, value, var_type) VALUES ('v1', 'apply-test', 'existing_var', '"old_value"', 'string')"#,
            [],
        )
        .unwrap();
        drop(conn);

        let old = load_document_for_edit(&fs, "apply-test").unwrap();
        assert_eq!(old.sections.len(), 1);
        assert_eq!(old.variables.len(), 1);

        // Create modified version - modify title, section content, and variable value
        let mut new = old.clone();
        new.document.title = "Modified Title".into();
        new.sections[0].content = "Modified content".into();
        new.variables[0].value = serde_json::Value::String("new_value".into());

        // Apply changes
        apply_changes(&fs, "apply-test", &old, &new).unwrap();

        // Verify changes
        let updated = load_document_for_edit(&fs, "apply-test").unwrap();
        assert_eq!(updated.document.title, "Modified Title");
        assert_eq!(updated.sections[0].content, "Modified content");
        assert_eq!(updated.variables.len(), 1);
        assert_eq!(
            updated.variables[0].value,
            serde_json::Value::String("new_value".into())
        );
    }

    #[test]
    fn test_editable_document_yaml_roundtrip() {
        let doc = create_test_editable_doc("Test", "1.0");

        let yaml = serde_yaml::to_string(&doc).unwrap();
        let parsed: EditableDocument = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(doc, parsed);
    }

    #[test]
    fn test_editable_document_toml_roundtrip() {
        let doc = create_test_editable_doc("Test", "1.0");

        let toml_str = toml::to_string_pretty(&doc).unwrap();
        let parsed: EditableDocument = toml::from_str(&toml_str).unwrap();

        assert_eq!(doc, parsed);
    }
}
