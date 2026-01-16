pub mod agent_transformer;
pub mod llm_converter;
pub mod openai;
pub mod parser;
pub mod variable_types;
pub mod normalizer;
pub mod embedding_matcher;
pub mod template_schema;
pub mod conformance;

pub use llm_converter::{
    ConvertError, LLMClient, LLMConverter, LLMDocumentSchema, LLMError, LLMRelationship,
    LLMSection, LLMVariable,
};

pub use openai::OpenAIClient;

pub use parser::{
    EdgeType, MarkdownParser, ParseError, ParsedDocument, ParsedEdge, ParsedSection, SectionType,
};

pub use variable_types::{
    common_enums, extract_frontmatter, extract_variable_names, infer_variable_type,
    merge_with_frontmatter, variables_to_typed, Frontmatter, ParsedVariable, VariableDefinition,
    VariableType, BOOL_VARIABLES, ENUM_VARIABLES, NUMBER_VARIABLES, STRING_ARRAY_VARIABLES,
};

pub use normalizer::{
    extract_status, ExtendedStatus, ParsedStatus, ProgressInfo, StatusPattern, STATUS_MAPPINGS,
};

pub use embedding_matcher::{cosine_similarity, StatusEmbeddings};

pub use template_schema::{
    validate_choice_value, AgentConfig, BmadTemplate, OutputConfig, OutputFormat,
    SectionContentType, TemplateMetadata, TemplateSection, WorkflowConfig,
};

pub use conformance::{
    is_template_file, scan_directory, BmadConformanceResult, ChoiceViolation, ConformanceSuggestion,
    MarkdownConformanceResult, MissingSection, SuggestionKind, TemplateManager, TypeViolation,
    TEMPLATE_PATTERNS,
};

pub use agent_transformer::{
    batch_transform, AgentTransformer, ConformArgs, ConformanceResult, TransformResult,
};
