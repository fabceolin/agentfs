# STORY-2.1.1: Core Markdown Parser

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.1.1 |
| **Parent** | STORY-2.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 2 - Parsing and Population |
| **Status** | Done |
| **Priority** | High |
| **File** | `sdk/rust/src/graphdocs/parser.rs` |
| **Dependencies** | STORY-1.1, STORY-1.2 |

## User Story

**As a** developer
**I want** a Markdown parser that extracts document structure
**So that** I can convert existing documents to GraphDocs format

## Acceptance Criteria

- [x] Parse headers (H1-H6) as sections
- [x] Parse paragraphs as sections
- [x] Parse lists as sections
- [x] Parse code blocks as sections

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
# Project Title

Description paragraph.

## Installation

```bash
npm install package
```

## Features

- Feature A
- Feature B
"#;

let parser = MarkdownParser::new();
let doc = parser.parse(markdown)?;

println!("Title: {:?}", doc.title);
println!("Sections: {}", doc.sections.len());
// Output:
// Title: Some("Project Title")
// Sections: 5
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

### Test 2: Parse Code Block
```rust
#[test]
fn test_parse_code_block() {
    let parser = MarkdownParser::new();
    let doc = parser.parse("```rust\nfn main() {}\n```").unwrap();

    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].section_type, SectionType::Code);
}
```

### Test 3: Parse List
```rust
#[test]
fn test_parse_list() {
    let parser = MarkdownParser::new();
    let doc = parser.parse("- Item 1\n- Item 2\n- Item 3").unwrap();

    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].section_type, SectionType::List);
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

### Test 5: Extract Title from H1
```rust
#[test]
fn test_extract_title() {
    let parser = MarkdownParser::new();
    let doc = parser.parse("# My Document\n\nContent here").unwrap();

    assert_eq!(doc.title, Some("My Document".to_string()));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/parser.rs` | Parser implementation |
| `sdk/rust/src/graphdocs/mod.rs` | Module exports |

## Dependencies

```toml
[dependencies]
pulldown-cmark = "0.9"
regex = "1"
uuid = { version = "1", features = ["v4"] }
thiserror = "1"
```

---

## Dev Agent Record

### Agent Model Used
Claude Opus 4.5 (claude-opus-4-5-20251101)

### File List

| File | Status | Description |
|------|--------|-------------|
| `sdk/rust/src/graphdocs/parser.rs` | Modified | Enhanced blockquote and list parsing to handle nested paragraph elements correctly |
| `sdk/rust/src/graphdocs/conformance.rs` | Modified | Fixed section type validation to use parsed section types instead of string matching; added `get_bmad_template` and `get_following_section` methods |
| `sdk/rust/src/graphdocs/normalizer.rs` | Modified | Fixed `extract_see_also` regex to correctly handle "Superseded → See X" patterns; fixed borrow-after-move in `extract_status` |
| `sdk/rust/src/graphdocs/llm_converter.rs` | Modified | Fixed raw string syntax error in test (removed `#` prefix from content) |

### Debug Log References
None required - all issues were resolved during development.

### Completion Notes

1. **Parser Implementation Complete**: The `MarkdownParser` correctly parses all required section types:
   - Headers (H1-H6) with proper level tracking
   - Paragraphs
   - Lists (bullet, numbered)
   - Code blocks
   - Additional: Blockquotes, Horizontal rules

2. **Bug Fixes Applied**:
   - Fixed blockquote parsing: Added depth tracking to prevent nested paragraphs from overriding blockquote type
   - Fixed list parsing: Added depth tracking similar to blockquotes for proper nested element handling
   - Fixed conformance validation: Updated to use parsed `SectionType` instead of raw markdown string matching
   - Fixed normalizer regex: Corrected pattern to extract references like "TEA-001" from "Superseded → See TEA-001"

3. **Test Results**: 227 tests pass, 1 ignored. All parser tests (13) pass. Full regression passes.

4. **Linting**: Clippy passes with only warnings (no errors).

### Change Log

| Date | Change | Reason |
|------|--------|--------|
| 2026-01-16 | Added `blockquote_depth` tracking to parser | Fix blockquote sections being incorrectly identified as paragraphs |
| 2026-01-16 | Added `list_depth` tracking to parser | Fix list sections being affected by nested paragraph events |
| 2026-01-16 | Updated `validate_section_type` in conformance.rs | Use parsed section types instead of string matching for validation |
| 2026-01-16 | Fixed `extract_see_also` regex pattern | Correctly extract reference from "Superseded → See X" format |
| 2026-01-16 | Fixed borrow-after-move in normalizer | Extract notes before moving `raw` into struct |

---

## QA Results

### Review Date: 2026-01-16

### Reviewed By: Quinn (Test Architect)

### Code Quality Assessment

Implementation is clean, idiomatic Rust following project coding standards. The parser uses an efficient event-driven approach via pulldown-cmark with proper depth tracking for nested elements (blockquotes, lists). Test coverage is comprehensive with 13 unit tests covering all acceptance criteria plus additional edge cases.

### Refactoring Performed

No refactoring performed - code quality is already high.

### Compliance Check

- Coding Standards: ✓ Rust 2021 edition, `thiserror` for errors, proper naming
- Project Structure: ✓ Located at `sdk/rust/src/graphdocs/parser.rs`
- Testing Strategy: ✓ Inline unit tests per Rust convention (13 tests)
- All ACs Met: ✓ All 4 acceptance criteria verified with passing tests

### Requirements Traceability

| AC | Requirement | Test(s) | Status |
|----|-------------|---------|--------|
| 1 | Parse headers (H1-H6) | `test_parse_simple`, `test_parse_headers_h1_to_h6` | ✓ |
| 2 | Parse paragraphs | `test_parse_simple` | ✓ |
| 3 | Parse lists | `test_parse_list` | ✓ |
| 4 | Parse code blocks | `test_parse_code_block` | ✓ |

### Improvements Checklist

- [x] Depth tracking for nested blockquotes (already implemented)
- [x] Depth tracking for nested lists (already implemented)
- [x] Comprehensive test coverage for all section types (already implemented)
- [ ] Consider caching regex in `extract_variables` for performance optimization (future)
- [ ] Consider extracting `flush_section` parameters into a struct (future/optional)

### Security Review

No security concerns - this is a pure parsing module with no external input vulnerabilities, no file system access, and no network operations.

### Performance Considerations

- Single-pass, event-driven parsing is efficient
- Minor optimization opportunity: `extract_variables` recompiles regex on each call
- Overall performance is appropriate for the use case

### Files Modified During Review

None - no modifications made during this review.

### Gate Status

Gate: **PASS** → `docs/qa/gates/2.1.1-core-markdown-parser.yml`

### Recommended Status

✓ **Ready for Done** - All acceptance criteria met, tests passing, code quality high.

(Story owner decides final status)
