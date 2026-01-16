//! BMAD Template Schema Parser
//!
//! Parses BMAD YAML template format for document conformance checking.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::relationships::RelationshipDecl;

/// BMAD Template Format
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BmadTemplate {
    pub template: TemplateMetadata,
    #[serde(default)]
    pub workflow: Option<WorkflowConfig>,
    #[serde(default)]
    pub agent_config: Option<AgentConfig>,
    /// Relationship declarations for cross-document queries (STORY-2.1.5)
    #[serde(default)]
    pub relationships: Option<Vec<RelationshipDecl>>,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Markdown,
    Yaml,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Markdown
    }
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
    pub required: Option<bool>, // None = true (default required)
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
    pub sections: Option<Vec<TemplateSection>>, // Nested sections
    /// Reference to relationships[].id for Relationship section type (STORY-2.1.5)
    #[serde(default)]
    pub relationship: Option<String>,
    /// Tera/Jinja2 template for rendering relationship data (STORY-2.1.5)
    #[serde(default)]
    pub render: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
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
    /// Renders related documents via relationship declaration (STORY-2.1.5)
    Relationship,
}

impl BmadTemplate {
    /// Load template from YAML file
    pub async fn load(path: &Path) -> Result<Self> {
        let content = tokio::fs::read_to_string(path).await?;
        let template: BmadTemplate = serde_yaml::from_str(&content)?;
        Ok(template)
    }

    /// Load template from YAML string
    pub fn from_yaml(content: &str) -> Result<Self> {
        let template: BmadTemplate = serde_yaml::from_str(content)?;
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

    /// Find section by ID (including nested)
    pub fn find_section_by_id(&self, id: &str) -> Option<&TemplateSection> {
        fn find_recursive<'a>(
            sections: &'a [TemplateSection],
            id: &str,
        ) -> Option<&'a TemplateSection> {
            for section in sections {
                if section.id == id {
                    return Some(section);
                }
                if let Some(ref nested) = section.sections {
                    if let Some(found) = find_recursive(nested, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        find_recursive(&self.sections, id)
    }

    /// Find section by title (case-insensitive, including nested)
    pub fn find_section_by_title(&self, title: &str) -> Option<&TemplateSection> {
        let lower_title = title.to_lowercase();
        fn find_recursive<'a>(
            sections: &'a [TemplateSection],
            title: &str,
        ) -> Option<&'a TemplateSection> {
            for section in sections {
                if section.title.to_lowercase() == title {
                    return Some(section);
                }
                if let Some(ref nested) = section.sections {
                    if let Some(found) = find_recursive(nested, title) {
                        return Some(found);
                    }
                }
            }
            None
        }
        find_recursive(&self.sections, &lower_title)
    }
}

/// Validate a choice value against allowed choices (case-insensitive)
pub fn validate_choice_value(value: &str, choices: &[impl AsRef<str>]) -> bool {
    choices
        .iter()
        .any(|c| c.as_ref().eq_ignore_ascii_case(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_bmad_template() {
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
        let template = BmadTemplate::from_yaml(yaml).unwrap();

        assert_eq!(template.template.id, "test-template");
        assert_eq!(template.template.version, "1.0");
        assert_eq!(template.sections.len(), 3);
        assert_eq!(
            template.sections[0].section_type,
            SectionContentType::Choice
        );
        assert_eq!(
            template.sections[0].choices,
            Some(vec![
                "Draft".to_string(),
                "Approved".to_string(),
                "Done".to_string()
            ])
        );
        assert_eq!(
            template.sections[2].section_type,
            SectionContentType::Checklist
        );
    }

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
        let template = BmadTemplate::from_yaml(yaml).unwrap();

        let all_ids = template.all_section_ids();
        assert!(all_ids.contains(&"dev-notes"));
        assert!(all_ids.contains(&"testing"));
        assert!(all_ids.contains(&"files"));
        assert_eq!(all_ids.len(), 3);

        let all_titles = template.all_section_titles();
        assert!(all_titles.contains(&"Dev Notes"));
        assert!(all_titles.contains(&"Testing"));
    }

    #[test]
    fn test_bmad_choice_validation() {
        // Valid choices
        assert!(validate_choice_value(
            "Done",
            &["Draft", "Approved", "Done"]
        ));
        assert!(validate_choice_value(
            "done",
            &["Draft", "Approved", "Done"]
        )); // case insensitive
        assert!(validate_choice_value(
            "DRAFT",
            &["Draft", "Approved", "Done"]
        ));

        // Invalid choices
        assert!(!validate_choice_value(
            "InProgress",
            &["Draft", "Approved", "Done"]
        ));
        assert!(!validate_choice_value("", &["Draft", "Approved", "Done"]));
    }

    #[test]
    fn test_section_required_default() {
        let section = TemplateSection {
            id: "test".to_string(),
            title: "Test".to_string(),
            section_type: SectionContentType::Paragraphs,
            required: None,
            choices: None,
            columns: None,
            template: None,
            instruction: None,
            owner: None,
            editors: None,
            elicit: None,
            sections: None,
            relationship: None,
            render: None,
        };

        // Default is required
        assert!(BmadTemplate::is_section_required(&section));
    }

    #[test]
    fn test_section_required_explicit() {
        let mut section = TemplateSection {
            id: "test".to_string(),
            title: "Test".to_string(),
            section_type: SectionContentType::Paragraphs,
            required: Some(false),
            choices: None,
            columns: None,
            template: None,
            instruction: None,
            owner: None,
            editors: None,
            elicit: None,
            sections: None,
            relationship: None,
            render: None,
        };

        assert!(!BmadTemplate::is_section_required(&section));

        section.required = Some(true);
        assert!(BmadTemplate::is_section_required(&section));
    }

    #[test]
    fn test_find_section_by_id() {
        let yaml = r#"
template:
  id: test
  name: Test
  version: 1.0
  output:
    format: markdown
    filename: test.md

sections:
  - id: parent
    title: Parent
    type: paragraphs
    sections:
      - id: child
        title: Child
        type: bullet-list
"#;
        let template = BmadTemplate::from_yaml(yaml).unwrap();

        assert!(template.find_section_by_id("parent").is_some());
        assert!(template.find_section_by_id("child").is_some());
        assert!(template.find_section_by_id("nonexistent").is_none());
    }

    #[test]
    fn test_find_section_by_title() {
        let yaml = r#"
template:
  id: test
  name: Test
  version: 1.0
  output:
    format: markdown
    filename: test.md

sections:
  - id: parent
    title: Parent Section
    type: paragraphs
"#;
        let template = BmadTemplate::from_yaml(yaml).unwrap();

        // Case insensitive
        assert!(template.find_section_by_title("Parent Section").is_some());
        assert!(template.find_section_by_title("parent section").is_some());
        assert!(template.find_section_by_title("PARENT SECTION").is_some());
        assert!(template.find_section_by_title("nonexistent").is_none());
    }

    #[test]
    fn test_output_format_default() {
        assert_eq!(OutputFormat::default(), OutputFormat::Markdown);
    }

    #[test]
    fn test_section_content_type_default() {
        assert_eq!(
            SectionContentType::default(),
            SectionContentType::Paragraphs
        );
    }

    #[test]
    fn test_workflow_config() {
        let yaml = r#"
template:
  id: test
  name: Test
  version: 1.0
  output:
    format: markdown
    filename: test.md

workflow:
  mode: interactive
  elicitation: elicit-task

sections: []
"#;
        let template = BmadTemplate::from_yaml(yaml).unwrap();

        assert!(template.workflow.is_some());
        let workflow = template.workflow.unwrap();
        assert_eq!(workflow.mode, Some("interactive".to_string()));
        assert_eq!(workflow.elicitation, Some("elicit-task".to_string()));
    }

    #[test]
    fn test_agent_config() {
        let yaml = r#"
template:
  id: test
  name: Test
  version: 1.0
  output:
    format: markdown
    filename: test.md

agent_config:
  editable_sections:
    - status
    - tasks

sections: []
"#;
        let template = BmadTemplate::from_yaml(yaml).unwrap();

        assert!(template.agent_config.is_some());
        let config = template.agent_config.unwrap();
        assert_eq!(config.editable_sections, vec!["status", "tasks"]);
    }

    #[test]
    fn test_all_section_types() {
        let yaml = r#"
template:
  id: test
  name: Test
  version: 1.0
  output:
    format: markdown
    filename: test.md

sections:
  - id: s1
    title: S1
    type: paragraphs
  - id: s2
    title: S2
    type: choice
  - id: s3
    title: S3
    type: bullet-list
  - id: s4
    title: S4
    type: numbered-list
  - id: s5
    title: S5
    type: checklist
  - id: s6
    title: S6
    type: table
  - id: s7
    title: S7
    type: template-text
  - id: s8
    title: S8
    type: code
  - id: s9
    title: S9
    type: mermaid
"#;
        let template = BmadTemplate::from_yaml(yaml).unwrap();

        assert_eq!(
            template.sections[0].section_type,
            SectionContentType::Paragraphs
        );
        assert_eq!(
            template.sections[1].section_type,
            SectionContentType::Choice
        );
        assert_eq!(
            template.sections[2].section_type,
            SectionContentType::BulletList
        );
        assert_eq!(
            template.sections[3].section_type,
            SectionContentType::NumberedList
        );
        assert_eq!(
            template.sections[4].section_type,
            SectionContentType::Checklist
        );
        assert_eq!(template.sections[5].section_type, SectionContentType::Table);
        assert_eq!(
            template.sections[6].section_type,
            SectionContentType::TemplateText
        );
        assert_eq!(template.sections[7].section_type, SectionContentType::Code);
        assert_eq!(
            template.sections[8].section_type,
            SectionContentType::Mermaid
        );
    }

    #[test]
    fn test_section_with_columns() {
        let yaml = r#"
template:
  id: test
  name: Test
  version: 1.0
  output:
    format: markdown
    filename: test.md

sections:
  - id: table
    title: Data Table
    type: table
    columns: [Name, Value, Description]
"#;
        let template = BmadTemplate::from_yaml(yaml).unwrap();

        assert_eq!(
            template.sections[0].columns,
            Some(vec![
                "Name".to_string(),
                "Value".to_string(),
                "Description".to_string()
            ])
        );
    }
}
