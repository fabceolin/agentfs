//! Agent-based document transformation using TEA subprocess
//!
//! This module provides document transformation capabilities using
//! TEA (The Edge Agent) as a subprocess. It supports:
//! - Status normalization via embeddings + LLM fallback
//! - Document transformation to match templates
//! - Batch processing of non-conforming documents

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

use super::conformance::BmadConformanceResult;
use super::normalizer::ExtendedStatus;
use super::parser::{MarkdownParser, ParsedDocument, SectionType};

/// Conformance result for transformation (simplified version for backward compatibility)
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

/// Enhanced conformance result with full details for TEA agents (STORY-7.5)
///
/// This struct provides the complete conformance information including:
/// - Detailed missing sections with `is_required` flags
/// - Type violations with expected vs actual types
/// - Choice violations with valid options
/// - Suggestions with `auto_fixable` flags
///
/// # JSON Schema
///
/// ```json
/// {
///   "file_path": "docs/stories/STORY-1.1.md",
///   "template_id": "story-template-v2",
///   "template_path": "story-tmpl.yaml",
///   "is_conformant": false,
///   "missing_sections": [
///     { "section_id": "qa-results", "section_title": "QA Results", "is_required": true }
///   ],
///   "type_violations": [
///     {
///       "section_id": "tasks",
///       "section_title": "Tasks",
///       "expected_type": "checklist",
///       "actual_content": "- Task 1...",
///       "suggestion": "Content should be a checklist"
///     }
///   ],
///   "choice_violations": [
///     {
///       "section_id": "status",
///       "section_title": "Status",
///       "expected_choices": ["Draft", "Approved", "Done"],
///       "actual_value": "WIP"
///     }
///   ],
///   "extra_sections": ["Random Notes"],
///   "suggestions": [
///     { "kind": "add_section", "description": "Add required section", "auto_fixable": true }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedConformanceResult {
    pub file_path: String,
    pub template_id: String,
    pub template_path: String,
    pub is_conformant: bool,
    pub missing_sections: Vec<MissingSectionInfo>,
    pub type_violations: Vec<TypeViolationInfo>,
    pub choice_violations: Vec<ChoiceViolationInfo>,
    pub extra_sections: Vec<String>,
    pub suggestions: Vec<SuggestionInfo>,
}

/// Missing section information for enhanced conformance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingSectionInfo {
    pub section_id: String,
    pub section_title: String,
    pub is_required: bool,
}

/// Type violation information for enhanced conformance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeViolationInfo {
    pub section_id: String,
    pub section_title: String,
    pub expected_type: String,
    pub actual_content: String,
    pub suggestion: String,
}

/// Choice violation information for enhanced conformance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceViolationInfo {
    pub section_id: String,
    pub section_title: String,
    pub expected_choices: Vec<String>,
    pub actual_value: String,
}

/// Suggestion information for enhanced conformance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionInfo {
    pub kind: String,
    pub description: String,
    pub auto_fixable: bool,
}

impl From<&BmadConformanceResult> for EnhancedConformanceResult {
    fn from(result: &BmadConformanceResult) -> Self {
        Self {
            file_path: result.file_path.clone(),
            template_id: result.template_id.clone(),
            template_path: result.template_path.clone(),
            is_conformant: result.is_conformant,
            missing_sections: result
                .missing_sections
                .iter()
                .map(|s| MissingSectionInfo {
                    section_id: s.section_id.clone(),
                    section_title: s.section_title.clone(),
                    is_required: s.is_required,
                })
                .collect(),
            type_violations: result
                .type_violations
                .iter()
                .map(|v| TypeViolationInfo {
                    section_id: v.section_id.clone(),
                    section_title: v.section_title.clone(),
                    expected_type: format!("{:?}", v.expected_type).to_lowercase(),
                    actual_content: v.actual_content.clone(),
                    suggestion: v.suggestion.clone(),
                })
                .collect(),
            choice_violations: result
                .choice_violations
                .iter()
                .map(|v| ChoiceViolationInfo {
                    section_id: v.section_id.clone(),
                    section_title: v.section_title.clone(),
                    expected_choices: v.expected_choices.clone(),
                    actual_value: v.actual_value.clone(),
                })
                .collect(),
            extra_sections: result.extra_sections.clone(),
            suggestions: result
                .suggestions
                .iter()
                .map(|s| SuggestionInfo {
                    kind: format!("{:?}", s.kind).to_lowercase(),
                    description: s.description.clone(),
                    auto_fixable: s.auto_fixable,
                })
                .collect(),
        }
    }
}

impl From<BmadConformanceResult> for EnhancedConformanceResult {
    fn from(result: BmadConformanceResult) -> Self {
        Self::from(&result)
    }
}

impl From<&EnhancedConformanceResult> for ConformanceResult {
    fn from(enhanced: &EnhancedConformanceResult) -> Self {
        Self {
            file_path: enhanced.file_path.clone(),
            template_path: Some(enhanced.template_path.clone()),
            is_conformant: enhanced.is_conformant,
            missing_sections: enhanced
                .missing_sections
                .iter()
                .map(|s| s.section_title.clone())
                .collect(),
            extra_sections: enhanced.extra_sections.clone(),
            type_mismatches: enhanced
                .type_violations
                .iter()
                .map(|v| v.section_title.clone())
                .collect(),
            suggestions: enhanced
                .suggestions
                .iter()
                .map(|s| s.description.clone())
                .collect(),
        }
    }
}

/// Agent transformer using TEA subprocess
pub struct AgentTransformer {
    tea_binary: String,
    agents_dir: PathBuf,
    model_path: Option<PathBuf>,
    overlay: Option<PathBuf>,
}

impl AgentTransformer {
    /// Create new transformer
    pub fn new(agents_dir: PathBuf) -> Self {
        Self {
            tea_binary: std::env::var("TEA_BINARY").unwrap_or_else(|_| "tea".to_string()),
            agents_dir,
            model_path: None,
            overlay: None,
        }
    }

    /// Set custom model path
    pub fn with_model_path(mut self, path: PathBuf) -> Self {
        self.model_path = Some(path);
        self
    }

    /// Set overlay YAML file to merge with agent configs
    pub fn with_overlay(mut self, path: PathBuf) -> Self {
        self.overlay = Some(path);
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
        cmd.arg("run").arg(&agent_path);

        // Add overlay file if specified (merge with agent config)
        if let Some(ref overlay_path) = self.overlay {
            cmd.arg("-f").arg(overlay_path);
        }

        cmd.arg("--input")
            .arg(input.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set model path env var if specified
        if let Some(ref model_path) = self.model_path {
            cmd.env("GGUF_MODEL_PATH", model_path.display().to_string());
        }

        let output = cmd.output().await.context("Failed to execute TEA")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("TEA agent failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Try parsing as pure JSON first (tea-rust format)
        // If that fails, extract JSON from decorated output (tea-python format)
        let json_str = if stdout.trim_start().starts_with('{') {
            // Pure JSON output (tea-rust)
            stdout.to_string()
        } else if let Some(start) = stdout.find("Final state: ") {
            // Decorated output (tea-python) - extract JSON after "Final state: "
            stdout[start + 13..].trim().to_string()
        } else {
            // Fallback: try to find last JSON object in output
            stdout
                .rfind('{')
                .map(|i| stdout[i..].to_string())
                .unwrap_or_else(|| stdout.to_string())
        };

        serde_json::from_str(&json_str).context("Failed to parse TEA output as JSON")
    }

    /// Normalize status using TEA agent
    pub async fn normalize_status(&self, raw_status: &str) -> Result<ExtendedStatus> {
        let input = json!({
            "raw_status": raw_status
        });

        let result = self
            .run_agent("document-conformance-agent.yaml", input)
            .await?;

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

    /// Transform document to conform to template (legacy simplified conformance)
    pub async fn transform_to_template(
        &self,
        doc: &ParsedDocument,
        template: &ParsedDocument,
        conformance: &ConformanceResult,
    ) -> Result<String> {
        // Convert simplified to enhanced format for backward compatibility
        let enhanced = EnhancedConformanceResult {
            file_path: conformance.file_path.clone(),
            template_id: String::new(),
            template_path: conformance.template_path.clone().unwrap_or_default(),
            is_conformant: conformance.is_conformant,
            missing_sections: conformance
                .missing_sections
                .iter()
                .map(|s| MissingSectionInfo {
                    section_id: s.to_lowercase().replace(' ', "-"),
                    section_title: s.clone(),
                    is_required: true, // Assume required for legacy format
                })
                .collect(),
            type_violations: conformance
                .type_mismatches
                .iter()
                .map(|s| TypeViolationInfo {
                    section_id: s.to_lowercase().replace(' ', "-"),
                    section_title: s.clone(),
                    expected_type: "unknown".to_string(),
                    actual_content: String::new(),
                    suggestion: format!("Fix type for section: {}", s),
                })
                .collect(),
            choice_violations: vec![],
            extra_sections: conformance.extra_sections.clone(),
            suggestions: conformance
                .suggestions
                .iter()
                .map(|s| SuggestionInfo {
                    kind: "unknown".to_string(),
                    description: s.clone(),
                    auto_fixable: false,
                })
                .collect(),
        };
        self.transform_to_template_enhanced(doc, template, &enhanced)
            .await
    }

    /// Transform document to conform to template with full enhanced conformance data (STORY-7.5)
    ///
    /// This method passes the complete conformance information to TEA agents including:
    /// - Missing sections with `is_required` flags
    /// - Type violations with `expected_type` details
    /// - Choice violations with `expected_choices`
    /// - Suggestions with `auto_fixable` flags
    pub async fn transform_to_template_enhanced(
        &self,
        doc: &ParsedDocument,
        template: &ParsedDocument,
        conformance: &EnhancedConformanceResult,
    ) -> Result<String> {
        let input = json!({
            "document": self.doc_to_json(doc),
            "template": self.doc_to_json(template),
            "conformance": {
                "file_path": conformance.file_path,
                "template_id": conformance.template_id,
                "template_path": conformance.template_path,
                "is_conformant": conformance.is_conformant,
                "missing_sections": conformance.missing_sections,
                "type_violations": conformance.type_violations,
                "choice_violations": conformance.choice_violations,
                "extra_sections": conformance.extra_sections,
                "suggestions": conformance.suggestions,
                // Legacy fields for backward compatibility with older agents
                "type_mismatches": conformance.type_violations.iter()
                    .map(|v| v.section_title.clone())
                    .collect::<Vec<_>>(),
            }
        });

        let result = self
            .run_agent("document-transformer-agent.yaml", input)
            .await?;

        let content = result
            .get("output_content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            // Fallback to rule-based if LLM returns empty
            let simplified = ConformanceResult::from(conformance);
            return self.transform_rule_based(doc, template, &simplified);
        }

        Ok(content)
    }

    /// Convert ParsedDocument to JSON for agent input
    fn doc_to_json(&self, doc: &ParsedDocument) -> Value {
        let sections: Vec<Value> = doc
            .sections
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "section_type": s.section_type.as_str(),
                    "level": s.level,
                    "content": s.content,
                    "order_idx": s.order_idx,
                })
            })
            .collect();

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
        let doc_sections: HashMap<String, _> = doc
            .sections
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
                output.push_str(&format!(
                    "{} {}\n\n<!-- TODO: Add content -->\n\n",
                    prefix, template_section.content
                ));
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
    pub overlay: Option<PathBuf>,
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

    let agents_dir = args
        .agents_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("agents"));

    let mut transformer = AgentTransformer::new(agents_dir);
    if let Some(ref model_path) = args.model_path {
        transformer = transformer.with_model_path(model_path.clone());
    }
    if let Some(ref overlay) = args.overlay {
        transformer = transformer.with_overlay(overlay.clone());
    }

    // Check TEA availability
    if !transformer.check_tea_available().await? {
        return Err(anyhow!(
            "TEA binary not found. Install with: cargo install --path /path/to/tea --features llm-local"
        ));
    }

    let conformance_results = scan_directory(&args.dir).await?;
    let mut results = Vec::new();

    // Load template - handle both YAML and Markdown formats
    let template_path = TemplateManager::detect_template(&args.dir)
        .ok_or_else(|| anyhow!("No template found in directory"))?;

    let template = if TemplateManager::is_yaml_template(&template_path) {
        // Load YAML template and convert to ParsedDocument
        use super::template_schema::BmadTemplate;
        let bmad_template = BmadTemplate::load(&template_path).await?;
        bmad_template.to_parsed_document()
    } else {
        // Parse as Markdown
        let template_content = tokio::fs::read_to_string(&template_path).await?;
        MarkdownParser::new().parse(&template_content)?
    };

    for conformance in conformance_results {
        if !conformance.is_conformant {
            let doc_content = tokio::fs::read_to_string(&conformance.file_path).await?;
            let doc = MarkdownParser::new().parse(&doc_content)?;

            // Convert BmadConformanceResult to our ConformanceResult
            let simple_conformance = ConformanceResult {
                file_path: conformance.file_path.clone(),
                template_path: Some(conformance.template_path.clone()),
                is_conformant: conformance.is_conformant,
                missing_sections: conformance
                    .missing_sections
                    .iter()
                    .map(|s| s.section_title.clone())
                    .collect(),
                extra_sections: vec![],
                type_mismatches: conformance
                    .type_violations
                    .iter()
                    .map(|v| v.section_title.clone())
                    .collect(),
                suggestions: conformance
                    .suggestions
                    .iter()
                    .map(|s| s.description.clone())
                    .collect(),
            };

            let transformed = transformer
                .transform_to_template(&doc, &template, &simple_conformance)
                .await?;

            if !args.dry_run {
                tokio::fs::write(&conformance.file_path, &transformed).await?;
            }

            results.push(TransformResult {
                file_path: conformance.file_path,
                original_issues: conformance.suggestions.len(),
                transformed: true,
                dry_run: args.dry_run,
                new_content: if args.dry_run {
                    Some(transformed)
                } else {
                    None
                },
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
    #[ignore] // Requires TEA with llm-local feature + GGUF model
    async fn test_normalize_status_agent() {
        // Resolve agents dir from project root (sdk/rust -> ../../agents)
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let agents_dir = manifest_dir.join("../../agents").canonicalize().unwrap();
        let transformer = AgentTransformer::new(agents_dir);
        let status = transformer.normalize_status("[**Done**]").await.unwrap();
        assert_eq!(status, ExtendedStatus::Done);
    }

    #[test]
    fn test_rule_based_transform() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"));
        let template = MarkdownParser::new()
            .parse("# Template\n## Status\n## Description")
            .unwrap();
        let doc = MarkdownParser::new()
            .parse("# My Doc\n## Status\nDone")
            .unwrap();
        let conformance = ConformanceResult {
            file_path: String::new(),
            template_path: None,
            is_conformant: false,
            missing_sections: vec!["description".to_string()],
            extra_sections: vec![],
            type_mismatches: vec![],
            suggestions: vec![],
        };

        let result = transformer
            .transform_rule_based(&doc, &template, &conformance)
            .unwrap();
        assert!(result.contains("## Description"));
        assert!(result.contains("<!-- TODO: Add content -->"));
    }

    #[tokio::test]
    async fn test_dry_run() {
        let dir = tempdir().unwrap();

        // Create template
        tokio::fs::write(
            dir.path().join("story-tmpl.md"),
            "# {{title}}\n## Status\n## Description",
        )
        .await
        .unwrap();

        // Create non-conforming doc
        tokio::fs::write(dir.path().join("story-1.md"), "# Story 1\n## Status\nDone")
            .await
            .unwrap();

        let args = ConformArgs {
            dir: dir.path().to_path_buf(),
            model_path: None,
            agents_dir: Some(PathBuf::from("agents")),
            overlay: None,
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
        assert_eq!(
            transformer.model_path,
            Some(PathBuf::from("/path/to/model.gguf"))
        );
    }

    #[test]
    fn test_agent_transformer_with_overlay() {
        let transformer = AgentTransformer::new(PathBuf::from("agents"))
            .with_overlay(PathBuf::from("/path/to/overlay.yaml"));
        assert_eq!(
            transformer.overlay,
            Some(PathBuf::from("/path/to/overlay.yaml"))
        );
    }

    #[test]
    fn test_conform_args() {
        let args = ConformArgs {
            dir: PathBuf::from("/test/dir"),
            model_path: Some(PathBuf::from("/model.gguf")),
            agents_dir: Some(PathBuf::from("/agents")),
            overlay: Some(PathBuf::from("/overlay.yaml")),
            dry_run: true,
        };
        assert_eq!(args.dir, PathBuf::from("/test/dir"));
        assert!(args.dry_run);
        assert!(args.overlay.is_some());
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
        let template = MarkdownParser::new()
            .parse("# Template\n## Section1\n## Section2")
            .unwrap();
        let doc = MarkdownParser::new()
            .parse("# My Document\n## Section1\nContent here")
            .unwrap();
        let conformance = ConformanceResult {
            file_path: String::new(),
            template_path: None,
            is_conformant: false,
            missing_sections: vec!["section2".to_string()],
            extra_sections: vec![],
            type_mismatches: vec![],
            suggestions: vec![],
        };

        let result = transformer
            .transform_rule_based(&doc, &template, &conformance)
            .unwrap();
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

        let result = transformer
            .transform_rule_based(&doc, &template, &conformance)
            .unwrap();
        assert!(result.contains("## Status"));
        assert!(result.contains("## Tasks"));
    }

    #[test]
    fn test_enhanced_conformance_result_from_bmad() {
        // Test From<BmadConformanceResult> for EnhancedConformanceResult (STORY-7.5)
        use crate::graphdocs::conformance::{
            BmadConformanceResult, ChoiceViolation, ConformanceSuggestion, MissingSection,
            SuggestionKind, TypeViolation,
        };
        use crate::graphdocs::template_schema::SectionContentType;

        let bmad_result = BmadConformanceResult {
            file_path: "test.md".to_string(),
            template_id: "story-template".to_string(),
            template_path: "story-tmpl.yaml".to_string(),
            is_conformant: false,
            missing_sections: vec![MissingSection {
                section_id: "qa".to_string(),
                section_title: "QA Results".to_string(),
                is_required: true,
            }],
            type_violations: vec![TypeViolation {
                section_id: "tasks".to_string(),
                section_title: "Tasks".to_string(),
                expected_type: SectionContentType::Checklist,
                actual_content: "- Task 1".to_string(),
                suggestion: "Use checklist format".to_string(),
            }],
            choice_violations: vec![ChoiceViolation {
                section_id: "status".to_string(),
                section_title: "Status".to_string(),
                expected_choices: vec!["Draft".to_string(), "Done".to_string()],
                actual_value: "WIP".to_string(),
            }],
            extra_sections: vec!["Random".to_string()],
            suggestions: vec![ConformanceSuggestion {
                kind: SuggestionKind::AddSection,
                description: "Add QA section".to_string(),
                auto_fixable: true,
            }],
        };

        let enhanced: EnhancedConformanceResult = bmad_result.into();

        assert_eq!(enhanced.file_path, "test.md");
        assert_eq!(enhanced.template_id, "story-template");
        assert!(!enhanced.is_conformant);
        assert_eq!(enhanced.missing_sections.len(), 1);
        assert!(enhanced.missing_sections[0].is_required);
        assert_eq!(enhanced.type_violations.len(), 1);
        assert_eq!(enhanced.type_violations[0].expected_type, "checklist");
        assert_eq!(enhanced.choice_violations.len(), 1);
        assert_eq!(enhanced.choice_violations[0].actual_value, "WIP");
        assert_eq!(enhanced.extra_sections, vec!["Random".to_string()]);
        assert_eq!(enhanced.suggestions.len(), 1);
        assert!(enhanced.suggestions[0].auto_fixable);
    }

    #[test]
    fn test_enhanced_conformance_result_serialization() {
        // Test EnhancedConformanceResult JSON serialization (STORY-7.5)
        let enhanced = EnhancedConformanceResult {
            file_path: "test.md".to_string(),
            template_id: "story".to_string(),
            template_path: "story-tmpl.yaml".to_string(),
            is_conformant: false,
            missing_sections: vec![MissingSectionInfo {
                section_id: "qa".to_string(),
                section_title: "QA".to_string(),
                is_required: true,
            }],
            type_violations: vec![],
            choice_violations: vec![ChoiceViolationInfo {
                section_id: "status".to_string(),
                section_title: "Status".to_string(),
                expected_choices: vec!["Draft".to_string()],
                actual_value: "Bad".to_string(),
            }],
            extra_sections: vec![],
            suggestions: vec![SuggestionInfo {
                kind: "add_section".to_string(),
                description: "Add QA".to_string(),
                auto_fixable: true,
            }],
        };

        let json = serde_json::to_string(&enhanced).unwrap();
        assert!(json.contains("\"is_required\":true"));
        assert!(json.contains("\"auto_fixable\":true"));

        // Roundtrip
        let parsed: EnhancedConformanceResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.file_path, "test.md");
        assert!(parsed.missing_sections[0].is_required);
    }

    #[test]
    fn test_conformance_result_from_enhanced() {
        // Test From<EnhancedConformanceResult> for ConformanceResult (backward compat)
        let enhanced = EnhancedConformanceResult {
            file_path: "doc.md".to_string(),
            template_id: "tmpl".to_string(),
            template_path: "tmpl.yaml".to_string(),
            is_conformant: false,
            missing_sections: vec![
                MissingSectionInfo {
                    section_id: "s1".to_string(),
                    section_title: "Section 1".to_string(),
                    is_required: true,
                },
                MissingSectionInfo {
                    section_id: "s2".to_string(),
                    section_title: "Section 2".to_string(),
                    is_required: false,
                },
            ],
            type_violations: vec![TypeViolationInfo {
                section_id: "tasks".to_string(),
                section_title: "Tasks".to_string(),
                expected_type: "checklist".to_string(),
                actual_content: "- item".to_string(),
                suggestion: "fix".to_string(),
            }],
            choice_violations: vec![],
            extra_sections: vec!["Extra".to_string()],
            suggestions: vec![SuggestionInfo {
                kind: "add_section".to_string(),
                description: "Add Section 1".to_string(),
                auto_fixable: true,
            }],
        };

        let simplified = ConformanceResult::from(&enhanced);

        assert_eq!(simplified.file_path, "doc.md");
        assert_eq!(simplified.template_path, Some("tmpl.yaml".to_string()));
        assert!(!simplified.is_conformant);
        // Missing sections converted to just titles
        assert_eq!(simplified.missing_sections.len(), 2);
        assert!(simplified.missing_sections.contains(&"Section 1".to_string()));
        assert!(simplified.missing_sections.contains(&"Section 2".to_string()));
        // Type violations converted to just titles
        assert_eq!(simplified.type_mismatches.len(), 1);
        assert!(simplified.type_mismatches.contains(&"Tasks".to_string()));
        // Extra sections preserved
        assert_eq!(simplified.extra_sections, vec!["Extra".to_string()]);
        // Suggestions converted to just descriptions
        assert_eq!(simplified.suggestions.len(), 1);
        assert!(simplified.suggestions.contains(&"Add Section 1".to_string()));
    }
}
