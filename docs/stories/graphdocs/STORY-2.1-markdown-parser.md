# STORY-2.1: Markdown Parser

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 2 - Parsing and Population |
| **Status** | Todo |
| **Priority** | High |
| **File** | `sdk/rust/src/graphdocs/parser.rs` |
| **Dependencies** | STORY-1.1, STORY-1.2 |

## User Story

**As a** developer
**I want** a Markdown parser for graphs
**So that** I can convert existing documents to GraphDocs format

## Acceptance Criteria

- [ ] Parse headers (H1-H6) as sections
- [ ] Parse paragraphs as sections
- [ ] Parse lists as sections
- [ ] Parse code blocks as sections
- [ ] Detect variables `{{name}}`
- [ ] Generate edge structure

## Technical Specification

### Parser Structure

```rust
// sdk/rust/src/graphdocs/parser.rs

use pulldown_cmark::{Event, Parser, Tag, HeadingLevel};
use uuid::Uuid;

/// Parsed section from Markdown
#[derive(Debug, Clone)]
pub struct ParsedSection {
    pub id: String,
    pub section_type: SectionType,
    pub level: Option<u8>,
    pub content: String,
    pub order_idx: u32,
    pub variables: Vec<String>, // Variable names found in content
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionType {
    Heading,
    Paragraph,
    List,
    Code,
    Table,
    Blockquote,
    HorizontalRule,
}

impl SectionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SectionType::Heading => "heading",
            SectionType::Paragraph => "paragraph",
            SectionType::List => "list",
            SectionType::Code => "code",
            SectionType::Table => "table",
            SectionType::Blockquote => "blockquote",
            SectionType::HorizontalRule => "hr",
        }
    }
}

/// Result of parsing a Markdown document
#[derive(Debug)]
pub struct ParsedDocument {
    pub title: Option<String>,
    pub sections: Vec<ParsedSection>,
    pub variables: Vec<String>, // All unique variable names
    pub edges: Vec<ParsedEdge>,
}

#[derive(Debug)]
pub struct ParsedEdge {
    pub source_idx: usize,
    pub target_idx: usize,
    pub edge_type: EdgeType,
}

#[derive(Debug, Clone, Copy)]
pub enum EdgeType {
    Follows,
    Contains,
}

/// Markdown to GraphDocs parser
pub struct MarkdownParser {
    generate_ids: bool,
}

impl MarkdownParser {
    pub fn new() -> Self {
        Self { generate_ids: true }
    }

    /// Parse Markdown content into structured document
    pub fn parse(&self, content: &str) -> Result<ParsedDocument, ParseError> {
        let parser = Parser::new(content);
        let mut sections = Vec::new();
        let mut current_content = String::new();
        let mut current_type: Option<SectionType> = None;
        let mut current_level: Option<u8> = None;
        let mut order_idx = 0u32;
        let mut all_variables = Vec::new();
        let mut title = None;

        for event in parser {
            match event {
                Event::Start(Tag::Heading(level, _, _)) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                    current_type = Some(SectionType::Heading);
                    current_level = Some(heading_level_to_u8(level));
                }
                Event::End(Tag::Heading(_, _, _)) => {
                    // Extract title from first H1
                    if title.is_none() && current_level == Some(1) {
                        title = Some(current_content.trim().to_string());
                    }
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                }
                Event::Start(Tag::Paragraph) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                    current_type = Some(SectionType::Paragraph);
                }
                Event::End(Tag::Paragraph) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                }
                Event::Start(Tag::CodeBlock(_)) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                    current_type = Some(SectionType::Code);
                }
                Event::End(Tag::CodeBlock(_)) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                }
                Event::Start(Tag::List(_)) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                    current_type = Some(SectionType::List);
                }
                Event::End(Tag::List(_)) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                }
                Event::Start(Tag::BlockQuote) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                    current_type = Some(SectionType::Blockquote);
                }
                Event::End(Tag::BlockQuote) => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                }
                Event::Rule => {
                    self.flush_section(
                        &mut sections,
                        &mut current_content,
                        &mut current_type,
                        &mut current_level,
                        &mut order_idx,
                        &mut all_variables,
                    );
                    sections.push(ParsedSection {
                        id: self.generate_id(),
                        section_type: SectionType::HorizontalRule,
                        level: None,
                        content: "---".to_string(),
                        order_idx,
                        variables: vec![],
                    });
                    order_idx += 1;
                }
                Event::Text(text) | Event::Code(text) => {
                    current_content.push_str(&text);
                }
                Event::SoftBreak | Event::HardBreak => {
                    current_content.push('\n');
                }
                _ => {}
            }
        }

        // Flush any remaining content
        self.flush_section(
            &mut sections,
            &mut current_content,
            &mut current_type,
            &mut current_level,
            &mut order_idx,
            &mut all_variables,
        );

        // Generate edges (sequential follows relationships)
        let edges = self.generate_edges(&sections);

        // Deduplicate variables
        all_variables.sort();
        all_variables.dedup();

        Ok(ParsedDocument {
            title,
            sections,
            variables: all_variables,
            edges,
        })
    }

    fn flush_section(
        &self,
        sections: &mut Vec<ParsedSection>,
        content: &mut String,
        section_type: &mut Option<SectionType>,
        level: &mut Option<u8>,
        order_idx: &mut u32,
        all_variables: &mut Vec<String>,
    ) {
        if let Some(st) = section_type.take() {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                let variables = extract_variables(trimmed);
                all_variables.extend(variables.clone());

                sections.push(ParsedSection {
                    id: self.generate_id(),
                    section_type: st,
                    level: level.take(),
                    content: trimmed.to_string(),
                    order_idx: *order_idx,
                    variables,
                });
                *order_idx += 1;
            }
        }
        content.clear();
        *level = None;
    }

    fn generate_id(&self) -> String {
        if self.generate_ids {
            Uuid::new_v4().to_string()
        } else {
            String::new()
        }
    }

    fn generate_edges(&self, sections: &[ParsedSection]) -> Vec<ParsedEdge> {
        let mut edges = Vec::new();

        // Create "follows" edges between sequential sections
        for i in 0..sections.len().saturating_sub(1) {
            edges.push(ParsedEdge {
                source_idx: i,
                target_idx: i + 1,
                edge_type: EdgeType::Follows,
            });
        }

        edges
    }
}

/// Extract variable names from content ({{variable_name}})
fn extract_variables(content: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\{\{(\w+)\}\}").unwrap();
    re.captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Invalid markdown structure: {0}")]
    InvalidStructure(String),
}
```

### Usage Example

```rust
use graphdocs::parser::MarkdownParser;

let markdown = r#"
# {{project_name}}

{{description}}

## Installation

```bash
{{install_command}}
```

## Features

- Feature A
- Feature B
"#;

let parser = MarkdownParser::new();
let doc = parser.parse(markdown)?;

println!("Title: {:?}", doc.title);
println!("Sections: {}", doc.sections.len());
println!("Variables: {:?}", doc.variables);
// Output:
// Title: Some("{{project_name}}")
// Sections: 5
// Variables: ["description", "install_command", "project_name"]
```

### Database Insertion

```rust
impl ParsedDocument {
    /// Insert parsed document into database
    pub async fn insert_into(
        &self,
        conn: &DuckConnection,
        doc_id: &str,
        title: &str,
    ) -> Result<()> {
        // Insert document
        conn.execute(
            "INSERT INTO gd_documents (id, title) VALUES (?, ?)",
            params![doc_id, title],
        )?;

        // Insert sections
        for section in &self.sections {
            conn.execute(
                r#"INSERT INTO gd_sections
                   (id, document_id, section_type, level, order_idx, content)
                   VALUES (?, ?, ?, ?, ?, ?)"#,
                params![
                    section.id,
                    doc_id,
                    section.section_type.as_str(),
                    section.level,
                    section.order_idx,
                    section.content,
                ],
            )?;
        }

        // Insert placeholder variables
        for var_name in &self.variables {
            let var_id = Uuid::new_v4().to_string();
            conn.execute(
                r#"INSERT INTO gd_variables (id, document_id, name, value, var_type)
                   VALUES (?, ?, ?, '""', 'string')"#,
                params![var_id, doc_id, var_name],
            )?;
        }

        // Insert edges
        for edge in &self.edges {
            let edge_id = Uuid::new_v4().to_string();
            let source_id = &self.sections[edge.source_idx].id;
            let target_id = &self.sections[edge.target_idx].id;
            conn.execute(
                r#"INSERT INTO gd_edges (id, source_id, target_id, edge_type)
                   VALUES (?, ?, ?, ?)"#,
                params![
                    edge_id,
                    source_id,
                    target_id,
                    match edge.edge_type {
                        EdgeType::Follows => "follows",
                        EdgeType::Contains => "contains",
                    }
                ],
            )?;
        }

        Ok(())
    }
}
```

## Tests

### Test 1: Parse Simple Document
```rust
#[test]
fn test_parse_simple() {
    let parser = MarkdownParser::new();
    let doc = parser.parse("# Hello\n\nWorld").unwrap();

    assert_eq!(doc.sections.len(), 2);
    assert_eq!(doc.sections[0].section_type, SectionType::Heading);
    assert_eq!(doc.sections[0].level, Some(1));
    assert_eq!(doc.sections[1].section_type, SectionType::Paragraph);
}
```

### Test 2: Extract Variables
```rust
#[test]
fn test_extract_variables() {
    let parser = MarkdownParser::new();
    let doc = parser.parse("# {{title}}\n\n{{description}}").unwrap();

    assert_eq!(doc.variables, vec!["description", "title"]);
    assert_eq!(doc.sections[0].variables, vec!["title"]);
    assert_eq!(doc.sections[1].variables, vec!["description"]);
}
```

### Test 3: Parse Code Block
```rust
#[test]
fn test_parse_code_block() {
    let parser = MarkdownParser::new();
    let doc = parser.parse("```rust\nfn main() {}\n```").unwrap();

    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].section_type, SectionType::Code);
}
```

### Test 4: Generate Edges
```rust
#[test]
fn test_generate_edges() {
    let parser = MarkdownParser::new();
    let doc = parser.parse("# A\n\nB\n\nC").unwrap();

    assert_eq!(doc.edges.len(), 2); // A->B, B->C
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/parser.rs` | Parser implementation |
| `sdk/rust/src/graphdocs/mod.rs` | Module exports |
| `Cargo.toml` | Dependencies (pulldown-cmark, regex) |

## Dependencies

```toml
[dependencies]
pulldown-cmark = "0.9"
regex = "1"
uuid = { version = "1", features = ["v4"] }
```
