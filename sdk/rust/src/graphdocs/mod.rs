pub mod agent_transformer;
pub mod conformance;
pub mod embedding_matcher;
pub mod engine;
pub mod llm_converter;
pub mod normalizer;
pub mod openai;
pub mod parser;
pub mod relationships;
pub mod renderer;
pub mod template_schema;
pub mod variable_types;

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
    is_template_file, scan_directory, BmadConformanceResult, ChoiceViolation,
    ConformanceSuggestion, MarkdownConformanceResult, MissingSection, SuggestionKind,
    TemplateManager, TypeViolation, TEMPLATE_PATTERNS,
};

pub use agent_transformer::{
    batch_transform, AgentTransformer, ChoiceViolationInfo, ConformArgs, ConformanceResult,
    EnhancedConformanceResult, MissingSectionInfo, SuggestionInfo, TransformResult,
    TypeViolationInfo,
};

pub use relationships::{
    Cardinality, Direction, RelatedDocument, RelationshipDecl, RelationshipEdgeType,
    ResolvedRelationship,
};

pub use renderer::{
    validate_include_path, DocumentContext, RenderConfig, RenderError, TemplateProcessor,
};

pub use engine::{DocumentEvent, GraphDocsEngine, RenderedDocument};
