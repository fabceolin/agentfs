//! Agent-based document transformation using TEA subprocess
//!
//! This module provides document transformation capabilities using
//! TEA (The Edge Agent) as a subprocess. It supports:
//! - Status normalization via embeddings + LLM fallback
//! - Document transformation to match templates
//! - Batch processing of non-conforming documents

use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use anyhow::{Result, anyhow, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::parser::{MarkdownParser, ParsedDocument, SectionType};
use super::normalizer::ExtendedStatus;

/// Conformance result for transformation (simplified version for agent use)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceResult {
    pub file_path: String,
    pub template_path: Option<String>,
    pub is_conformant: bool,
    pub missing_sections: Vec<String>,
    pub extra_sections: Vec<String>,
    pub type_mismatches: Vec<String>,
    pub suggestions: Vec<String>,
}

/// Agent transformer using TEA subprocess
pub struct AgentTransformer {
    tea_binary: String,
    agents_dir: PathBuf,
    model_path: Option<PathBuf>,
}

impl AgentTransformer {
    /// Create new transformer
    pub fn new(agents_dir: PathBuf) -> Self {
        Self {
            tea_binary: std::env::var("TEA_BINARY").unwrap_or_else(|_| "tea".to_string()),
            agents_dir,
            model_path: None,
        }
    }

    /// Set custom model path
    pub fn with_model_path(mut self, path: PathBuf) -> Self {
        self.model_path = Some(path);
        self
    }

    /// Check if TEA is available
    pub async fn check_tea_available(&self) -> Result<bool> {
        let output = Command::new(&self.tea_binary)
            .arg("--version")
            .output()
            .await;

        match output {
            Ok(o) => Ok(o.status.success()),
            Err(_) => Ok(false),
        }
    }

    /// Run TEA agent with input
    async fn run_agent(&self, agent_file: &str, input: Value) -> Result<Value> {
        let agent_path = self.agents_dir.join(agent_file);

        if !agent_path.exists() {
            return Err(anyhow!("Agent file not found: {}", agent_path.display()));
        }

        let mut cmd = Command::new(&self.tea_binary);
        cmd.arg("run")
           .arg(&agent_path)
           .arg("--input")
           .arg(input.to_string())
           .stdout(Stdio::piped())
           .stderr(Stdio::piped());

        // Set model path env var if specified
        if let Some(ref model_path) = self.model_path {
            cmd.env("GGUF_MODEL_PATH", model_path.display().to_string());
        }

        let output = cmd.output().await
            .context("Failed to execute TEA")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("TEA agent failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&stdout)
            .context("Failed to parse TEA output as JSON")
    }

    /// Normalize status using TEA agent
    pub async fn normalize_status(&self, raw_status: &str) -> Result<ExtendedStatus> {
        let input = json!({
            "raw_status": raw_status
        });

        let result = self.run_agent("document-conformance-agent.yaml", input).await?;

        let status_str = result
            .get("normalized_status")
            .and_then(|v| v.as_str())
            .unwrap_or("Draft");

        Ok(match status_str {
            "Draft" => ExtendedStatus::Draft,
            "Approved" => ExtendedStatus::Approved,
            "InProgress" => ExtendedStatus::InProgress,
            "Review" => ExtendedStatus::Review,
            "Done" => ExtendedStatus::Done,
            _ => ExtendedStatus::Draft,
        })
    }

    /// Transform document to conform to template
    pub async fn transform_to_template(
        &self,
        doc: &ParsedDocument,
        template: &ParsedDocument,
        conformance: &ConformanceResult,
    ) -> Result<String> {
        let input = json!({
            "document": self.doc_to_json(doc),
            "template": self.doc_to_json(template),
            "conformance": {
                "missing_sections": conformance.missing_sections,
                "type_mismatches": conformance.type_mismatches,
            }
        });

        let result = self.run_agent("document-transformer-agent.yaml", input).await?;

        let content = result
            .get("output_content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            // Fallback to rule-based if LLM returns empty
            return self.transform_rule_based(doc, template, conformance);
        }

        Ok(content)
    }

    /// Convert ParsedDocument to JSON for agent input
    fn doc_to_json(&self, doc: &ParsedDocument) -> Value {
        let sections: Vec<Value> = doc.sections.iter().map(|s| {
            json!({
                "id": s.id,
                "section_type": s.section_type.as_str(),
                "level": s.level,
                "content": s.content,
                "order_idx": s.order_idx,
            })
        }).collect();

        json!({
            "title": doc.title,
            "sections": sections,
            "variables": doc.variables,
        })
    }

    /// Rule-based transformation fallback
    pub fn transform_rule_based(
        &self,
        doc: &ParsedDocument,
        template: &ParsedDocument,
        _conformance: &ConformanceResult,
    ) -> Result<String> {
        use std::collections::HashMap;

        let mut output = String::new();

        // Add title if present
        if let Some(title) = &doc.title {
            output.push_str(&format!("# {}\n\n", title));
        }

        // Build map of document sections by name
        let doc_sections: HashMap<String, _> = doc.sections
            .iter()
            .filter(|s| s.section_type == SectionType::Heading)
            .map(|s| (s.content.to_lowercase(), s))
            .collect();

        // Follow template structure
        for template_section in &template.sections {
            if template_section.section_type != SectionType::Heading {
                continue;
            }

            let section_name = template_section.content.to_lowercase();
            let level = template_section.level.unwrap_or(2);
            let prefix = "#".repeat(level as usize);

            if let Some(doc_section) = doc_sections.get(&section_name) {
                // Use existing content
                output.push_str(&format!("{} {}\n\n", prefix, doc_section.content));
            } else {
                // Add placeholder for missing section
                output.push_str(&format!("{} {}\n\n<!-- TODO: Add content -->\n\n",
                    prefix, template_section.content));
            }
        }

        Ok(output)
    }
}

/// CLI arguments for conform command
#[derive(Debug, Clone)]
pub struct ConformArgs {
    pub dir: PathBuf,
    pub model_path: Option<PathBuf>,
    pub agents_dir: Option<PathBuf>,
    pub dry_run: bool,
}

/// Result of transformation
#[derive(Debug)]
pub struct TransformResult {
    pub file_path: String,
    pub original_issues: usize,
    pub transformed: bool,
    pub dry_run: bool,
    pub new_content: Option<String>,
}

/// Batch transform all non-conforming documents
pub async fn batch_transform(args: &ConformArgs) -> Result<Vec<TransformResult>> {
    use super::conformance::{scan_directory, TemplateManager};

    let agents_dir = args.agents_dir.clone()
        .unwrap_or_else(|| PathBuf::from("agents"));

    let mut transformer = AgentTransformer::new(agents_dir);
    if let Some(ref model_path) = args.model_path {
        transformer = transformer.with_model_path(model_path.clone());
    }

    // Check TEA availability
    if !transformer.check_tea_available().await? {
        return Err(anyhow!(
            "TEA binary not found. Install with: cargo install --path /path/to/tea --features llm-local"
        ));
    }

    let conformance_results = scan_directory(&args.dir).await?;
    let mut results = Vec::new();

    // Load template
    let template_path = TemplateManager::detect_template(&args.dir)
        .ok_or_else(|| anyhow!("No template found in directory"))?;

    let template_content = tokio::fs::read_to_string(&template_path).await?;
    let template = MarkdownParser::new().parse(&template_content)?;

    for conformance in conformance_results {
        if !conformance.is_conformant {
            let doc_content = tokio::fs::read_to_string(&conformance.file_path).await?;
            let doc = MarkdownParser::new().parse(&doc_content)?;

            // Convert BmadConformanceResult to our ConformanceResult
            let simple_conformance = ConformanceResult {
                file_path: conformance.file_path.clone(),
                template_path: Some(conformance.template_path.clone()),
                is_conformant: conformance.is_conformant,
                missing_sections: conformance.missing_sections.iter()
                    .map(|s| s.section_title.clone())
                    .collect(),
                extra_sections: vec![],
                type_mismatches: conformance.type_violations.iter()
                    .map(|v| v.section_title.clone())
                    .collect(),
                suggestions: conformance.suggestions.iter()
                    .map(|s| s.description.clone())
                    .collect(),
            };

            let transformed = transformer.transform_to_template(
                &doc,
                &template,
                &simple_conformance,
            ).await?;

            if !args.dry_run {
                tokio::fs::write(&conformance.file_path, &transformed).await?;
            }

            results.push(TransformResult {
                file_path: conformance.file_path,
                original_issues: conformance.suggestions.len(),
                transformed: true,
                dry_run: args.dry_run,
                new_content: if args.dry_run { Some(transformed) } else { None },
            });
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_tea_available() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"));
        // This will pass if TEA is installed, return false otherwise
        let _ = transformer.check_tea_available().await;
    }

    #[tokio::test]
    #[ignore] // Requires TEA + model
    async fn test_normalize_status_agent() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"));
        let status = transformer.normalize_status("[**Done**]").await.unwrap();
        assert_eq!(status, ExtendedStatus::Done);
    }

    #[test]
    fn test_rule_based_transform() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"));
        let template = MarkdownParser::new().parse("# Template\n## Status\n## Description").unwrap();
        let doc = MarkdownParser::new().parse("# My Doc\n## Status\nDone").unwrap();
        let conformance = ConformanceResult {
            file_path: String::new(),
            template_path: None,
            is_conformant: false,
            missing_sections: vec!["description".to_string()],
            extra_sections: vec![],
            type_mismatches: vec![],
            suggestions: vec![],
        };

        let result = transformer.transform_rule_based(&doc, &template, &conformance).unwrap();
        assert!(result.contains("## Description"));
        assert!(result.contains("<!-- TODO: Add content -->"));
    }

    #[tokio::test]
    async fn test_dry_run() {
        let dir = tempdir().unwrap();

        // Create template
        tokio::fs::write(
            dir.path().join("story-tmpl.md"),
            "# {{title}}\n## Status\n## Description"
        ).await.unwrap();

        // Create non-conforming doc
        tokio::fs::write(
            dir.path().join("story-1.md"),
            "# Story 1\n## Status\nDone"
        ).await.unwrap();

        let args = ConformArgs {
            dir: dir.path().to_path_buf(),
            model_path: None,
            agents_dir: Some(PathBuf::from("agents")),
            dry_run: true,
        };

        // This would fail without TEA, but tests the struct construction
        assert!(args.dry_run);
        assert!(args.agents_dir.is_some());
    }

    #[test]
    fn test_agent_transformer_new() {
        let transformer = AgentTransformer::new(PathBuf::from("test_agents"));
        assert_eq!(transformer.agents_dir, PathBuf::from("test_agents"));
        assert!(transformer.model_path.is_none());
    }

    #[test]
    fn test_agent_transformer_with_model_path() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"))
            .with_model_path(PathBuf::from("/path/to/model.gguf"));
        assert_eq!(transformer.model_path, Some(PathBuf::from("/path/to/model.gguf")));
    }

    #[test]
    fn test_conform_args() {
        let args = ConformArgs {
            dir: PathBuf::from("/test/dir"),
            model_path: Some(PathBuf::from("/model.gguf")),
            agents_dir: Some(PathBuf::from("/agents")),
            dry_run: true,
        };
        assert_eq!(args.dir, PathBuf::from("/test/dir"));
        assert!(args.dry_run);
    }

    #[test]
    fn test_transform_result() {
        let result = TransformResult {
            file_path: "test.md".to_string(),
            original_issues: 3,
            transformed: true,
            dry_run: false,
            new_content: None,
        };
        assert_eq!(result.original_issues, 3);
        assert!(result.transformed);
    }

    #[test]
    fn test_conformance_result() {
        let result = ConformanceResult {
            file_path: "doc.md".to_string(),
            template_path: Some("template.md".to_string()),
            is_conformant: false,
            missing_sections: vec!["Description".to_string()],
            extra_sections: vec![],
            type_mismatches: vec![],
            suggestions: vec!["Add Description section".to_string()],
        };
        assert!(!result.is_conformant);
        assert_eq!(result.missing_sections.len(), 1);
    }

    #[test]
    fn test_doc_to_json() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"));
        let doc = MarkdownParser::new().parse("# Test\n\nContent").unwrap();
        let json = transformer.doc_to_json(&doc);

        assert!(json.get("title").is_some());
        assert!(json.get("sections").is_some());
        assert!(json.get("variables").is_some());
    }

    #[test]
    fn test_rule_based_transform_with_title() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"));
        let template = MarkdownParser::new().parse("# Template\n## Section1\n## Section2").unwrap();
        let doc = MarkdownParser::new().parse("# My Document\n## Section1\nContent here").unwrap();
        let conformance = ConformanceResult {
            file_path: String::new(),
            template_path: None,
            is_conformant: false,
            missing_sections: vec!["section2".to_string()],
            extra_sections: vec![],
            type_mismatches: vec![],
            suggestions: vec![],
        };

        let result = transformer.transform_rule_based(&doc, &template, &conformance).unwrap();
        assert!(result.starts_with("# My Document"));
        assert!(result.contains("## Section1"));
        assert!(result.contains("## Section2"));
        assert!(result.contains("<!-- TODO: Add content -->"));
    }

    #[test]
    fn test_rule_based_transform_no_title() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"));
        let template = MarkdownParser::new().parse("## Status\n## Tasks").unwrap();
        let doc = MarkdownParser::new().parse("## Status\nDone").unwrap();
        let conformance = ConformanceResult {
            file_path: String::new(),
            template_path: None,
            is_conformant: false,
            missing_sections: vec!["tasks".to_string()],
            extra_sections: vec![],
            type_mismatches: vec![],
            suggestions: vec![],
        };

        let result = transformer.transform_rule_based(&doc, &template, &conformance).unwrap();
        assert!(result.contains("## Status"));
        assert!(result.contains("## Tasks"));
    }
}
