use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
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
    Checklist,
    Choice,
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
            SectionType::Checklist => "checklist",
            SectionType::Choice => "choice",
        }
    }
}

/// Result of parsing a Markdown document
#[derive(Debug)]
pub struct ParsedDocument {
    pub title: Option<String>,
    pub sections: Vec<ParsedSection>,
    pub variables: Vec<String>, // Raw variable names (typed in STORY-2.1.2)
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

impl Default for MarkdownParser {
    fn default() -> Self {
        Self::new()
    }
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
        let mut blockquote_depth = 0u32;
        let mut list_depth = 0u32;

        for event in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
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
                Event::End(pulldown_cmark::TagEnd::Heading(_)) => {
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
                    // Don't start a new paragraph section if we're inside a blockquote or list
                    if blockquote_depth == 0 && list_depth == 0 {
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
                }
                Event::End(pulldown_cmark::TagEnd::Paragraph) => {
                    // Only flush paragraph if not inside a blockquote or list
                    if blockquote_depth == 0 && list_depth == 0 {
                        self.flush_section(
                            &mut sections,
                            &mut current_content,
                            &mut current_type,
                            &mut current_level,
                            &mut order_idx,
                            &mut all_variables,
                        );
                    }
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
                Event::End(pulldown_cmark::TagEnd::CodeBlock) => {
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
                    // Only flush and set type at the outermost list
                    if list_depth == 0 {
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
                    list_depth += 1;
                }
                Event::End(pulldown_cmark::TagEnd::List(_)) => {
                    list_depth = list_depth.saturating_sub(1);
                    // Only flush at the outermost list
                    if list_depth == 0 {
                        self.flush_section(
                            &mut sections,
                            &mut current_content,
                            &mut current_type,
                            &mut current_level,
                            &mut order_idx,
                            &mut all_variables,
                        );
                    }
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    // Only flush and set type at the outermost blockquote
                    if blockquote_depth == 0 {
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
                    blockquote_depth += 1;
                }
                Event::End(pulldown_cmark::TagEnd::BlockQuote(_)) => {
                    blockquote_depth = blockquote_depth.saturating_sub(1);
                    // Only flush at the outermost blockquote
                    if blockquote_depth == 0 {
                        self.flush_section(
                            &mut sections,
                            &mut current_content,
                            &mut current_type,
                            &mut current_level,
                            &mut order_idx,
                            &mut all_variables,
                        );
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("# Hello\n\nWorld").unwrap();

        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].section_type, SectionType::Heading);
        assert_eq!(doc.sections[0].level, Some(1));
        assert_eq!(doc.sections[1].section_type, SectionType::Paragraph);
    }

    #[test]
    fn test_parse_code_block() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("```rust\nfn main() {}\n```").unwrap();

        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].section_type, SectionType::Code);
    }

    #[test]
    fn test_parse_list() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("- Item 1\n- Item 2\n- Item 3").unwrap();

        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].section_type, SectionType::List);
    }

    #[test]
    fn test_generate_edges() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("# A\n\nB\n\nC").unwrap();

        assert_eq!(doc.edges.len(), 2); // A->B, B->C
    }

    #[test]
    fn test_extract_title() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("# My Document\n\nContent here").unwrap();

        assert_eq!(doc.title, Some("My Document".to_string()));
    }

    #[test]
    fn test_parse_headers_h1_to_h6() {
        let parser = MarkdownParser::new();
        let markdown = "# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6";
        let doc = parser.parse(markdown).unwrap();

        assert_eq!(doc.sections.len(), 6);
        for (i, section) in doc.sections.iter().enumerate() {
            assert_eq!(section.section_type, SectionType::Heading);
            assert_eq!(section.level, Some((i + 1) as u8));
        }
    }

    #[test]
    fn test_parse_blockquote() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("> This is a quote").unwrap();

        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].section_type, SectionType::Blockquote);
    }

    #[test]
    fn test_parse_horizontal_rule() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("Before\n\n---\n\nAfter").unwrap();

        assert_eq!(doc.sections.len(), 3);
        assert_eq!(doc.sections[1].section_type, SectionType::HorizontalRule);
    }

    #[test]
    fn test_extract_variables_from_content() {
        let parser = MarkdownParser::new();
        let doc = parser
            .parse("Hello {{name}}, your order {{order_id}} is ready")
            .unwrap();

        assert!(doc.variables.contains(&"name".to_string()));
        assert!(doc.variables.contains(&"order_id".to_string()));
    }

    #[test]
    fn test_section_type_as_str() {
        assert_eq!(SectionType::Heading.as_str(), "heading");
        assert_eq!(SectionType::Paragraph.as_str(), "paragraph");
        assert_eq!(SectionType::List.as_str(), "list");
        assert_eq!(SectionType::Code.as_str(), "code");
        assert_eq!(SectionType::Table.as_str(), "table");
        assert_eq!(SectionType::Blockquote.as_str(), "blockquote");
        assert_eq!(SectionType::HorizontalRule.as_str(), "hr");
        assert_eq!(SectionType::Checklist.as_str(), "checklist");
        assert_eq!(SectionType::Choice.as_str(), "choice");
    }

    #[test]
    fn test_order_idx_sequential() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("# First\n\nSecond\n\n# Third").unwrap();

        for (i, section) in doc.sections.iter().enumerate() {
            assert_eq!(section.order_idx, i as u32);
        }
    }

    #[test]
    fn test_edge_follows_relationship() {
        let parser = MarkdownParser::new();
        let doc = parser.parse("# A\n\nB\n\nC\n\nD").unwrap();

        assert_eq!(doc.edges.len(), 3);
        for (i, edge) in doc.edges.iter().enumerate() {
            assert_eq!(edge.source_idx, i);
            assert_eq!(edge.target_idx, i + 1);
            assert!(matches!(edge.edge_type, EdgeType::Follows));
        }
    }

    #[test]
    fn test_default_implementation() {
        let parser = MarkdownParser::default();
        let doc = parser.parse("# Test").unwrap();
        assert_eq!(doc.sections.len(), 1);
    }
}
