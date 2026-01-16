# STORY-2.1.2: Variable Detection and Typing

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.1.2 |
| **Parent** | STORY-2.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 2 - Parsing and Population |
| **Status** | Ready for Development |
| **Priority** | High |
| **File** | `sdk/rust/src/graphdocs/variable_types.rs` |
| **Dependencies** | STORY-2.1.1 |

## User Story

**As a** developer
**I want** variables detected in markdown to be automatically typed
**So that** I can validate document data against expected schemas

## Acceptance Criteria

- [ ] Detect variables `{{name}}` with typed inference
- [ ] Support variable types: `bool`, `enum`, `number`, `string`, `string[]`, `object`
- [ ] Parse YAML frontmatter for type hints and enum definitions

## Technical Specification

### Variable Types

Based on analysis of BMAD template YAML files, variables should be typed:

```rust
// sdk/rust/src/graphdocs/variable_types.rs

use serde::{Deserialize, Serialize};

/// Variable type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableType {
    Bool,
    Enum,
    Number,
    String,
    StringArray,
    Object,
}

impl VariableType {
    pub fn as_str(&self) -> &'static str {
        match self {
            VariableType::Bool => "bool",
            VariableType::Enum => "enum",
            VariableType::Number => "number",
            VariableType::String => "string",
            VariableType::StringArray => "string[]",
            VariableType::Object => "object",
        }
    }
}

/// Parsed variable with type information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedVariable {
    pub name: String,
    pub var_type: VariableType,
    pub enum_values: Option<Vec<String>>,  // Only for enum type
    pub default_value: Option<serde_json::Value>,
    pub required: bool,
    pub description: Option<String>,
}
```

### Known Variable Patterns

```rust
/// Common enums discovered from BMAD templates
pub mod common_enums {
    /// Story lifecycle status
    pub const STORY_STATUS: &[&str] = &["Draft", "Approved", "InProgress", "Review", "Done"];

    /// QA Gate decision
    pub const GATE_DECISION: &[&str] = &["PASS", "CONCERNS", "FAIL", "WAIVED"];

    /// Issue severity levels
    pub const SEVERITY: &[&str] = &["low", "medium", "high"];

    /// Workflow interaction mode
    pub const WORKFLOW_MODE: &[&str] = &["interactive", "non-interactive"];

    /// Workflow project type
    pub const WORKFLOW_TYPE: &[&str] = &["greenfield", "brownfield"];

    /// Section rendering types
    pub const SECTION_TYPE: &[&str] = &[
        "bullet-list", "numbered-list", "table", "mermaid",
        "code", "checklist", "template-text", "paragraphs", "choice"
    ];

    /// Mermaid diagram types
    pub const MERMAID_TYPE: &[&str] = &["graph", "sequence", "flowchart", "erDiagram"];

    /// Code block languages
    pub const CODE_LANGUAGE: &[&str] = &[
        "typescript", "javascript", "yaml", "json", "sql",
        "css", "bash", "graphql", "plaintext", "text"
    ];

    /// Output format
    pub const OUTPUT_FORMAT: &[&str] = &["markdown", "yaml"];

    /// Accessibility levels
    pub const ACCESSIBILITY: &[&str] = &["None", "WCAG AA", "WCAG AAA"];

    /// Target platforms
    pub const TARGET_PLATFORM: &[&str] = &[
        "Web Responsive", "Mobile Only", "Desktop Only", "Cross-Platform"
    ];

    /// Repository structure
    pub const REPOSITORY_STRUCTURE: &[&str] = &["Monorepo", "Polyrepo", "Multi-repo"];

    /// Service architecture
    pub const SERVICE_ARCHITECTURE: &[&str] = &["Monolith", "Microservices", "Serverless"];

    /// Testing strategy
    pub const TESTING_STRATEGY: &[&str] = &[
        "Unit Only", "Unit + Integration", "Full Testing Pyramid"
    ];
}

/// Known boolean variables from BMAD templates
pub const BOOL_VARIABLES: &[&str] = &[
    "elicit", "repeatable", "optional", "modified",
    "markdownExploder", "prdSharded", "architectureSharded",
    "active", "multiSelect", "required"
];

/// Known number variables from BMAD templates
pub const NUMBER_VARIABLES: &[&str] = &[
    "epic_num", "epic_number", "story_num", "story_number",
    "priority_level", "segment_number", "opportunity_number",
    "criterion_number", "total_ideas", "quality_score",
    "tests_reviewed", "risks_identified", "critical", "high", "medium", "low"
];

/// Known enum variables with their enum type
pub const ENUM_VARIABLES: &[(&str, &str)] = &[
    ("status", "STORY_STATUS"),
    ("gate", "GATE_DECISION"),
    ("severity", "SEVERITY"),
    ("mode", "WORKFLOW_MODE"),
    ("type", "SECTION_TYPE"),
    ("mermaid_type", "MERMAID_TYPE"),
    ("language", "CODE_LANGUAGE"),
    ("format", "OUTPUT_FORMAT"),
    ("accessibility", "ACCESSIBILITY"),
    ("platforms", "TARGET_PLATFORM"),
    ("repository", "REPOSITORY_STRUCTURE"),
    ("architecture", "SERVICE_ARCHITECTURE"),
    ("testing", "TESTING_STRATEGY"),
    ("workflow_type", "WORKFLOW_TYPE"),
];

/// Known string array variables
pub const STRING_ARRAY_VARIABLES: &[&str] = &[
    "agents", "workflows", "project_types", "optional_steps",
    "requires", "columns", "items", "editable_sections", "editors",
    "ides_setup", "expansion_packs", "devLoadAlwaysFiles",
    "ac_covered", "ac_gaps", "must_fix", "monitor", "choices"
];
```

### Type Inference

```rust
/// Infer variable type from name using known patterns
pub fn infer_variable_type(name: &str) -> ParsedVariable {
    // Check if it's a known boolean
    if BOOL_VARIABLES.contains(&name) {
        return ParsedVariable {
            name: name.to_string(),
            var_type: VariableType::Bool,
            enum_values: None,
            default_value: Some(serde_json::Value::Bool(false)),
            required: false,
            description: None,
        };
    }

    // Check if it's a known number
    if NUMBER_VARIABLES.contains(&name) {
        return ParsedVariable {
            name: name.to_string(),
            var_type: VariableType::Number,
            enum_values: None,
            default_value: Some(serde_json::Value::Number(0.into())),
            required: false,
            description: None,
        };
    }

    // Check if it's a known enum
    if let Some((_, enum_type)) = ENUM_VARIABLES.iter().find(|(n, _)| *n == name) {
        let values = match *enum_type {
            "STORY_STATUS" => common_enums::STORY_STATUS,
            "GATE_DECISION" => common_enums::GATE_DECISION,
            "SEVERITY" => common_enums::SEVERITY,
            "WORKFLOW_MODE" => common_enums::WORKFLOW_MODE,
            "SECTION_TYPE" => common_enums::SECTION_TYPE,
            "MERMAID_TYPE" => common_enums::MERMAID_TYPE,
            "CODE_LANGUAGE" => common_enums::CODE_LANGUAGE,
            "OUTPUT_FORMAT" => common_enums::OUTPUT_FORMAT,
            "ACCESSIBILITY" => common_enums::ACCESSIBILITY,
            "TARGET_PLATFORM" => common_enums::TARGET_PLATFORM,
            "REPOSITORY_STRUCTURE" => common_enums::REPOSITORY_STRUCTURE,
            "SERVICE_ARCHITECTURE" => common_enums::SERVICE_ARCHITECTURE,
            "TESTING_STRATEGY" => common_enums::TESTING_STRATEGY,
            "WORKFLOW_TYPE" => common_enums::WORKFLOW_TYPE,
            _ => &[],
        };
        return ParsedVariable {
            name: name.to_string(),
            var_type: VariableType::Enum,
            enum_values: Some(values.iter().map(|s| s.to_string()).collect()),
            default_value: values.first().map(|v| serde_json::Value::String(v.to_string())),
            required: false,
            description: None,
        };
    }

    // Check if it's a known string array
    if STRING_ARRAY_VARIABLES.contains(&name) {
        return ParsedVariable {
            name: name.to_string(),
            var_type: VariableType::StringArray,
            enum_values: None,
            default_value: Some(serde_json::Value::Array(vec![])),
            required: false,
            description: None,
        };
    }

    // Default to string
    ParsedVariable {
        name: name.to_string(),
        var_type: VariableType::String,
        enum_values: None,
        default_value: Some(serde_json::Value::String(String::new())),
        required: false,
        description: None,
    }
}

/// Convert raw variable names to typed ParsedVariables
pub fn variables_to_typed(names: Vec<String>) -> Vec<ParsedVariable> {
    names.into_iter()
        .map(|name| infer_variable_type(&name))
        .collect()
}
```

### YAML Frontmatter Parsing

```rust
use serde_yaml;

/// YAML frontmatter with variable type hints
#[derive(Debug, Deserialize)]
pub struct Frontmatter {
    #[serde(default)]
    pub variables: HashMap<String, VariableDefinition>,
}

#[derive(Debug, Deserialize)]
pub struct VariableDefinition {
    #[serde(rename = "type")]
    pub var_type: String,
    #[serde(default)]
    pub enum_values: Option<Vec<String>>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
}

/// Extract YAML frontmatter from markdown content
pub fn extract_frontmatter(content: &str) -> Option<(Frontmatter, &str)> {
    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    if let Some(end_idx) = rest.find("\n---") {
        let yaml_str = &rest[..end_idx];
        let remaining = &rest[end_idx + 4..];

        match serde_yaml::from_str::<Frontmatter>(yaml_str) {
            Ok(fm) => Some((fm, remaining.trim_start())),
            Err(_) => None,
        }
    } else {
        None
    }
}

/// Merge frontmatter definitions with inferred types
pub fn merge_with_frontmatter(
    inferred: Vec<ParsedVariable>,
    frontmatter: &Frontmatter,
) -> Vec<ParsedVariable> {
    inferred.into_iter().map(|mut var| {
        if let Some(def) = frontmatter.variables.get(&var.name) {
            // Override with frontmatter definition
            var.var_type = match def.var_type.as_str() {
                "bool" => VariableType::Bool,
                "enum" => VariableType::Enum,
                "number" => VariableType::Number,
                "string[]" => VariableType::StringArray,
                "object" => VariableType::Object,
                _ => VariableType::String,
            };
            if let Some(ref values) = def.enum_values {
                var.enum_values = Some(values.clone());
            }
            if let Some(ref default) = def.default {
                var.default_value = Some(default.clone());
            }
            var.required = def.required;
            var.description = def.description.clone();
        }
        var
    }).collect()
}
```

### Database Insertion

```rust
impl ParsedDocument {
    /// Insert typed variables into database
    pub async fn insert_variables(
        &self,
        conn: &DuckConnection,
        doc_id: &str,
    ) -> Result<()> {
        for var in &self.variables {
            let var_id = Uuid::new_v4().to_string();
            let default_json = var.default_value.as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string());
            let enum_json = var.enum_values.as_ref()
                .map(|v| serde_json::to_string(v).unwrap())
                .unwrap_or_else(|| "null".to_string());

            conn.execute(
                r#"INSERT INTO gd_variables
                   (id, document_id, name, value, var_type, enum_values, required, description)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
                params![
                    var_id,
                    doc_id,
                    var.name,
                    default_json,
                    var.var_type.as_str(),
                    enum_json,
                    var.required,
                    var.description,
                ],
            )?;
        }
        Ok(())
    }
}
```

## Tests

### Test 1: Extract Variables with Types
```rust
#[test]
fn test_extract_variables() {
    let parser = MarkdownParser::new();
    let doc = parser.parse("# {{title}}\n\n{{description}}").unwrap();

    assert_eq!(doc.variables.len(), 2);
    // Both should be string type (unknown variables default to string)
    assert!(doc.variables.iter().all(|v| v.var_type == VariableType::String));
}
```

### Test 2: Infer Boolean Variables
```rust
#[test]
fn test_infer_bool_variable() {
    let var = infer_variable_type("elicit");
    assert_eq!(var.var_type, VariableType::Bool);
    assert_eq!(var.default_value, Some(serde_json::Value::Bool(false)));

    let var2 = infer_variable_type("repeatable");
    assert_eq!(var2.var_type, VariableType::Bool);
}
```

### Test 3: Infer Enum Variables
```rust
#[test]
fn test_infer_enum_variable() {
    let var = infer_variable_type("status");
    assert_eq!(var.var_type, VariableType::Enum);
    assert_eq!(
        var.enum_values,
        Some(vec!["Draft", "Approved", "InProgress", "Review", "Done"]
            .into_iter().map(String::from).collect())
    );

    let var2 = infer_variable_type("gate");
    assert_eq!(var2.var_type, VariableType::Enum);
    assert!(var2.enum_values.as_ref().unwrap().contains(&"PASS".to_string()));
}
```

### Test 4: Infer Number Variables
```rust
#[test]
fn test_infer_number_variable() {
    let var = infer_variable_type("epic_num");
    assert_eq!(var.var_type, VariableType::Number);

    let var2 = infer_variable_type("story_num");
    assert_eq!(var2.var_type, VariableType::Number);
}
```

### Test 5: Infer String Array Variables
```rust
#[test]
fn test_infer_string_array_variable() {
    let var = infer_variable_type("agents");
    assert_eq!(var.var_type, VariableType::StringArray);
    assert_eq!(var.default_value, Some(serde_json::Value::Array(vec![])));
}
```

### Test 6: Parse YAML Frontmatter
```rust
#[test]
fn test_parse_frontmatter() {
    let content = r#"---
variables:
  custom_status:
    type: enum
    enum_values: ["Open", "Closed", "Pending"]
    required: true
---
# Document

Status: {{custom_status}}
"#;

    let (fm, remaining) = extract_frontmatter(content).unwrap();
    assert!(fm.variables.contains_key("custom_status"));
    assert!(remaining.starts_with("# Document"));
}
```

### Test 7: Merge Frontmatter with Inferred
```rust
#[test]
fn test_merge_frontmatter() {
    let inferred = vec![
        infer_variable_type("custom_field"),  // defaults to string
    ];

    let mut fm = Frontmatter { variables: HashMap::new() };
    fm.variables.insert("custom_field".to_string(), VariableDefinition {
        var_type: "number".to_string(),
        enum_values: None,
        default: Some(serde_json::json!(42)),
        required: true,
        description: Some("A custom number".to_string()),
    });

    let merged = merge_with_frontmatter(inferred, &fm);
    assert_eq!(merged[0].var_type, VariableType::Number);
    assert_eq!(merged[0].default_value, Some(serde_json::json!(42)));
    assert!(merged[0].required);
}
```

## Variable Type Reference

| Type | Description | Example Variables |
|------|-------------|-------------------|
| `bool` | Boolean flags | `elicit`, `repeatable`, `optional`, `modified`, `required` |
| `enum` | Constrained choice | `status`, `gate`, `severity`, `mode`, `type` |
| `number` | Numeric values | `epic_num`, `story_num`, `priority_level`, `quality_score` |
| `string` | Free text | `project_name`, `title`, `description`, `user_type` |
| `string[]` | String arrays | `agents`, `workflows`, `columns`, `items`, `choices` |
| `object` | Complex structures | (reserved for nested YAML structures) |

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/variable_types.rs` | Variable type definitions and inference |
| `sdk/rust/src/graphdocs/parser.rs` | Parser integration (STORY-2.1.1) |
| `sdk/rust/src/graphdocs/mod.rs` | Module exports |

## Dependencies

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
```
