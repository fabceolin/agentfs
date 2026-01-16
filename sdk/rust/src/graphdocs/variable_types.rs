//! Variable detection and typing for GraphDocs markdown templates.
//!
//! This module provides functionality to detect variables in markdown content
//! (using `{{name}}` syntax) and infer their types based on known patterns
//! or YAML frontmatter definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// Returns the string representation of the variable type
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

impl std::fmt::Display for VariableType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Parsed variable with type information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedVariable {
    pub name: String,
    pub var_type: VariableType,
    pub enum_values: Option<Vec<String>>,
    pub default_value: Option<serde_json::Value>,
    pub required: bool,
    pub description: Option<String>,
}

impl ParsedVariable {
    /// Create a new ParsedVariable with the given name and type
    pub fn new(name: impl Into<String>, var_type: VariableType) -> Self {
        Self {
            name: name.into(),
            var_type,
            enum_values: None,
            default_value: None,
            required: false,
            description: None,
        }
    }

    /// Set enum values for an enum type variable
    pub fn with_enum_values(mut self, values: Vec<String>) -> Self {
        self.enum_values = Some(values);
        self
    }

    /// Set the default value
    pub fn with_default(mut self, value: serde_json::Value) -> Self {
        self.default_value = Some(value);
        self
    }

    /// Set whether the variable is required
    pub fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

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
        "bullet-list",
        "numbered-list",
        "table",
        "mermaid",
        "code",
        "checklist",
        "template-text",
        "paragraphs",
        "choice",
    ];

    /// Mermaid diagram types
    pub const MERMAID_TYPE: &[&str] = &["graph", "sequence", "flowchart", "erDiagram"];

    /// Code block languages
    pub const CODE_LANGUAGE: &[&str] = &[
        "typescript",
        "javascript",
        "yaml",
        "json",
        "sql",
        "css",
        "bash",
        "graphql",
        "plaintext",
        "text",
    ];

    /// Output format
    pub const OUTPUT_FORMAT: &[&str] = &["markdown", "yaml"];

    /// Accessibility levels
    pub const ACCESSIBILITY: &[&str] = &["None", "WCAG AA", "WCAG AAA"];

    /// Target platforms
    pub const TARGET_PLATFORM: &[&str] = &[
        "Web Responsive",
        "Mobile Only",
        "Desktop Only",
        "Cross-Platform",
    ];

    /// Repository structure
    pub const REPOSITORY_STRUCTURE: &[&str] = &["Monorepo", "Polyrepo", "Multi-repo"];

    /// Service architecture
    pub const SERVICE_ARCHITECTURE: &[&str] = &["Monolith", "Microservices", "Serverless"];

    /// Testing strategy
    pub const TESTING_STRATEGY: &[&str] =
        &["Unit Only", "Unit + Integration", "Full Testing Pyramid"];

    /// Get enum values by name
    pub fn get_enum_values(enum_type: &str) -> Option<&'static [&'static str]> {
        match enum_type {
            "STORY_STATUS" => Some(STORY_STATUS),
            "GATE_DECISION" => Some(GATE_DECISION),
            "SEVERITY" => Some(SEVERITY),
            "WORKFLOW_MODE" => Some(WORKFLOW_MODE),
            "SECTION_TYPE" => Some(SECTION_TYPE),
            "MERMAID_TYPE" => Some(MERMAID_TYPE),
            "CODE_LANGUAGE" => Some(CODE_LANGUAGE),
            "OUTPUT_FORMAT" => Some(OUTPUT_FORMAT),
            "ACCESSIBILITY" => Some(ACCESSIBILITY),
            "TARGET_PLATFORM" => Some(TARGET_PLATFORM),
            "REPOSITORY_STRUCTURE" => Some(REPOSITORY_STRUCTURE),
            "SERVICE_ARCHITECTURE" => Some(SERVICE_ARCHITECTURE),
            "TESTING_STRATEGY" => Some(TESTING_STRATEGY),
            "WORKFLOW_TYPE" => Some(WORKFLOW_TYPE),
            _ => None,
        }
    }
}

/// Known boolean variables from BMAD templates
pub const BOOL_VARIABLES: &[&str] = &[
    "elicit",
    "repeatable",
    "optional",
    "modified",
    "markdownExploder",
    "prdSharded",
    "architectureSharded",
    "active",
    "multiSelect",
    "required",
];

/// Known number variables from BMAD templates
pub const NUMBER_VARIABLES: &[&str] = &[
    "epic_num",
    "epic_number",
    "story_num",
    "story_number",
    "priority_level",
    "segment_number",
    "opportunity_number",
    "criterion_number",
    "total_ideas",
    "quality_score",
    "tests_reviewed",
    "risks_identified",
    "critical",
    "high",
    "medium",
    "low",
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
    "agents",
    "workflows",
    "project_types",
    "optional_steps",
    "requires",
    "columns",
    "items",
    "editable_sections",
    "editors",
    "ides_setup",
    "expansion_packs",
    "devLoadAlwaysFiles",
    "ac_covered",
    "ac_gaps",
    "must_fix",
    "monitor",
    "choices",
];

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
        let values = common_enums::get_enum_values(enum_type).unwrap_or(&[]);
        return ParsedVariable {
            name: name.to_string(),
            var_type: VariableType::Enum,
            enum_values: Some(values.iter().map(|s| s.to_string()).collect()),
            default_value: values
                .first()
                .map(|v| serde_json::Value::String(v.to_string())),
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
    names
        .into_iter()
        .map(|name| infer_variable_type(&name))
        .collect()
}

/// YAML frontmatter with variable type hints
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Frontmatter {
    #[serde(default)]
    pub variables: HashMap<String, VariableDefinition>,
}

/// Variable definition from YAML frontmatter
#[derive(Debug, Clone, Deserialize, Serialize)]
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
///
/// Returns the parsed frontmatter and the remaining content after the frontmatter.
/// Returns None if no valid frontmatter is found.
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
///
/// Frontmatter definitions take precedence over inferred types.
pub fn merge_with_frontmatter(
    inferred: Vec<ParsedVariable>,
    frontmatter: &Frontmatter,
) -> Vec<ParsedVariable> {
    inferred
        .into_iter()
        .map(|mut var| {
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
        })
        .collect()
}

/// Extract variable names from markdown content using {{name}} pattern
pub fn extract_variable_names(content: &str) -> Vec<String> {
    let mut variables = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second '{'
            let mut name = String::new();
            let mut valid = true;

            while let Some(&next_char) = chars.peek() {
                if next_char == '}' {
                    chars.next();
                    if chars.peek() == Some(&'}') {
                        chars.next();
                        break;
                    } else {
                        valid = false;
                        break;
                    }
                } else if next_char.is_alphanumeric() || next_char == '_' {
                    name.push(next_char);
                    chars.next();
                } else {
                    valid = false;
                    break;
                }
            }

            if valid && !name.is_empty() && !seen.contains(&name) {
                seen.insert(name.clone());
                variables.push(name);
            }
        }
    }

    variables
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_type_as_str() {
        assert_eq!(VariableType::Bool.as_str(), "bool");
        assert_eq!(VariableType::Enum.as_str(), "enum");
        assert_eq!(VariableType::Number.as_str(), "number");
        assert_eq!(VariableType::String.as_str(), "string");
        assert_eq!(VariableType::StringArray.as_str(), "string[]");
        assert_eq!(VariableType::Object.as_str(), "object");
    }

    #[test]
    fn test_variable_type_display() {
        assert_eq!(format!("{}", VariableType::Bool), "bool");
        assert_eq!(format!("{}", VariableType::StringArray), "string[]");
    }

    #[test]
    fn test_extract_variables() {
        let content = "# {{title}}\n\n{{description}}";
        let names = extract_variable_names(content);

        assert_eq!(names.len(), 2);
        assert!(names.contains(&"title".to_string()));
        assert!(names.contains(&"description".to_string()));

        let typed = variables_to_typed(names);
        assert_eq!(typed.len(), 2);
        // Both should be string type (unknown variables default to string)
        assert!(typed.iter().all(|v| v.var_type == VariableType::String));
    }

    #[test]
    fn test_extract_variables_no_duplicates() {
        let content = "{{name}} and {{name}} again";
        let names = extract_variable_names(content);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "name");
    }

    #[test]
    fn test_extract_variables_with_underscores() {
        let content = "{{my_variable}} and {{another_one}}";
        let names = extract_variable_names(content);
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"my_variable".to_string()));
        assert!(names.contains(&"another_one".to_string()));
    }

    #[test]
    fn test_infer_bool_variable() {
        let var = infer_variable_type("elicit");
        assert_eq!(var.var_type, VariableType::Bool);
        assert_eq!(var.default_value, Some(serde_json::Value::Bool(false)));

        let var2 = infer_variable_type("repeatable");
        assert_eq!(var2.var_type, VariableType::Bool);

        let var3 = infer_variable_type("required");
        assert_eq!(var3.var_type, VariableType::Bool);
    }

    #[test]
    fn test_infer_enum_variable() {
        let var = infer_variable_type("status");
        assert_eq!(var.var_type, VariableType::Enum);
        assert_eq!(
            var.enum_values,
            Some(
                vec!["Draft", "Approved", "InProgress", "Review", "Done"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            )
        );

        let var2 = infer_variable_type("gate");
        assert_eq!(var2.var_type, VariableType::Enum);
        assert!(var2
            .enum_values
            .as_ref()
            .unwrap()
            .contains(&"PASS".to_string()));
    }

    #[test]
    fn test_infer_number_variable() {
        let var = infer_variable_type("epic_num");
        assert_eq!(var.var_type, VariableType::Number);

        let var2 = infer_variable_type("story_num");
        assert_eq!(var2.var_type, VariableType::Number);

        let var3 = infer_variable_type("quality_score");
        assert_eq!(var3.var_type, VariableType::Number);
    }

    #[test]
    fn test_infer_string_array_variable() {
        let var = infer_variable_type("agents");
        assert_eq!(var.var_type, VariableType::StringArray);
        assert_eq!(var.default_value, Some(serde_json::Value::Array(vec![])));

        let var2 = infer_variable_type("workflows");
        assert_eq!(var2.var_type, VariableType::StringArray);
    }

    #[test]
    fn test_infer_unknown_defaults_to_string() {
        let var = infer_variable_type("my_custom_var");
        assert_eq!(var.var_type, VariableType::String);
        assert_eq!(
            var.default_value,
            Some(serde_json::Value::String(String::new()))
        );
    }

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

        let var_def = fm.variables.get("custom_status").unwrap();
        assert_eq!(var_def.var_type, "enum");
        assert!(var_def.required);
        assert_eq!(
            var_def.enum_values,
            Some(vec![
                "Open".to_string(),
                "Closed".to_string(),
                "Pending".to_string()
            ])
        );
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "# Just a document\n\nNo frontmatter here.";
        let result = extract_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_frontmatter_incomplete() {
        let content = "---\nvariables: {}\n# Missing closing ---";
        let result = extract_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_merge_frontmatter() {
        let inferred = vec![
            infer_variable_type("custom_field"), // defaults to string
        ];

        let mut fm = Frontmatter {
            variables: HashMap::new(),
        };
        fm.variables.insert(
            "custom_field".to_string(),
            VariableDefinition {
                var_type: "number".to_string(),
                enum_values: None,
                default: Some(serde_json::json!(42)),
                required: true,
                description: Some("A custom number".to_string()),
            },
        );

        let merged = merge_with_frontmatter(inferred, &fm);
        assert_eq!(merged[0].var_type, VariableType::Number);
        assert_eq!(merged[0].default_value, Some(serde_json::json!(42)));
        assert!(merged[0].required);
        assert_eq!(merged[0].description, Some("A custom number".to_string()));
    }

    #[test]
    fn test_merge_frontmatter_preserves_inferred_when_not_overridden() {
        let inferred = vec![
            infer_variable_type("elicit"),  // known bool
            infer_variable_type("unknown"), // unknown string
        ];

        let fm = Frontmatter {
            variables: HashMap::new(),
        };

        let merged = merge_with_frontmatter(inferred, &fm);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].var_type, VariableType::Bool);
        assert_eq!(merged[1].var_type, VariableType::String);
    }

    #[test]
    fn test_variables_to_typed() {
        let names = vec![
            "elicit".to_string(),
            "status".to_string(),
            "epic_num".to_string(),
            "agents".to_string(),
            "custom".to_string(),
        ];

        let typed = variables_to_typed(names);

        assert_eq!(typed.len(), 5);
        assert_eq!(typed[0].var_type, VariableType::Bool);
        assert_eq!(typed[1].var_type, VariableType::Enum);
        assert_eq!(typed[2].var_type, VariableType::Number);
        assert_eq!(typed[3].var_type, VariableType::StringArray);
        assert_eq!(typed[4].var_type, VariableType::String);
    }

    #[test]
    fn test_common_enums_get_enum_values() {
        assert!(common_enums::get_enum_values("STORY_STATUS").is_some());
        assert!(common_enums::get_enum_values("GATE_DECISION").is_some());
        assert!(common_enums::get_enum_values("UNKNOWN").is_none());
    }

    #[test]
    fn test_parsed_variable_builder_pattern() {
        let var = ParsedVariable::new("test_var", VariableType::Enum)
            .with_enum_values(vec!["A".to_string(), "B".to_string()])
            .with_default(serde_json::json!("A"))
            .with_required(true)
            .with_description("Test description");

        assert_eq!(var.name, "test_var");
        assert_eq!(var.var_type, VariableType::Enum);
        assert_eq!(
            var.enum_values,
            Some(vec!["A".to_string(), "B".to_string()])
        );
        assert_eq!(var.default_value, Some(serde_json::json!("A")));
        assert!(var.required);
        assert_eq!(var.description, Some("Test description".to_string()));
    }

    #[test]
    fn test_frontmatter_with_default_value() {
        let content = r#"---
variables:
  count:
    type: number
    default: 10
    description: The count value
---
Content"#;

        let (fm, _) = extract_frontmatter(content).unwrap();
        let var_def = fm.variables.get("count").unwrap();
        assert_eq!(var_def.var_type, "number");
        assert_eq!(var_def.default, Some(serde_json::json!(10)));
        assert_eq!(var_def.description, Some("The count value".to_string()));
    }

    #[test]
    fn test_serde_roundtrip_variable_type() {
        let original = VariableType::StringArray;
        let json = serde_json::to_string(&original).unwrap();
        let parsed: VariableType = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_serde_roundtrip_parsed_variable() {
        let original = ParsedVariable {
            name: "test".to_string(),
            var_type: VariableType::Enum,
            enum_values: Some(vec!["A".to_string(), "B".to_string()]),
            default_value: Some(serde_json::json!("A")),
            required: true,
            description: Some("A test variable".to_string()),
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: ParsedVariable = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
}
