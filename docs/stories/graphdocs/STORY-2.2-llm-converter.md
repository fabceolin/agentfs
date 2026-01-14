# STORY-2.2: LLM Schema Converter

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.2 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 2 - Parsing and Population |
| **Status** | Todo |
| **Priority** | Medium |
| **File** | `sdk/rust/src/graphdocs/llm_converter.rs` |
| **Dependencies** | STORY-2.1 |

## User Story

**As a** developer
**I want** to convert free text into structured schema via LLM
**So that** I can populate graphs automatically with intelligent extraction

## Acceptance Criteria

- [ ] Prompt template for structure extraction
- [ ] Validation of LLM output
- [ ] Fallback to deterministic parser

## Technical Specification

### LLM Converter Structure

```rust
// sdk/rust/src/graphdocs/llm_converter.rs

use serde::{Deserialize, Serialize};
use crate::graphdocs::parser::{ParsedDocument, ParsedSection, SectionType};

/// LLM-based document converter
pub struct LLMConverter {
    client: Box<dyn LLMClient>,
    model: String,
    fallback_parser: MarkdownParser,
}

/// Trait for LLM API clients
#[async_trait]
pub trait LLMClient: Send + Sync {
    async fn complete(&self, prompt: &str, model: &str) -> Result<String, LLMError>;
}

/// Schema expected from LLM output
#[derive(Debug, Serialize, Deserialize)]
pub struct LLMDocumentSchema {
    pub title: String,
    pub description: Option<String>,
    pub sections: Vec<LLMSection>,
    pub variables: Vec<LLMVariable>,
    pub relationships: Vec<LLMRelationship>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LLMSection {
    pub id: String,
    pub section_type: String, // heading, paragraph, list, code, etc.
    pub level: Option<u8>,
    pub content: String,
    pub order: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LLMVariable {
    pub name: String,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub var_type: String, // string, number, boolean, array
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LLMRelationship {
    pub from_section: String,
    pub to_section: String,
    pub relationship_type: String, // follows, contains, references
}

impl LLMConverter {
    pub fn new(client: Box<dyn LLMClient>, model: String) -> Self {
        Self {
            client,
            model,
            fallback_parser: MarkdownParser::new(),
        }
    }

    /// Convert content using LLM with fallback
    pub async fn convert(&self, content: &str) -> Result<ParsedDocument, ConvertError> {
        // Try LLM conversion first
        match self.convert_with_llm(content).await {
            Ok(doc) => Ok(doc),
            Err(e) => {
                tracing::warn!("LLM conversion failed, using fallback: {}", e);
                self.fallback_parser.parse(content)
                    .map_err(|e| ConvertError::ParseError(e.to_string()))
            }
        }
    }

    async fn convert_with_llm(&self, content: &str) -> Result<ParsedDocument, ConvertError> {
        let prompt = self.build_prompt(content);
        let response = self.client.complete(&prompt, &self.model).await?;
        let schema = self.parse_response(&response)?;
        self.validate_schema(&schema)?;
        Ok(self.schema_to_document(schema))
    }

    fn build_prompt(&self, content: &str) -> String {
        format!(r#"
You are a document structure analyzer. Analyze the following document and extract its structure as JSON.

## Instructions

1. Identify the document title (usually the first heading)
2. Break the document into logical sections
3. Identify any template variables (text in {{braces}})
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
      "section_type": "heading|paragraph|list|code|table|blockquote",
      "level": 1,  // For headings only, 1-6
      "content": "Section content with {{variables}}",
      "order": 0
    }}
  ],
  "variables": [
    {{
      "name": "variable_name",
      "default_value": "",
      "description": "What this variable represents",
      "var_type": "string|number|boolean|array"
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

"#)
    }

    fn parse_response(&self, response: &str) -> Result<LLMDocumentSchema, ConvertError> {
        // Extract JSON from response (handle markdown code blocks)
        let json_str = self.extract_json(response)?;

        serde_json::from_str(&json_str)
            .map_err(|e| ConvertError::InvalidJson(e.to_string()))
    }

    fn extract_json(&self, response: &str) -> Result<String, ConvertError> {
        // Try to find JSON in code block
        if let Some(start) = response.find("```json") {
            let after_start = &response[start + 7..];
            if let Some(end) = after_start.find("```") {
                return Ok(after_start[..end].trim().to_string());
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

    fn validate_schema(&self, schema: &LLMDocumentSchema) -> Result<(), ConvertError> {
        // Validate section types
        let valid_types = ["heading", "paragraph", "list", "code", "table", "blockquote", "hr"];
        for section in &schema.sections {
            if !valid_types.contains(&section.section_type.as_str()) {
                return Err(ConvertError::InvalidSectionType(section.section_type.clone()));
            }
        }

        // Validate heading levels
        for section in &schema.sections {
            if section.section_type == "heading" {
                if let Some(level) = section.level {
                    if level < 1 || level > 6 {
                        return Err(ConvertError::InvalidHeadingLevel(level));
                    }
                }
            }
        }

        // Validate variable types
        let valid_var_types = ["string", "number", "boolean", "array", "object"];
        for var in &schema.variables {
            if !valid_var_types.contains(&var.var_type.as_str()) {
                return Err(ConvertError::InvalidVarType(var.var_type.clone()));
            }
        }

        // Validate relationship references
        let section_ids: std::collections::HashSet<_> =
            schema.sections.iter().map(|s| s.id.as_str()).collect();
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

    fn schema_to_document(&self, schema: LLMDocumentSchema) -> ParsedDocument {
        let sections = schema.sections.iter().map(|s| {
            ParsedSection {
                id: s.id.clone(),
                section_type: match s.section_type.as_str() {
                    "heading" => SectionType::Heading,
                    "paragraph" => SectionType::Paragraph,
                    "list" => SectionType::List,
                    "code" => SectionType::Code,
                    "table" => SectionType::Table,
                    "blockquote" => SectionType::Blockquote,
                    _ => SectionType::Paragraph,
                },
                level: s.level,
                content: s.content.clone(),
                order_idx: s.order,
                variables: extract_variables(&s.content),
            }
        }).collect();

        let variables = schema.variables.iter()
            .map(|v| v.name.clone())
            .collect();

        let edges = schema.relationships.iter()
            .filter_map(|r| {
                let source_idx = schema.sections.iter()
                    .position(|s| s.id == r.from_section)?;
                let target_idx = schema.sections.iter()
                    .position(|s| s.id == r.to_section)?;
                Some(ParsedEdge {
                    source_idx,
                    target_idx,
                    edge_type: match r.relationship_type.as_str() {
                        "contains" => EdgeType::Contains,
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

#[derive(Debug, thiserror::Error)]
pub enum LLMError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Rate limited")]
    RateLimited,
    #[error("Timeout")]
    Timeout,
}
```

### OpenAI Client Implementation

```rust
// sdk/rust/src/graphdocs/openai.rs

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OpenAIClient {
    client: Client,
    api_key: String,
    base_url: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}

impl OpenAIClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    async fn complete(&self, prompt: &str, model: &str) -> Result<String, LLMError> {
        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            temperature: 0.1, // Low temperature for structured output
        };

        let response = self.client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| LLMError::ApiError(e.to_string()))?;

        if response.status() == 429 {
            return Err(LLMError::RateLimited);
        }

        let chat_response: ChatResponse = response.json().await
            .map_err(|e| LLMError::ApiError(e.to_string()))?;

        chat_response.choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| LLMError::ApiError("No response".to_string()))
    }
}
```

### Usage Example

```rust
use graphdocs::llm_converter::{LLMConverter, OpenAIClient};

// Create converter with OpenAI
let client = OpenAIClient::new(std::env::var("OPENAI_API_KEY")?);
let converter = LLMConverter::new(Box::new(client), "gpt-4".to_string());

let content = r#"
# Project Documentation

This is my awesome project that does amazing things.

## Getting Started

First, install the dependencies:

```bash
npm install
```

Then run the application:

```bash
npm start
```

## Configuration

Set the following environment variables:
- API_KEY: Your API key
- DEBUG: Enable debug mode
"#;

let doc = converter.convert(content).await?;
println!("Extracted {} sections", doc.sections.len());
println!("Found variables: {:?}", doc.variables);
```

## Tests

### Test 1: Valid LLM Response
```rust
#[tokio::test]
async fn test_valid_response() {
    let mock_response = r#"
    ```json
    {
        "title": "Test",
        "sections": [
            {"id": "s1", "section_type": "heading", "level": 1, "content": "# Test", "order": 0}
        ],
        "variables": [],
        "relationships": []
    }
    ```
    "#;

    let converter = LLMConverter::new(MockClient::new(mock_response), "test".into());
    let doc = converter.convert("# Test").await.unwrap();

    assert_eq!(doc.title, Some("Test".to_string()));
}
```

### Test 2: Fallback on Invalid Response
```rust
#[tokio::test]
async fn test_fallback() {
    let converter = LLMConverter::new(
        MockClient::new("invalid json"),
        "test".into()
    );

    // Should fall back to deterministic parser
    let doc = converter.convert("# Test\n\nParagraph").await.unwrap();
    assert_eq!(doc.sections.len(), 2);
}
```

### Test 3: Validate Schema
```rust
#[test]
fn test_validate_invalid_section_type() {
    let schema = LLMDocumentSchema {
        title: "Test".into(),
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

    let converter = LLMConverter::new(MockClient::new(""), "".into());
    assert!(converter.validate_schema(&schema).is_err());
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/llm_converter.rs` | LLM converter |
| `sdk/rust/src/graphdocs/openai.rs` | OpenAI client |
| `sdk/rust/src/graphdocs/parser.rs` | Fallback parser |

## Implementation Notes

1. **Low Temperature**: Use low temperature (0.1) for consistent structured output
2. **JSON Extraction**: Handle both raw JSON and markdown code blocks
3. **Validation**: Strict validation prevents invalid data in database
4. **Fallback**: Always fall back to deterministic parser on failure
5. **Rate Limiting**: Handle rate limits gracefully with retries
