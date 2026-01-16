# STORY-2.1.3: Template Conformance and Status Normalization

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.1.3 |
| **Parent** | STORY-2.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 2 - Parsing and Population |
| **Status** | Done |
| **Priority** | High |
| **Files** | `sdk/rust/src/graphdocs/conformance.rs`, `normalizer.rs`, `embedding_matcher.rs` |
| **Dependencies** | STORY-2.1.1, STORY-2.1.2 |

## User Story

**As a** developer
**I want** documents in a directory to conform to templates and have normalized statuses
**So that** I can ensure consistency across project documentation

## Acceptance Criteria

- [x] Generate edge structure from parsed sections
- [x] Detect templates in directories (`*-tmpl.yaml`, `*-tmpl.md`, etc.) with **priority for YAML format**
- [x] Parse BMAD YAML template format (`template:`, `sections:[]` structure)
- [x] Validate document conformance against detected templates
- [x] Validate document sections against template `sections[].type` (choice, bullet-list, table, numbered-list, template-text, etc.)
- [x] Map variant status markers to standard enum values using embeddings

## Problem Statement

Documents in a directory may have inconsistent formatting:
```
[Done] TD.13.parallel-reliability-enhancement.md
[**Done**] TEA-AGENT-001.1-rust-multi-agent.md
[Complete] TEA-BUILTIN-002.1.web-actions.md
[Dev Complete] TEA-BUILTIN-008.6-llamaextract-async-polling.md
[In Progress (8/9 stories complete)] TEA-BUILTIN-008-epic.md
[**DONE** - All tasks complete] TEA-BUILTIN-001.4.long-term-memory.md
[Superseded → See TEA-BUILTIN-002.3] TEA-BUILTIN-014-epic.md
```

These need to be normalized to standard `StoryStatus` enum values.

## Template Format Specification

Templates use the **BMAD YAML Template Format**. Each directory can have one template file that defines the expected structure for all markdown documents in that directory.

### Template Detection Priority

1. `*-tmpl.yaml` (preferred - structured schema)
2. `*-template.yaml`
3. `template.yaml`
4. `_template.yaml`
5. `*-tmpl.md` (fallback - heading-based)
6. `*-template.md`

### BMAD Template Schema

```yaml
template:
  id: string           # Unique identifier
  name: string         # Human-readable name
  version: string      # Schema version (e.g., "2.0")
  output:
    format: markdown | yaml
    filename: string   # Pattern with {{variables}}
    title: string      # Document title pattern

workflow:              # Optional workflow configuration
  mode: interactive | non-interactive
  elicitation: string  # Elicitation task reference

agent_config:          # Optional agent permissions
  editable_sections: [] # Sections agents can modify

sections:              # Array of section definitions
  - id: string         # Section identifier (lowercase, snake_case)
    title: string      # Section heading text
    type: string       # Section content type (see below)
    required: bool     # Optional, default true
    choices: []        # For type: choice - allowed values
    columns: []        # For type: table - column headers
    template: string   # Content template with {{variables}}
    instruction: string # Guidance for content
    owner: string      # Primary owner agent
    editors: []        # Agents allowed to edit
    elicit: bool       # Requires user interaction
    sections: []       # Nested sub-sections (recursive)
```

### Section Types

| Type | Description | Validation |
|------|-------------|------------|
| `choice` | Single selection from `choices[]` | Value must be in choices array |
| `bullet-list` | Unordered list (`- item`) | Must be valid list structure |
| `numbered-list` | Ordered list (`1. item`) | Must be valid list structure |
| `checklist` | Task list (`- [ ] item`) | Must be checklist format |
| `table` | Markdown table | Must have `columns[]` headers |
| `template-text` | Structured text with variables | Variables must be present |
| `paragraphs` | Free-form paragraphs | No specific validation |
| `code` | Code block | Must be fenced code |
| `mermaid` | Mermaid diagram | Must be valid mermaid syntax |

### Example: story-tmpl.yaml

```yaml
template:
  id: story-template-v2
  name: Story Document
  version: 2.0
  output:
    format: markdown
    filename: docs/stories/{{epic_num}}.{{story_num}}.{{story_title_short}}.md

sections:
  - id: status
    title: Status
    type: choice
    choices: [Draft, Approved, InProgress, Review, Done]

  - id: story
    title: Story
    type: template-text
    template: |
      **As a** {{role}},
      **I want** {{action}},
      **so that** {{benefit}}

  - id: acceptance-criteria
    title: Acceptance Criteria
    type: numbered-list
    required: true

  - id: tasks
    title: Tasks / Subtasks
    type: checklist
    required: true

  - id: dev-notes
    title: Dev Notes
    type: paragraphs
    sections:
      - id: testing
        title: Testing
        type: bullet-list
```

## Technical Specification

### Status Normalization

```rust
// sdk/rust/src/graphdocs/normalizer.rs

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Known status patterns and their normalized values
#[derive(Debug, Clone)]
pub struct StatusPattern {
    pub pattern: Regex,
    pub normalized: StoryStatus,
    pub confidence: f32,
}

/// Status variants that map to standard enum
pub const STATUS_MAPPINGS: &[(&str, &str)] = &[
    // Done variants
    ("Done", "Done"),
    ("**Done**", "Done"),
    ("[Done]", "Done"),
    ("**DONE**", "Done"),
    ("DONE", "Done"),
    ("Complete", "Done"),
    ("Completed", "Done"),
    ("✅", "Done"),
    ("Done ✅", "Done"),

    // Dev Complete -> Review (needs QA)
    ("Dev Complete", "Review"),
    ("**Dev Complete**", "Review"),
    ("Development Complete", "Review"),

    // In Progress variants
    ("In Progress", "InProgress"),
    ("WIP", "InProgress"),
    ("Working", "InProgress"),

    // Superseded/Cancelled -> special handling
    ("Superseded", "Superseded"),
    ("Cancelled", "Cancelled"),
    ("Deprecated", "Deprecated"),

    // Draft variants
    ("Draft", "Draft"),
    ("TODO", "Draft"),
    ("Planned", "Draft"),

    // Optional/Experimental markers (keep Done status but flag)
    ("Optional", "Done"),  // with optional=true flag
    ("Experimental", "Done"),  // with experimental=true flag
];

/// Extended status enum to handle edge cases
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtendedStatus {
    // Standard statuses
    Draft,
    Approved,
    InProgress,
    Review,
    Done,
    // Extended statuses
    Superseded { see_also: Option<String> },
    Cancelled,
    Deprecated,
}

/// Parsed status from document title/header
#[derive(Debug, Clone)]
pub struct ParsedStatus {
    pub status: ExtendedStatus,
    pub raw_text: String,
    pub optional: bool,
    pub experimental: bool,
    pub progress: Option<ProgressInfo>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProgressInfo {
    pub completed: u32,
    pub total: u32,
}

/// Extract and normalize status from document title
pub fn extract_status(title: &str) -> Option<ParsedStatus> {
    // Pattern: [STATUS] or [STATUS - notes] or [STATUS (progress)]
    let re = Regex::new(r"^\s*\[([^\]]+)\]").unwrap();

    if let Some(caps) = re.captures(title) {
        let raw = caps[1].to_string();
        let status_text = normalize_status_text(&raw);

        // Check for progress pattern like "(8/9 stories complete)"
        let progress = extract_progress(&raw);

        // Check for "See X" references
        let see_also = extract_see_also(&raw);

        // Check for optional/experimental flags
        let optional = raw.to_lowercase().contains("optional");
        let experimental = raw.to_lowercase().contains("experimental");

        // Map to extended status
        let status = map_to_status(&status_text, see_also);

        return Some(ParsedStatus {
            status,
            raw_text: raw,
            optional,
            experimental,
            progress,
            notes: extract_notes(&raw),
        });
    }
    None
}

fn normalize_status_text(raw: &str) -> String {
    // Remove markdown formatting
    let mut text = raw.replace("**", "");
    // Remove emoji
    text = text.replace("✅", "").replace("🔄", "").replace("⏳", "");
    // Extract first word/phrase before special chars
    if let Some(idx) = text.find(|c| c == '-' || c == '(' || c == '|') {
        text = text[..idx].to_string();
    }
    text.trim().to_string()
}

fn extract_progress(raw: &str) -> Option<ProgressInfo> {
    let re = Regex::new(r"\((\d+)/(\d+)").unwrap();
    re.captures(raw).map(|caps| ProgressInfo {
        completed: caps[1].parse().unwrap_or(0),
        total: caps[2].parse().unwrap_or(0),
    })
}

fn extract_see_also(raw: &str) -> Option<String> {
    let re = Regex::new(r"(?:See|→)\s*([\w\-\.]+)").unwrap();
    re.captures(raw).map(|caps| caps[1].to_string())
}

fn extract_notes(raw: &str) -> Option<String> {
    // Extract text after " - " that isn't a reference
    if let Some(idx) = raw.find(" - ") {
        let notes = raw[idx + 3..].trim();
        if !notes.starts_with("See") && !notes.starts_with("→") {
            return Some(notes.to_string());
        }
    }
    None
}

fn map_to_status(text: &str, see_also: Option<String>) -> ExtendedStatus {
    let lower = text.to_lowercase();

    if lower.contains("superseded") {
        return ExtendedStatus::Superseded { see_also };
    }
    if lower.contains("cancelled") || lower.contains("canceled") {
        return ExtendedStatus::Cancelled;
    }
    if lower.contains("deprecated") {
        return ExtendedStatus::Deprecated;
    }
    if lower.contains("done") || lower.contains("complete") && !lower.contains("dev") {
        return ExtendedStatus::Done;
    }
    if lower.contains("dev complete") || lower.contains("development complete") {
        return ExtendedStatus::Review;
    }
    if lower.contains("progress") || lower.contains("wip") {
        return ExtendedStatus::InProgress;
    }
    if lower.contains("approved") {
        return ExtendedStatus::Approved;
    }

    ExtendedStatus::Draft
}
```

### Embedding-Based Status Matching

For unknown status variants, use embeddings to find the closest match:

```rust
// sdk/rust/src/graphdocs/embedding_matcher.rs

use anyhow::Result;

/// Known status phrases with pre-computed embeddings
pub struct StatusEmbeddings {
    known_statuses: Vec<(String, Vec<f32>, ExtendedStatus)>,
}

impl StatusEmbeddings {
    /// Initialize with known status phrases
    pub fn new() -> Self {
        Self {
            known_statuses: Vec::new(),
        }
    }

    /// Pre-compute embeddings for known status phrases
    /// Uses TEA's memory.embed action via subprocess
    pub async fn initialize(&mut self) -> Result<()> {
        let known_phrases = vec![
            ("Done", ExtendedStatus::Done),
            ("Complete", ExtendedStatus::Done),
            ("Finished", ExtendedStatus::Done),
            ("Completed successfully", ExtendedStatus::Done),
            ("Development complete", ExtendedStatus::Review),
            ("Ready for review", ExtendedStatus::Review),
            ("In progress", ExtendedStatus::InProgress),
            ("Work in progress", ExtendedStatus::InProgress),
            ("Currently working", ExtendedStatus::InProgress),
            ("Draft", ExtendedStatus::Draft),
            ("Not started", ExtendedStatus::Draft),
            ("Planned", ExtendedStatus::Draft),
            ("Approved", ExtendedStatus::Approved),
            ("Ready for development", ExtendedStatus::Approved),
        ];

        for (phrase, status) in known_phrases {
            let embedding = self.embed_text(phrase).await?;
            self.known_statuses.push((phrase.to_string(), embedding, status));
        }

        Ok(())
    }

    /// Embed text using TEA's model2vec (via subprocess)
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        // Call TEA CLI for embedding
        let output = tokio::process::Command::new("tea")
            .args(["embed", "--model", "model2vec", "--text", text])
            .output()
            .await?;

        let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let embedding: Vec<f32> = result["embedding"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
    }

    /// Match unknown status text to closest known status
    pub async fn match_status(&self, text: &str) -> Result<(ExtendedStatus, f32)> {
        let embedding = self.embed_text(text).await?;

        let mut best_match = ExtendedStatus::Draft;
        let mut best_score = 0.0f32;

        for (_, known_emb, status) in &self.known_statuses {
            let score = cosine_similarity(&embedding, known_emb);
            if score > best_score {
                best_score = score;
                best_match = status.clone();
            }
        }

        Ok((best_match, best_score))
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
```

### BMAD Template Schema Parser

```rust
// sdk/rust/src/graphdocs/template_schema.rs

use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::Result;

/// BMAD Template Format
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BmadTemplate {
    pub template: TemplateMetadata,
    #[serde(default)]
    pub workflow: Option<WorkflowConfig>,
    #[serde(default)]
    pub agent_config: Option<AgentConfig>,
    pub sections: Vec<TemplateSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub filename: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Markdown,
    Yaml,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub elicitation: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentConfig {
    #[serde(default)]
    pub editable_sections: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateSection {
    pub id: String,
    pub title: String,
    #[serde(rename = "type", default)]
    pub section_type: SectionContentType,
    #[serde(default)]
    pub required: Option<bool>,  // None = true (default required)
    #[serde(default)]
    pub choices: Option<Vec<String>>,
    #[serde(default)]
    pub columns: Option<Vec<String>>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub editors: Option<Vec<String>>,
    #[serde(default)]
    pub elicit: Option<bool>,
    #[serde(default)]
    pub sections: Option<Vec<TemplateSection>>,  // Nested sections
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum SectionContentType {
    #[default]
    Paragraphs,
    Choice,
    BulletList,
    NumberedList,
    Checklist,
    Table,
    TemplateText,
    Code,
    Mermaid,
}

impl BmadTemplate {
    /// Load template from YAML file
    pub async fn load(path: &Path) -> Result<Self> {
        let content = tokio::fs::read_to_string(path).await?;
        let template: BmadTemplate = serde_yaml::from_str(&content)?;
        Ok(template)
    }

    /// Check if a section is required (default: true)
    pub fn is_section_required(section: &TemplateSection) -> bool {
        section.required.unwrap_or(true)
    }

    /// Get all section IDs (including nested) as flat list
    pub fn all_section_ids(&self) -> Vec<&str> {
        fn collect_ids<'a>(sections: &'a [TemplateSection], ids: &mut Vec<&'a str>) {
            for section in sections {
                ids.push(&section.id);
                if let Some(ref nested) = section.sections {
                    collect_ids(nested, ids);
                }
            }
        }
        let mut ids = Vec::new();
        collect_ids(&self.sections, &mut ids);
        ids
    }

    /// Get all section titles (including nested) as flat list
    pub fn all_section_titles(&self) -> Vec<&str> {
        fn collect_titles<'a>(sections: &'a [TemplateSection], titles: &mut Vec<&'a str>) {
            for section in sections {
                titles.push(&section.title);
                if let Some(ref nested) = section.sections {
                    collect_titles(nested, titles);
                }
            }
        }
        let mut titles = Vec::new();
        collect_titles(&self.sections, &mut titles);
        titles
    }
}
```

### Template Conformance Checking

```rust
// sdk/rust/src/graphdocs/conformance.rs

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use glob::glob;
use anyhow::Result;

use super::parser::{MarkdownParser, ParsedDocument, ParsedSection, SectionType};
use super::template_schema::{BmadTemplate, TemplateSection, SectionContentType};

/// Template detection patterns (YAML takes priority)
pub const TEMPLATE_PATTERNS: &[&str] = &[
    "*-tmpl.yaml",      // Priority 1: YAML templates
    "*-template.yaml",
    "template.yaml",
    "_template.yaml",
    "*-tmpl.md",        // Fallback: Markdown templates
    "*-template.md",
    "template.md",
    "_template.md",
];

/// Enhanced conformance result for BMAD templates
#[derive(Debug)]
pub struct BmadConformanceResult {
    pub file_path: String,
    pub template_id: String,
    pub template_path: String,
    pub is_conformant: bool,
    pub missing_sections: Vec<MissingSection>,
    pub type_violations: Vec<TypeViolation>,
    pub choice_violations: Vec<ChoiceViolation>,
    pub suggestions: Vec<ConformanceSuggestion>,
}

#[derive(Debug)]
pub struct MissingSection {
    pub section_id: String,
    pub section_title: String,
    pub is_required: bool,
}

#[derive(Debug)]
pub struct TypeViolation {
    pub section_id: String,
    pub section_title: String,
    pub expected_type: SectionContentType,
    pub actual_content: String,
    pub suggestion: String,
}

#[derive(Debug)]
pub struct ChoiceViolation {
    pub section_id: String,
    pub section_title: String,
    pub expected_choices: Vec<String>,
    pub actual_value: String,
}

#[derive(Debug)]
pub struct ConformanceSuggestion {
    pub kind: SuggestionKind,
    pub description: String,
    pub auto_fixable: bool,
}

#[derive(Debug)]
pub enum SuggestionKind {
    AddSection,
    RemoveSection,
    FixType,
    FixChoice,
    NormalizeStatus,
    ReorderSections,
}

/// Template manager supporting both BMAD YAML and markdown fallback
#[derive(Default)]
pub struct TemplateManager {
    bmad_templates: HashMap<PathBuf, BmadTemplate>,
    markdown_templates: HashMap<PathBuf, ParsedDocument>,
}

impl TemplateManager {
    /// Scan directory for template files (YAML takes priority)
    pub fn detect_template(dir: &Path) -> Option<PathBuf> {
        for pattern in TEMPLATE_PATTERNS {
            let search = dir.join(pattern);
            if let Some(search_str) = search.to_str() {
                if let Ok(mut paths) = glob(search_str) {
                    if let Some(Ok(path)) = paths.next() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    /// Check if path is a YAML template
    pub fn is_yaml_template(path: &Path) -> bool {
        path.extension()
            .map(|ext| ext == "yaml" || ext == "yml")
            .unwrap_or(false)
    }

    /// Load BMAD YAML template
    pub async fn load_bmad_template(&mut self, path: &Path) -> Result<&BmadTemplate> {
        if !self.bmad_templates.contains_key(path) {
            let template = BmadTemplate::load(path).await?;
            self.bmad_templates.insert(path.to_path_buf(), template);
        }
        Ok(self.bmad_templates.get(path).unwrap())
    }

    /// Load markdown template (fallback)
    pub async fn load_markdown_template(&mut self, path: &Path) -> Result<&ParsedDocument> {
        if !self.markdown_templates.contains_key(path) {
            let content = tokio::fs::read_to_string(path).await?;
            let parser = MarkdownParser::new();
            let doc = parser.parse(&content)?;
            self.markdown_templates.insert(path.to_path_buf(), doc);
        }
        Ok(self.markdown_templates.get(path).unwrap())
    }

    /// Check document conformance against BMAD template
    pub fn check_bmad_conformance(
        &self,
        doc: &ParsedDocument,
        template: &BmadTemplate,
        template_path: &Path,
    ) -> BmadConformanceResult {
        let mut result = BmadConformanceResult {
            file_path: String::new(),
            template_id: template.template.id.clone(),
            template_path: template_path.display().to_string(),
            is_conformant: true,
            missing_sections: Vec::new(),
            type_violations: Vec::new(),
            choice_violations: Vec::new(),
            suggestions: Vec::new(),
        };

        // Build map of document sections by normalized title
        let doc_sections: HashMap<String, &ParsedSection> = doc.sections
            .iter()
            .filter(|s| s.section_type == SectionType::Heading)
            .map(|s| (s.content.to_lowercase(), s))
            .collect();

        // Check each template section (including nested)
        self.check_sections_recursive(
            &template.sections,
            &doc_sections,
            doc,
            &mut result,
        );

        result
    }

    fn check_sections_recursive(
        &self,
        template_sections: &[TemplateSection],
        doc_sections: &HashMap<String, &ParsedSection>,
        doc: &ParsedDocument,
        result: &mut BmadConformanceResult,
    ) {
        for template_section in template_sections {
            let title_lower = template_section.title.to_lowercase();

            match doc_sections.get(&title_lower) {
                Some(doc_section) => {
                    // Section exists - validate type
                    self.validate_section_type(
                        doc_section,
                        template_section,
                        doc,
                        result,
                    );

                    // Validate choices if applicable
                    if template_section.section_type == SectionContentType::Choice {
                        self.validate_choice(
                            doc_section,
                            template_section,
                            doc,
                            result,
                        );
                    }
                }
                None => {
                    // Section missing
                    let is_required = BmadTemplate::is_section_required(template_section);
                    result.missing_sections.push(MissingSection {
                        section_id: template_section.id.clone(),
                        section_title: template_section.title.clone(),
                        is_required,
                    });
                    if is_required {
                        result.is_conformant = false;
                        result.suggestions.push(ConformanceSuggestion {
                            kind: SuggestionKind::AddSection,
                            description: format!(
                                "Add required section: ## {}",
                                template_section.title
                            ),
                            auto_fixable: true,
                        });
                    }
                }
            }

            // Recursively check nested sections
            if let Some(ref nested) = template_section.sections {
                self.check_sections_recursive(nested, doc_sections, doc, result);
            }
        }
    }

    fn validate_section_type(
        &self,
        doc_section: &ParsedSection,
        template_section: &TemplateSection,
        doc: &ParsedDocument,
        result: &mut BmadConformanceResult,
    ) {
        // Get content following this heading
        let section_content = self.get_section_content(doc_section, doc);

        let (type_matches, suggestion) = match template_section.section_type {
            SectionContentType::BulletList => (
                section_content.contains("- ") && !section_content.contains("- [ ]"),
                "Content should be a bullet list (- item)",
            ),
            SectionContentType::NumberedList => (
                section_content.lines().any(|l| l.trim().starts_with(|c: char| c.is_ascii_digit())),
                "Content should be a numbered list (1. item)",
            ),
            SectionContentType::Checklist => (
                section_content.contains("- [ ]") || section_content.contains("- [x]"),
                "Content should be a checklist (- [ ] item)",
            ),
            SectionContentType::Table => (
                section_content.contains("|") && section_content.contains("---"),
                "Content should be a markdown table",
            ),
            SectionContentType::Code => (
                section_content.contains("```"),
                "Content should be a fenced code block",
            ),
            SectionContentType::Mermaid => (
                section_content.contains("```mermaid"),
                "Content should be a mermaid diagram block",
            ),
            SectionContentType::Choice => (true, ""), // Validated separately
            SectionContentType::TemplateText => (true, ""), // Variables validated separately
            SectionContentType::Paragraphs => (true, ""), // No strict validation
        };

        if !type_matches && !suggestion.is_empty() {
            result.type_violations.push(TypeViolation {
                section_id: template_section.id.clone(),
                section_title: template_section.title.clone(),
                expected_type: template_section.section_type.clone(),
                actual_content: section_content.chars().take(100).collect(),
                suggestion: suggestion.to_string(),
            });
            result.is_conformant = false;
        }
    }

    fn validate_choice(
        &self,
        doc_section: &ParsedSection,
        template_section: &TemplateSection,
        doc: &ParsedDocument,
        result: &mut BmadConformanceResult,
    ) {
        if let Some(ref choices) = template_section.choices {
            let section_content = self.get_section_content(doc_section, doc);
            let value = section_content.trim();

            // Check if value matches any choice (case-insensitive)
            if !choices.iter().any(|c| c.eq_ignore_ascii_case(value)) {
                result.choice_violations.push(ChoiceViolation {
                    section_id: template_section.id.clone(),
                    section_title: template_section.title.clone(),
                    expected_choices: choices.clone(),
                    actual_value: value.to_string(),
                });
                result.is_conformant = false;
                result.suggestions.push(ConformanceSuggestion {
                    kind: SuggestionKind::FixChoice,
                    description: format!(
                        "Section '{}' value '{}' not in allowed choices: {:?}",
                        template_section.title, value, choices
                    ),
                    auto_fixable: false,
                });
            }
        }
    }

    /// Get content between this heading and the next heading
    fn get_section_content(&self, section: &ParsedSection, doc: &ParsedDocument) -> String {
        let section_idx = section.order_idx as usize;
        let mut content = String::new();

        for s in &doc.sections {
            if s.order_idx as usize > section_idx {
                if s.section_type == SectionType::Heading {
                    break; // Stop at next heading
                }
                content.push_str(&s.content);
                content.push('\n');
            }
        }

        content
    }
}

/// Scan all files in directory and check conformance
pub async fn scan_directory(dir: &Path) -> Result<Vec<BmadConformanceResult>> {
    let mut results = Vec::new();
    let mut manager = TemplateManager::default();

    // Detect template for this directory
    let template_path = match TemplateManager::detect_template(dir) {
        Some(path) => path,
        None => return Ok(results), // No template = no conformance checking
    };

    // Load template based on type
    let is_yaml = TemplateManager::is_yaml_template(&template_path);

    if is_yaml {
        let template = manager.load_bmad_template(&template_path).await?;

        // Scan all markdown files
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            // Only check markdown files
            if !path.extension().map(|e| e == "md").unwrap_or(false) {
                continue;
            }

            // Skip template files themselves
            if is_template_file(&path) {
                continue;
            }

            let content = tokio::fs::read_to_string(&path).await?;
            let parser = MarkdownParser::new();
            let doc = parser.parse(&content)?;

            let mut result = manager.check_bmad_conformance(&doc, template, &template_path);
            result.file_path = path.display().to_string();

            results.push(result);
        }
    }
    // Note: Markdown template fallback omitted for brevity - uses heading-based matching

    Ok(results)
}

fn is_template_file(path: &Path) -> bool {
    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    TEMPLATE_PATTERNS.iter().any(|pattern| {
        glob::Pattern::new(pattern)
            .map(|p| p.matches(filename))
            .unwrap_or(false)
    })
}
```

## Tests

### Test 1: Status Normalization
```rust
#[test]
fn test_status_normalization() {
    assert_eq!(extract_status("[Done]").unwrap().status, ExtendedStatus::Done);
    assert_eq!(extract_status("[**Done**]").unwrap().status, ExtendedStatus::Done);
    assert_eq!(extract_status("[Complete]").unwrap().status, ExtendedStatus::Done);
    assert_eq!(extract_status("[Dev Complete]").unwrap().status, ExtendedStatus::Review);
    assert_eq!(extract_status("[In Progress (8/9)]").unwrap().status, ExtendedStatus::InProgress);
    assert_eq!(extract_status("[Superseded → See TEA-001]").unwrap().status,
        ExtendedStatus::Superseded { see_also: Some("TEA-001".to_string()) });
}
```

### Test 2: Progress Extraction
```rust
#[test]
fn test_progress_extraction() {
    let status = extract_status("[In Progress (8/9 stories complete)]").unwrap();
    let progress = status.progress.unwrap();
    assert_eq!(progress.completed, 8);
    assert_eq!(progress.total, 9);
}
```

### Test 3: Optional/Experimental Flags
```rust
#[test]
fn test_optional_flags() {
    let status = extract_status("[**Done** | **Optional/Experimental**]").unwrap();
    assert_eq!(status.status, ExtendedStatus::Done);
    assert!(status.optional);
    assert!(status.experimental);
}
```

### Test 4: Template Detection
```rust
#[tokio::test]
async fn test_template_detection() {
    let dir = tempdir().unwrap();
    let template_path = dir.path().join("story-tmpl.md");
    tokio::fs::write(&template_path, "# {{story_title}}\n## Status\n## Description").await.unwrap();

    let detected = TemplateManager::detect_template(dir.path());
    assert!(detected.is_some());
    assert_eq!(detected.unwrap().file_name().unwrap(), "story-tmpl.md");
}
```

### Test 5: Conformance Check
```rust
#[tokio::test]
async fn test_conformance_check() {
    let template = MarkdownParser::new().parse("# Template\n## Status\n## Description\n## Tasks").unwrap();
    let doc = MarkdownParser::new().parse("# My Doc\n## Status\n## Notes").unwrap();

    let manager = TemplateManager::default();
    let result = manager.check_conformance(&doc, &template);

    assert!(!result.is_conformant);
    assert!(result.missing_sections.contains(&"description".to_string()));
    assert!(result.missing_sections.contains(&"tasks".to_string()));
}
```

### Test 6: Cosine Similarity
```rust
#[test]
fn test_cosine_similarity() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![1.0, 0.0, 0.0];
    assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

    let c = vec![0.0, 1.0, 0.0];
    assert!((cosine_similarity(&a, &c) - 0.0).abs() < 0.001);

    let d = vec![0.707, 0.707, 0.0];
    assert!((cosine_similarity(&a, &d) - 0.707).abs() < 0.01);
}
```

### Test 7: Load BMAD Template
```rust
#[tokio::test]
async fn test_load_bmad_template() {
    let yaml = r#"
template:
  id: test-template
  name: Test Document
  version: 1.0
  output:
    format: markdown
    filename: test.md

sections:
  - id: status
    title: Status
    type: choice
    choices: [Draft, Approved, Done]
  - id: description
    title: Description
    type: paragraphs
  - id: tasks
    title: Tasks
    type: checklist
    required: true
"#;
    let template: BmadTemplate = serde_yaml::from_str(yaml).unwrap();

    assert_eq!(template.template.id, "test-template");
    assert_eq!(template.template.version, "1.0");
    assert_eq!(template.sections.len(), 3);
    assert_eq!(template.sections[0].section_type, SectionContentType::Choice);
    assert_eq!(template.sections[0].choices, Some(vec!["Draft".to_string(), "Approved".to_string(), "Done".to_string()]));
    assert_eq!(template.sections[2].section_type, SectionContentType::Checklist);
}
```

### Test 8: BMAD Nested Sections
```rust
#[test]
fn test_bmad_nested_sections() {
    let yaml = r#"
template:
  id: nested-test
  name: Nested Template
  version: 1.0
  output:
    format: markdown
    filename: test.md

sections:
  - id: dev-notes
    title: Dev Notes
    type: paragraphs
    sections:
      - id: testing
        title: Testing
        type: bullet-list
      - id: files
        title: Files Changed
        type: bullet-list
"#;
    let template: BmadTemplate = serde_yaml::from_str(yaml).unwrap();

    let all_ids = template.all_section_ids();
    assert!(all_ids.contains(&"dev-notes"));
    assert!(all_ids.contains(&"testing"));
    assert!(all_ids.contains(&"files"));
    assert_eq!(all_ids.len(), 3);

    let all_titles = template.all_section_titles();
    assert!(all_titles.contains(&"Dev Notes"));
    assert!(all_titles.contains(&"Testing"));
}
```

### Test 9: BMAD Choice Validation
```rust
#[test]
fn test_bmad_choice_validation() {
    // Valid choices
    assert!(validate_choice_value("Done", &["Draft", "Approved", "Done"]));
    assert!(validate_choice_value("done", &["Draft", "Approved", "Done"])); // case insensitive
    assert!(validate_choice_value("DRAFT", &["Draft", "Approved", "Done"]));

    // Invalid choices
    assert!(!validate_choice_value("InProgress", &["Draft", "Approved", "Done"]));
    assert!(!validate_choice_value("", &["Draft", "Approved", "Done"]));
}

fn validate_choice_value(value: &str, choices: &[&str]) -> bool {
    choices.iter().any(|c| c.eq_ignore_ascii_case(value))
}
```

### Test 10: BMAD Conformance Check
```rust
#[tokio::test]
async fn test_bmad_conformance_check() {
    let yaml = r#"
template:
  id: story-template
  name: Story
  version: 1.0
  output:
    format: markdown
    filename: story.md

sections:
  - id: status
    title: Status
    type: choice
    choices: [Draft, Done]
  - id: description
    title: Description
    type: paragraphs
    required: true
  - id: tasks
    title: Tasks
    type: checklist
    required: false
"#;
    let template: BmadTemplate = serde_yaml::from_str(yaml).unwrap();

    // Document missing required "Description" section
    let doc = MarkdownParser::new().parse("# My Story\n## Status\nDraft\n## Notes\nSome notes").unwrap();

    let manager = TemplateManager::default();
    let result = manager.check_bmad_conformance(&doc, &template, Path::new("test.yaml"));

    assert!(!result.is_conformant);
    assert!(result.missing_sections.iter().any(|s| s.section_id == "description" && s.is_required));
    // Tasks is optional, so missing it shouldn't fail conformance by itself
    assert!(result.missing_sections.iter().any(|s| s.section_id == "tasks" && !s.is_required));
}
```

### Test 11: YAML Template Priority
```rust
#[tokio::test]
async fn test_yaml_template_priority() {
    let dir = tempdir().unwrap();

    // Create both YAML and MD templates
    tokio::fs::write(dir.path().join("story-tmpl.yaml"), "template:\n  id: yaml\n  name: YAML\n  version: 1.0\n  output:\n    format: markdown\n    filename: t.md\nsections: []").await.unwrap();
    tokio::fs::write(dir.path().join("story-tmpl.md"), "# Markdown Template").await.unwrap();

    let detected = TemplateManager::detect_template(dir.path());
    assert!(detected.is_some());

    // YAML should be detected first (higher priority)
    let path = detected.unwrap();
    assert!(path.extension().unwrap() == "yaml");
}
```

### Test 12: Section Type Validation
```rust
#[test]
fn test_section_type_validation() {
    // Checklist content
    let checklist = "- [ ] Task 1\n- [x] Task 2\n- [ ] Task 3";
    assert!(checklist.contains("- [ ]") || checklist.contains("- [x]"));

    // Bullet list (not checklist)
    let bullet = "- Item 1\n- Item 2";
    assert!(bullet.contains("- ") && !bullet.contains("- [ ]"));

    // Numbered list
    let numbered = "1. First\n2. Second";
    assert!(numbered.lines().any(|l| l.trim().starts_with(|c: char| c.is_ascii_digit())));

    // Table
    let table = "| Col1 | Col2 |\n|------|------|\n| A | B |";
    assert!(table.contains("|") && table.contains("---"));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/template_schema.rs` | BMAD template format parser and types |
| `sdk/rust/src/graphdocs/conformance.rs` | Template conformance checking |
| `sdk/rust/src/graphdocs/normalizer.rs` | Status normalization and pattern matching |
| `sdk/rust/src/graphdocs/embedding_matcher.rs` | Embedding-based status matching |
| `sdk/rust/src/graphdocs/parser.rs` | Parser (STORY-2.1.1) |
| `sdk/rust/src/graphdocs/variable_types.rs` | Variable types (STORY-2.1.2) |

## Dependencies

```toml
[dependencies]
regex = "1"
glob = "0.3"
tokio = { version = "1", features = ["fs", "process", "rt-multi-thread"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"   # For BMAD template parsing
anyhow = "1"
```

## Change Log

| Date | Change | Author |
|------|--------|--------|
| 2026-01-15 | Added BMAD YAML template format specification | Sarah (PO) |
| 2026-01-15 | Updated AC to prioritize YAML templates | Sarah (PO) |
| 2026-01-15 | Added template_schema.rs with BmadTemplate types | Sarah (PO) |
| 2026-01-15 | Added section type and choice validation | Sarah (PO) |
| 2026-01-15 | Added 6 new BMAD-specific tests (7-12) | Sarah (PO) |
| 2026-01-16 | Verified all ACs implemented, all 12 tests passing | James (Dev) |

---

## Dev Agent Record

### Agent Model Used
Claude Opus 4.5 (claude-opus-4-5-20251101)

### File List

| File | Status | Description |
|------|--------|-------------|
| `sdk/rust/src/graphdocs/conformance.rs` | Existing | Template detection, conformance checking, section validation (14 tests) |
| `sdk/rust/src/graphdocs/normalizer.rs` | Modified | Fixed clippy warning for char array pattern matching (13 tests) |
| `sdk/rust/src/graphdocs/embedding_matcher.rs` | Existing | Cosine similarity, status embeddings (12 tests) |
| `sdk/rust/src/graphdocs/template_schema.rs` | Existing | BMAD template parsing, section types (13 tests) |
| `sdk/rust/src/graphdocs/parser.rs` | Existing | Edge generation from parsed sections (13 tests) |

### Debug Log References
None required - all implementations verified complete.

### Completion Notes

1. **All Acceptance Criteria Verified**: All 6 ACs were already implemented:
   - AC1: Edge structure generation in `parser.rs:321-334` (`generate_edges()`)
   - AC2: Template detection with YAML priority in `conformance.rs` (`TEMPLATE_PATTERNS`, `detect_template()`)
   - AC3: BMAD YAML parsing in `template_schema.rs` (`BmadTemplate`, `TemplateSection`, etc.)
   - AC4: Document conformance in `conformance.rs` (`check_bmad_conformance()`)
   - AC5: Section type validation in `conformance.rs` (`validate_section_type()`)
   - AC6: Status mapping in `normalizer.rs` and `embedding_matcher.rs`

2. **All 12 Story Tests Verified**:
   - Test 1-3: Status normalization, progress extraction, optional flags in `normalizer::tests`
   - Test 4-5: Template detection, conformance check in `conformance::tests`
   - Test 6: Cosine similarity in `embedding_matcher::tests`
   - Test 7-12: BMAD templates, nested sections, choice validation in `template_schema::tests` and `conformance::tests`

3. **Minor Fix Applied**:
   - Fixed clippy warning in `normalizer.rs:132`: Changed manual char comparison to array pattern

4. **Test Results**: 122 graphdocs tests pass, 1 ignored. Full regression passes.

5. **Linting**: No clippy errors in graphdocs module.

### Change Log

| Date | Change | Reason |
|------|--------|--------|
| 2026-01-16 | Fixed manual char comparison in normalizer.rs | Clippy warning about `find(\|c\| c == '-' \|\| c == '(' \|\| c == '\|')` changed to `find(['-', '(', '\|'])` |

---

## QA Results

### Review Date: 2026-01-16

### Reviewed By: Quinn (Test Architect)

### Code Quality Assessment

Implementation is clean, idiomatic Rust following project coding standards. The module uses well-designed abstractions:

- **conformance.rs (833 lines)**: Template detection and conformance checking with proper separation of BMAD YAML and markdown template formats. Good use of HashMap for section lookups, clean recursive section checking.
- **normalizer.rs (327 lines)**: Status normalization with comprehensive regex-based pattern matching. Proper handling of edge cases (progress info, see-also references, optional/experimental flags).
- **embedding_matcher.rs (282 lines)**: Cosine similarity implementation with graceful fallback when TEA CLI unavailable. Good defensive coding.
- **template_schema.rs (577 lines)**: Complete BMAD template schema with serde deserialization. Helper methods for nested section traversal.

### Refactoring Performed

No refactoring performed - code quality is already high. Minor clippy fix was applied by dev agent.

### Compliance Check

- Coding Standards: ✓ Rust 2021 edition, proper error handling with `anyhow`, idiomatic patterns
- Project Structure: ✓ Located at `sdk/rust/src/graphdocs/` per architecture docs
- Testing Strategy: ✓ Inline unit tests per Rust convention (52 tests across 4 modules)
- All ACs Met: ✓ All 6 acceptance criteria verified with passing tests

### Requirements Traceability

| AC | Requirement | Test(s) | Status |
|----|-------------|---------|--------|
| 1 | Generate edge structure from parsed sections | `parser::test_generate_edges`, `parser::test_edge_follows_relationship` | ✓ |
| 2 | Detect templates with YAML priority | `conformance::test_template_detection`, `conformance::test_yaml_template_priority`, `conformance::test_template_patterns_order` | ✓ |
| 3 | Parse BMAD YAML template format | `template_schema::test_load_bmad_template`, `template_schema::test_bmad_nested_sections`, `template_schema::test_all_section_types` | ✓ |
| 4 | Validate document conformance | `conformance::test_bmad_conformance_check_missing_required`, `conformance::test_bmad_conformance_check_all_present`, `conformance::test_markdown_conformance_check` | ✓ |
| 5 | Validate section types | `conformance::test_section_type_validation_checklist`, `conformance::test_section_type_validation_bullet_list`, `conformance::test_section_type_validation_numbered_list`, `conformance::test_choice_validation` | ✓ |
| 6 | Map status markers using embeddings | `normalizer::test_status_normalization_*` (6 tests), `embedding_matcher::test_cosine_similarity_*` (6 tests), `embedding_matcher::test_match_status_sync_*`, `embedding_matcher::test_fallback_match` | ✓ |

### Improvements Checklist

- [x] All acceptance criteria implemented and tested
- [x] Clippy warning fixed (char array pattern in normalizer.rs)
- [x] Comprehensive test coverage (52 tests in scope)
- [x] Proper error handling with graceful fallbacks
- [ ] Consider caching compiled regex patterns in normalizer.rs (future optimization)
- [ ] Consider adding integration tests for scan_directory function (future)

### Security Review

No security concerns - this is a pure parsing/validation module with:
- No external input vulnerabilities (template files are trusted project files)
- No network operations (TEA embedding is optional fallback)
- No file system writes (read-only conformance checking)
- Proper input validation on YAML parsing via serde

### Performance Considerations

- Single-pass template detection using glob patterns
- Efficient HashMap-based section lookups
- Regex compilation on each call in normalizer (minor optimization opportunity)
- Async file I/O where appropriate
- Overall performance appropriate for build-time document validation

### Files Modified During Review

None - no refactoring was necessary.

### Gate Status

Gate: PASS → docs/qa/gates/2.1.3-template-conformance.yml

### Recommended Status

✓ Ready for Done

All acceptance criteria are met, comprehensive test coverage exists (52 tests, 122 total in graphdocs module), code quality is high, and no blocking issues were identified.
