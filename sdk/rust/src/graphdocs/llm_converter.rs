//! LLM-based document converter for GraphDocs
//!
//! This module provides intelligent document structure extraction using LLMs,
//! with fallback to the deterministic Markdown parser when needed.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::graphdocs::parser::{
    EdgeType, MarkdownParser, ParsedDocument, ParsedEdge, ParsedSection, SectionType,
};

/// Error types for LLM operations
#[derive(Debug, thiserror::Error)]
pub enum LLMError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Rate limited")]
    RateLimited,
    #[error("Timeout")]
    Timeout,
}

/// Errors that can occur during document conversion
#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("LLM error: {0}")]
    LLMError(#[from] LLMError),
    #[error("No JSON found in response")]
    NoJsonFound,
    #[error("Invalid JSON: {0}")]
    InvalidJson(String),
    #[error("Invalid section type: {0}")]
    InvalidSectionType(String),
    #[error("Invalid heading level: {0}")]
    InvalidHeadingLevel(u8),
    #[error("Invalid variable type: {0}")]
    InvalidVarType(String),
    #[error("Invalid section reference: {0}")]
    InvalidReference(String),
    #[error("Parse error: {0}")]
    ParseError(String),
}

/// Trait for LLM API clients
#[async_trait]
pub trait LLMClient: Send + Sync {
    async fn complete(&self, prompt: &str, model: &str) -> Result<String, LLMError>;
}

/// Schema expected from LLM output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMDocumentSchema {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sections: Vec<LLMSection>,
    #[serde(default)]
    pub variables: Vec<LLMVariable>,
    #[serde(default)]
    pub relationships: Vec<LLMRelationship>,
}

/// A section in the LLM-parsed document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMSection {
    pub id: String,
    pub section_type: String,
    #[serde(default)]
    pub level: Option<u8>,
    pub content: String,
    #[serde(default)]
    pub order: u32,
}

/// A variable extracted by the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMVariable {
    pub name: String,
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub var_type: String,
}

/// A relationship between sections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRelationship {
    pub from_section: String,
    pub to_section: String,
    pub relationship_type: String,
}

/// LLM-based document converter with fallback to deterministic parsing
pub struct LLMConverter {
    client: Box<dyn LLMClient>,
    model: String,
    fallback_parser: MarkdownParser,
}

impl LLMConverter {
    /// Create a new LLM converter with the specified client and model
    pub fn new(client: Box<dyn LLMClient>, model: String) -> Self {
        Self {
            client,
            model,
            fallback_parser: MarkdownParser::new(),
        }
    }

    /// Convert content using LLM with fallback to deterministic parser
    pub async fn convert(&self, content: &str) -> Result<ParsedDocument, ConvertError> {
        match self.convert_with_llm(content).await {
            Ok(doc) => Ok(doc),
            Err(e) => {
                tracing::warn!("LLM conversion failed, using fallback: {}", e);
                self.fallback_parser
                    .parse(content)
                    .map_err(|e| ConvertError::ParseError(e.to_string()))
            }
        }
    }

    /// Convert content using only the deterministic parser (no LLM)
    pub fn convert_deterministic(&self, content: &str) -> Result<ParsedDocument, ConvertError> {
        self.fallback_parser
            .parse(content)
            .map_err(|e| ConvertError::ParseError(e.to_string()))
    }

    async fn convert_with_llm(&self, content: &str) -> Result<ParsedDocument, ConvertError> {
        let prompt = self.build_prompt(content);
        let response = self.client.complete(&prompt, &self.model).await?;
        let schema = self.parse_response(&response)?;
        self.validate_schema(&schema)?;
        Ok(self.schema_to_document(schema))
    }

    /// Build the prompt for structure extraction
    fn build_prompt(&self, content: &str) -> String {
        format!(
            r#"You are a document structure analyzer. Analyze the following document and extract its structure as JSON.

## Instructions

1. Identify the document title (usually the first heading)
2. Break the document into logical sections
3. Identify any template variables (text in {{{{braces}}}})
4. Identify relationships between sections
5. For each variable, suggest a type and default value

## Output Format

Return ONLY valid JSON matching this schema:

```json
{{
  "title": "Document Title",
  "description": "Brief description",
  "sections": [
    {{
      "id": "s1",
      "section_type": "heading|paragraph|list|code|table|blockquote|hr",
      "level": 1,
      "content": "Section content with {{{{variables}}}}",
      "order": 0
    }}
  ],
  "variables": [
    {{
      "name": "variable_name",
      "default_value": "",
      "description": "What this variable represents",
      "var_type": "string|number|boolean|array|object"
    }}
  ],
  "relationships": [
    {{
      "from_section": "s1",
      "to_section": "s2",
      "relationship_type": "follows|contains|references"
    }}
  ]
}}
```

## Document to Analyze

```
{content}
```

## Response

"#
        )
    }

    /// Parse the LLM response into a schema
    fn parse_response(&self, response: &str) -> Result<LLMDocumentSchema, ConvertError> {
        let json_str = self.extract_json(response)?;
        serde_json::from_str(&json_str).map_err(|e| ConvertError::InvalidJson(e.to_string()))
    }

    /// Extract JSON from an LLM response (handles markdown code blocks)
    fn extract_json(&self, response: &str) -> Result<String, ConvertError> {
        // Try to find JSON in code block
        if let Some(start) = response.find("```json") {
            let after_start = &response[start + 7..];
            if let Some(end) = after_start.find("```") {
                return Ok(after_start[..end].trim().to_string());
            }
        }

        // Try plain code block
        if let Some(start) = response.find("```") {
            let after_start = &response[start + 3..];
            // Skip language identifier if present
            let json_start = after_start.find('\n').map(|i| i + 1).unwrap_or(0);
            let after_lang = &after_start[json_start..];
            if let Some(end) = after_lang.find("```") {
                let potential_json = after_lang[..end].trim();
                if potential_json.starts_with('{') {
                    return Ok(potential_json.to_string());
                }
            }
        }

        // Try to find raw JSON
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                return Ok(response[start..=end].to_string());
            }
        }

        Err(ConvertError::NoJsonFound)
    }

    /// Validate the schema for correctness
    pub fn validate_schema(&self, schema: &LLMDocumentSchema) -> Result<(), ConvertError> {
        // Valid section types
        let valid_types = [
            "heading",
            "paragraph",
            "list",
            "code",
            "table",
            "blockquote",
            "hr",
            "checklist",
            "choice",
        ];
        for section in &schema.sections {
            if !valid_types.contains(&section.section_type.as_str()) {
                return Err(ConvertError::InvalidSectionType(
                    section.section_type.clone(),
                ));
            }
        }

        // Validate heading levels
        for section in &schema.sections {
            if section.section_type == "heading" {
                if let Some(level) = section.level {
                    if !(1..=6).contains(&level) {
                        return Err(ConvertError::InvalidHeadingLevel(level));
                    }
                }
            }
        }

        // Valid variable types
        let valid_var_types = ["string", "number", "boolean", "array", "object"];
        for var in &schema.variables {
            if !valid_var_types.contains(&var.var_type.as_str()) {
                return Err(ConvertError::InvalidVarType(var.var_type.clone()));
            }
        }

        // Validate relationship references
        let section_ids: HashSet<_> = schema.sections.iter().map(|s| s.id.as_str()).collect();
        for rel in &schema.relationships {
            if !section_ids.contains(rel.from_section.as_str()) {
                return Err(ConvertError::InvalidReference(rel.from_section.clone()));
            }
            if !section_ids.contains(rel.to_section.as_str()) {
                return Err(ConvertError::InvalidReference(rel.to_section.clone()));
            }
        }

        Ok(())
    }

    /// Convert the LLM schema to a ParsedDocument
    fn schema_to_document(&self, schema: LLMDocumentSchema) -> ParsedDocument {
        let sections = schema
            .sections
            .iter()
            .map(|s| {
                ParsedSection {
                    id: s.id.clone(),
                    section_type: match s.section_type.as_str() {
                        "heading" => SectionType::Heading,
                        "paragraph" => SectionType::Paragraph,
                        "list" => SectionType::List,
                        "code" => SectionType::Code,
                        "table" => SectionType::Table,
                        "blockquote" => SectionType::Blockquote,
                        "hr" => SectionType::HorizontalRule,
                        "checklist" => SectionType::Checklist,
                        "choice" => SectionType::Choice,
                        _ => SectionType::Paragraph, // Default fallback
                    },
                    level: s.level,
                    content: s.content.clone(),
                    order_idx: s.order,
                    variables: extract_variables(&s.content),
                }
            })
            .collect();

        let variables = schema.variables.iter().map(|v| v.name.clone()).collect();

        let edges = schema
            .relationships
            .iter()
            .filter_map(|r| {
                let source_idx = schema
                    .sections
                    .iter()
                    .position(|s| s.id == r.from_section)?;
                let target_idx = schema.sections.iter().position(|s| s.id == r.to_section)?;
                Some(ParsedEdge {
                    source_idx,
                    target_idx,
                    edge_type: match r.relationship_type.as_str() {
                        "contains" => EdgeType::Contains,
                        "references" => EdgeType::Follows, // Map references to follows for now
                        _ => EdgeType::Follows,
                    },
                })
            })
            .collect();

        ParsedDocument {
            title: Some(schema.title),
            sections,
            variables,
            edges,
        }
    }
}

/// Extract variable names from content ({{variable_name}})
fn extract_variables(content: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\{\{(\w+)\}\}").unwrap();
    re.captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock LLM client for testing
    struct MockLLMClient {
        response: String,
    }

    impl MockLLMClient {
        fn new(response: &str) -> Self {
            Self {
                response: response.to_string(),
            }
        }

        fn error() -> Self {
            Self {
                response: String::new(),
            }
        }
    }

    #[async_trait]
    impl LLMClient for MockLLMClient {
        async fn complete(&self, _prompt: &str, _model: &str) -> Result<String, LLMError> {
            if self.response.is_empty() {
                Err(LLMError::ApiError("Mock error".to_string()))
            } else {
                Ok(self.response.clone())
            }
        }
    }

    #[tokio::test]
    async fn test_valid_response() {
        let mock_response = r#"
        ```json
        {
            "title": "Test Document",
            "sections": [
                {"id": "s1", "section_type": "heading", "level": 1, "content": "Test Heading", "order": 0}
            ],
            "variables": [],
            "relationships": []
        }
        ```
        "#;

        let converter =
            LLMConverter::new(Box::new(MockLLMClient::new(mock_response)), "test".into());
        let doc = converter.convert("# Test").await.unwrap();

        assert_eq!(doc.title, Some("Test Document".to_string()));
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].section_type, SectionType::Heading);
    }

    #[tokio::test]
    async fn test_fallback_on_llm_error() {
        let converter = LLMConverter::new(Box::new(MockLLMClient::error()), "test".into());

        // Should fall back to deterministic parser
        let doc = converter.convert("# Test\n\nParagraph").await.unwrap();
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].section_type, SectionType::Heading);
        assert_eq!(doc.sections[1].section_type, SectionType::Paragraph);
    }

    #[tokio::test]
    async fn test_fallback_on_invalid_json() {
        let converter =
            LLMConverter::new(Box::new(MockLLMClient::new("invalid json")), "test".into());

        // Should fall back to deterministic parser
        let doc = converter.convert("# Test\n\nParagraph").await.unwrap();
        assert_eq!(doc.sections.len(), 2);
    }

    #[test]
    fn test_validate_invalid_section_type() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![LLMSection {
                id: "s1".into(),
                section_type: "invalid".into(),
                level: None,
                content: "test".into(),
                order: 0,
            }],
            variables: vec![],
            relationships: vec![],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        assert!(converter.validate_schema(&schema).is_err());
    }

    #[test]
    fn test_validate_invalid_heading_level() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![LLMSection {
                id: "s1".into(),
                section_type: "heading".into(),
                level: Some(7), // Invalid: must be 1-6
                content: "test".into(),
                order: 0,
            }],
            variables: vec![],
            relationships: vec![],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        let result = converter.validate_schema(&schema);
        assert!(matches!(result, Err(ConvertError::InvalidHeadingLevel(7))));
    }

    #[test]
    fn test_validate_invalid_var_type() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![],
            variables: vec![LLMVariable {
                name: "test".into(),
                default_value: None,
                description: None,
                var_type: "invalid".into(),
            }],
            relationships: vec![],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        assert!(converter.validate_schema(&schema).is_err());
    }

    #[test]
    fn test_validate_invalid_relationship_reference() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![LLMSection {
                id: "s1".into(),
                section_type: "paragraph".into(),
                level: None,
                content: "test".into(),
                order: 0,
            }],
            variables: vec![],
            relationships: vec![LLMRelationship {
                from_section: "s1".into(),
                to_section: "nonexistent".into(), // Invalid reference
                relationship_type: "follows".into(),
            }],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        assert!(converter.validate_schema(&schema).is_err());
    }

    #[test]
    fn test_extract_json_from_code_block() {
        let response = r#"Here is the analysis:

```json
{
    "title": "Test",
    "sections": []
}
```

That's the result."#;

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        let json = converter.extract_json(response).unwrap();
        assert!(json.contains("\"title\": \"Test\""));
    }

    #[test]
    fn test_extract_json_raw() {
        let response = r#"{"title": "Test", "sections": []}"#;

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        let json = converter.extract_json(response).unwrap();
        assert_eq!(json, response);
    }

    #[test]
    fn test_extract_json_no_json() {
        let response = "No JSON here!";

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        assert!(converter.extract_json(response).is_err());
    }

    #[test]
    fn test_schema_to_document_with_variables() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: Some("A test document".into()),
            sections: vec![LLMSection {
                id: "s1".into(),
                section_type: "paragraph".into(),
                level: None,
                content: "Hello {{name}}!".into(),
                order: 0,
            }],
            variables: vec![LLMVariable {
                name: "name".into(),
                default_value: Some("World".into()),
                description: Some("The name to greet".into()),
                var_type: "string".into(),
            }],
            relationships: vec![],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        let doc = converter.schema_to_document(schema);

        assert_eq!(doc.title, Some("Test".to_string()));
        assert_eq!(doc.sections.len(), 1);
        assert!(doc.variables.contains(&"name".to_string()));
        assert!(doc.sections[0].variables.contains(&"name".to_string()));
    }

    #[test]
    fn test_schema_to_document_with_relationships() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![
                LLMSection {
                    id: "s1".into(),
                    section_type: "heading".into(),
                    level: Some(1),
                    content: "First".into(),
                    order: 0,
                },
                LLMSection {
                    id: "s2".into(),
                    section_type: "paragraph".into(),
                    level: None,
                    content: "Second".into(),
                    order: 1,
                },
            ],
            variables: vec![],
            relationships: vec![LLMRelationship {
                from_section: "s1".into(),
                to_section: "s2".into(),
                relationship_type: "contains".into(),
            }],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        let doc = converter.schema_to_document(schema);

        assert_eq!(doc.edges.len(), 1);
        assert_eq!(doc.edges[0].source_idx, 0);
        assert_eq!(doc.edges[0].target_idx, 1);
        assert!(matches!(doc.edges[0].edge_type, EdgeType::Contains));
    }

    #[test]
    fn test_validate_valid_schema() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![
                LLMSection {
                    id: "s1".into(),
                    section_type: "heading".into(),
                    level: Some(1),
                    content: "Heading".into(),
                    order: 0,
                },
                LLMSection {
                    id: "s2".into(),
                    section_type: "paragraph".into(),
                    level: None,
                    content: "Para".into(),
                    order: 1,
                },
            ],
            variables: vec![LLMVariable {
                name: "test".into(),
                default_value: None,
                description: None,
                var_type: "string".into(),
            }],
            relationships: vec![LLMRelationship {
                from_section: "s1".into(),
                to_section: "s2".into(),
                relationship_type: "follows".into(),
            }],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        assert!(converter.validate_schema(&schema).is_ok());
    }

    #[test]
    fn test_all_section_types() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![
                LLMSection {
                    id: "s1".into(),
                    section_type: "heading".into(),
                    level: Some(1),
                    content: "H".into(),
                    order: 0,
                },
                LLMSection {
                    id: "s2".into(),
                    section_type: "paragraph".into(),
                    level: None,
                    content: "P".into(),
                    order: 1,
                },
                LLMSection {
                    id: "s3".into(),
                    section_type: "list".into(),
                    level: None,
                    content: "L".into(),
                    order: 2,
                },
                LLMSection {
                    id: "s4".into(),
                    section_type: "code".into(),
                    level: None,
                    content: "C".into(),
                    order: 3,
                },
                LLMSection {
                    id: "s5".into(),
                    section_type: "table".into(),
                    level: None,
                    content: "T".into(),
                    order: 4,
                },
                LLMSection {
                    id: "s6".into(),
                    section_type: "blockquote".into(),
                    level: None,
                    content: "B".into(),
                    order: 5,
                },
                LLMSection {
                    id: "s7".into(),
                    section_type: "hr".into(),
                    level: None,
                    content: "---".into(),
                    order: 6,
                },
                LLMSection {
                    id: "s8".into(),
                    section_type: "checklist".into(),
                    level: None,
                    content: "CL".into(),
                    order: 7,
                },
                LLMSection {
                    id: "s9".into(),
                    section_type: "choice".into(),
                    level: None,
                    content: "CH".into(),
                    order: 8,
                },
            ],
            variables: vec![],
            relationships: vec![],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        assert!(converter.validate_schema(&schema).is_ok());

        let doc = converter.schema_to_document(schema);
        assert_eq!(doc.sections[0].section_type, SectionType::Heading);
        assert_eq!(doc.sections[1].section_type, SectionType::Paragraph);
        assert_eq!(doc.sections[2].section_type, SectionType::List);
        assert_eq!(doc.sections[3].section_type, SectionType::Code);
        assert_eq!(doc.sections[4].section_type, SectionType::Table);
        assert_eq!(doc.sections[5].section_type, SectionType::Blockquote);
        assert_eq!(doc.sections[6].section_type, SectionType::HorizontalRule);
        assert_eq!(doc.sections[7].section_type, SectionType::Checklist);
        assert_eq!(doc.sections[8].section_type, SectionType::Choice);
    }

    #[test]
    fn test_all_variable_types() {
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![],
            variables: vec![
                LLMVariable {
                    name: "s".into(),
                    default_value: None,
                    description: None,
                    var_type: "string".into(),
                },
                LLMVariable {
                    name: "n".into(),
                    default_value: None,
                    description: None,
                    var_type: "number".into(),
                },
                LLMVariable {
                    name: "b".into(),
                    default_value: None,
                    description: None,
                    var_type: "boolean".into(),
                },
                LLMVariable {
                    name: "a".into(),
                    default_value: None,
                    description: None,
                    var_type: "array".into(),
                },
                LLMVariable {
                    name: "o".into(),
                    default_value: None,
                    description: None,
                    var_type: "object".into(),
                },
            ],
            relationships: vec![],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        assert!(converter.validate_schema(&schema).is_ok());
    }

    #[test]
    fn test_heading_level_boundaries() {
        // Valid levels 1-6
        for level in 1..=6u8 {
            let schema = LLMDocumentSchema {
                title: "Test".into(),
                description: None,
                sections: vec![LLMSection {
                    id: "s1".into(),
                    section_type: "heading".into(),
                    level: Some(level),
                    content: "test".into(),
                    order: 0,
                }],
                variables: vec![],
                relationships: vec![],
            };

            let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
            assert!(
                converter.validate_schema(&schema).is_ok(),
                "Level {} should be valid",
                level
            );
        }

        // Invalid level 0
        let schema = LLMDocumentSchema {
            title: "Test".into(),
            description: None,
            sections: vec![LLMSection {
                id: "s1".into(),
                section_type: "heading".into(),
                level: Some(0),
                content: "test".into(),
                order: 0,
            }],
            variables: vec![],
            relationships: vec![],
        };

        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        assert!(converter.validate_schema(&schema).is_err());
    }

    #[test]
    fn test_convert_deterministic() {
        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        let doc = converter.convert_deterministic("# Hello\n\nWorld").unwrap();

        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.title, Some("Hello".to_string()));
    }

    #[test]
    fn test_build_prompt_contains_content() {
        let converter = LLMConverter::new(Box::new(MockLLMClient::new("")), "".into());
        let prompt = converter.build_prompt("# My Document\n\nSome content");

        assert!(prompt.contains("# My Document"));
        assert!(prompt.contains("Some content"));
        assert!(prompt.contains("document structure analyzer"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn test_extract_variables() {
        assert_eq!(extract_variables("Hello {{name}}!"), vec!["name"]);
        assert_eq!(extract_variables("{{a}} and {{b}}"), vec!["a", "b"]);
        assert_eq!(extract_variables("No variables here"), Vec::<String>::new());
        assert_eq!(
            extract_variables("{{snake_case_var}}"),
            vec!["snake_case_var"]
        );
    }

    #[tokio::test]
    async fn test_complex_document_conversion() {
        let mock_response = r#"```json
        {
            "title": "Project README",
            "description": "A project documentation",
            "sections": [
                {"id": "s1", "section_type": "heading", "level": 1, "content": "Project README", "order": 0},
                {"id": "s2", "section_type": "paragraph", "level": null, "content": "Welcome to {{project_name}}!", "order": 1},
                {"id": "s3", "section_type": "heading", "level": 2, "content": "Installation", "order": 2},
                {"id": "s4", "section_type": "code", "level": null, "content": "npm install {{package_name}}", "order": 3}
            ],
            "variables": [
                {"name": "project_name", "default_value": "My Project", "description": "The project name", "var_type": "string"},
                {"name": "package_name", "default_value": "my-package", "description": "NPM package name", "var_type": "string"}
            ],
            "relationships": [
                {"from_section": "s1", "to_section": "s2", "relationship_type": "follows"},
                {"from_section": "s3", "to_section": "s4", "relationship_type": "contains"}
            ]
        }
        ```"#;

        let converter =
            LLMConverter::new(Box::new(MockLLMClient::new(mock_response)), "gpt-4".into());
        let doc = converter
            .convert("# Project README\n\nWelcome!")
            .await
            .unwrap();

        assert_eq!(doc.title, Some("Project README".to_string()));
        assert_eq!(doc.sections.len(), 4);
        assert_eq!(doc.variables.len(), 2);
        assert!(doc.variables.contains(&"project_name".to_string()));
        assert!(doc.variables.contains(&"package_name".to_string()));
        assert_eq!(doc.edges.len(), 2);
    }
}
