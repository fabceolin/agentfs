//! GraphDocs CLI commands for importing and exporting Markdown documents
//!
//! This module provides CLI handlers for:
//! - `agentfs graphdocs import <file.md>` - Import a single Markdown file
//! - `agentfs graphdocs import-dir <dir>` - Import all Markdown files from a directory
//! - `agentfs graphdocs export <doc_id>` - Export a GraphDoc to Markdown
//! - `agentfs graphdocs conform <dir>` - Check and fix document conformance using TEA

use agentfs_sdk::filesystem::duckagentfs::{DuckAgentFS, DuckAgentFSConfig};
use agentfs_sdk::graphdocs::{
    batch_transform, ConformArgs as SdkConformArgs, LLMConverter, MarkdownParser, OpenAIClient,
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
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
    /// Import a Markdown file as GraphDoc
    Import(ImportArgs),

    /// Import all Markdown files from a directory
    ImportDir(ImportDirArgs),

    /// Export a GraphDoc to Markdown file
    Export(ExportArgs),

    /// Check and fix document conformance using TEA agents
    Conform(ConformArgs),
}

#[derive(Args, Debug)]
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
        let api_key = std::env::var("OPENAI_API_KEY")
            .context("OPENAI_API_KEY not set for --llm mode")?;
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
    let conn = fs.pool.get_write_connection()?;

    // Check if document already exists
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM gd_documents WHERE id = ?)",
            [&doc_id],
            |r| r.get(0),
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

    let files: Vec<PathBuf> = glob::glob(&pattern)?
        .filter_map(|r| r.ok())
        .collect();

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
    let conn = fs.pool.get_connection()?;

    // Get document info
    let (title, base_template): (String, Option<String>) = conn
        .query_row(
            "SELECT title, base_template FROM gd_documents WHERE id = ?",
            [&args.doc_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .with_context(|| format!("Document '{}' not found", args.doc_id))?;

    // Get sections ordered by order_idx
    let mut stmt = conn.prepare(
        r#"SELECT section_type, level, content
           FROM gd_sections
           WHERE document_id = ?
           ORDER BY order_idx"#,
    )?;

    let sections = stmt
        .query_map([&args.doc_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Get variables for substitution
    let mut var_stmt = conn.prepare(
        "SELECT name, value FROM gd_variables WHERE document_id = ?",
    )?;

    let variables: std::collections::HashMap<String, String> = var_stmt
        .query_map([&args.doc_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Build markdown
    let mut markdown = String::new();

    // Add metadata comment if there's a base template
    if let Some(template) = base_template {
        markdown.push_str(&format!("<!-- Base template: {} -->\n\n", template));
    }

    for (section_type, level, content) in sections {
        let rendered_content = substitute_variables(&content, &variables);

        match section_type.as_str() {
            "heading" => {
                let level = level.unwrap_or(1) as usize;
                let prefix = "#".repeat(level);
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
fn substitute_variables(content: &str, variables: &std::collections::HashMap<String, String>) -> String {
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

/// Open a DuckAgentFS instance from an ID or path
pub async fn open_duckagentfs(id_or_path: &str) -> Result<DuckAgentFS> {
    let path = if id_or_path == ":memory:" {
        ":memory:".to_string()
    } else if std::path::Path::new(id_or_path).exists() {
        id_or_path.to_string()
    } else {
        // Try as agent ID
        let agentfs_dir = agentfs_sdk::agentfs_dir();
        let db_path = agentfs_dir.join(format!("{}.duckdb", id_or_path));
        if db_path.exists() {
            db_path.to_string_lossy().to_string()
        } else {
            anyhow::bail!("Database not found: {} (tried {} and {:?})", id_or_path, id_or_path, db_path);
        }
    };

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
        DuckAgentFS::open(config).await.expect("Failed to open test fs")
    }

    fn write_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes()).expect("Failed to write temp file");
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
        let conn = fs.pool.get_connection().unwrap();
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
        let conn = fs.pool.get_connection().unwrap();
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
        let conn = fs.pool.get_write_connection().unwrap();
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
        let conn = fs.pool.get_connection().unwrap();
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
        let conn = fs.pool.get_connection().unwrap();
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
        let conn = fs.pool.get_write_connection().unwrap();
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
}
