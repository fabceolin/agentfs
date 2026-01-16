# STORY-2.1: Markdown Parser (Parent)

> **NOTE**: This is a parent story. Implementation details are in the sub-stories listed below.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 2 - Parsing and Population |
| **Status** | Ready for Development |
| **Priority** | High |
| **Dependencies** | STORY-1.1, STORY-1.2 |

## User Story

**As a** developer
**I want** a Markdown parser for graphs
**So that** I can convert existing documents to GraphDocs format

## Sub-Stories

This story has been split into 4 sub-stories for better manageability:

| Sub-Story | Description | Acceptance Criteria | Status |
|-----------|-------------|---------------------|--------|
| [STORY-2.1.1](STORY-2.1.1-core-markdown-parser.md) | Core Markdown Parser | AC1-4: Headers, paragraphs, lists, code blocks | Ready |
| [STORY-2.1.2](STORY-2.1.2-variable-detection.md) | Variable Detection & Typing | AC5-7: Variable detection, typing, frontmatter | Ready |
| [STORY-2.1.3](STORY-2.1.3-template-conformance.md) | Template Conformance & Status | AC8-10: Edges, templates, status normalization | Ready |
| [STORY-2.1.4](STORY-2.1.4-agent-transformation.md) | Agent-Based Transformation | AC11: TEA subprocess, GGUF model transformation | Ready |

## Acceptance Criteria (Summary)

### Core Parsing (STORY-2.1.1)
- [ ] AC1: Parse headers (H1-H6) as sections
- [ ] AC2: Parse paragraphs as sections
- [ ] AC3: Parse lists as sections
- [ ] AC4: Parse code blocks as sections

### Variable Detection (STORY-2.1.2)
- [ ] AC5: Detect variables `{{name}}` with typed inference
- [ ] AC6: Support variable types: `bool`, `enum`, `number`, `string`, `string[]`, `object`
- [ ] AC7: Parse YAML frontmatter for type hints and enum definitions

### Template Conformance (STORY-2.1.3)
- [ ] AC8: Generate edge structure
- [ ] AC9: Detect templates in directories and validate conformance
- [ ] AC10: Map variant status markers to standard enum values using embeddings

### Agent Transformation (STORY-2.1.4)
- [ ] AC11: Use local YAML agents (TEA subprocess) with GGUF model to transform non-conforming documents

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           STORY-2.1 Pipeline                            │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌──────────┐ │
│  │  2.1.1      │    │  2.1.2      │    │  2.1.3      │    │  2.1.4   │ │
│  │  Core       │───▶│  Variable   │───▶│  Template   │───▶│  Agent   │ │
│  │  Parser     │    │  Detection  │    │  Conformance│    │  Transform│ │
│  └─────────────┘    └─────────────┘    └─────────────┘    └──────────┘ │
│        │                  │                  │                  │       │
│        ▼                  ▼                  ▼                  ▼       │
│   ParsedSection     ParsedVariable    ConformanceResult   Transformed  │
│   ParsedDocument    VariableType      StatusEmbeddings    Document     │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                          ┌─────────────────┐
                          │  TEA Subprocess │
                          │  (External)     │
                          │  - llm.chat     │
                          │  - memory.embed │
                          └─────────────────┘
```

## Dependency Graph

```
STORY-1.1 (Schema)
    │
    ▼
STORY-1.2 (Connection)
    │
    ▼
STORY-2.1.1 (Core Parser)
    │
    ├──────────────────┐
    ▼                  ▼
STORY-2.1.2        STORY-2.1.3
(Variables)        (Conformance)
    │                  │
    └────────┬─────────┘
             ▼
       STORY-2.1.4
       (Agent Transform)
             │
             ▼
         TEA Binary
         (External)
```

## Related Files

| File | Sub-Story | Description |
|------|-----------|-------------|
| `sdk/rust/src/graphdocs/parser.rs` | 2.1.1 | Core parser implementation |
| `sdk/rust/src/graphdocs/variable_types.rs` | 2.1.2 | Variable type definitions |
| `sdk/rust/src/graphdocs/conformance.rs` | 2.1.3 | Template conformance |
| `sdk/rust/src/graphdocs/normalizer.rs` | 2.1.3 | Status normalization |
| `sdk/rust/src/graphdocs/embedding_matcher.rs` | 2.1.3 | Embedding-based matching |
| `sdk/rust/src/graphdocs/agent_transformer.rs` | 2.1.4 | TEA subprocess integration |
| `agents/document-conformance-agent.yaml` | 2.1.4 | Status normalization agent |
| `agents/document-transformer-agent.yaml` | 2.1.4 | Document transformation agent |

## Dependencies

```toml
[dependencies]
pulldown-cmark = "0.9"
regex = "1"
uuid = { version = "1", features = ["v4"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
glob = "0.3"
tokio = { version = "1", features = ["fs", "process", "rt-multi-thread"] }
anyhow = "1"
thiserror = "1"
```

## External Dependencies

| Dependency | Required By | Installation |
|------------|-------------|--------------|
| TEA binary | STORY-2.1.4 | `cargo install --path /path/to/tea --features llm-local` |
| GGUF model | STORY-2.1.4 | Download gemma-3n-E4B-it-Q4_K_M.gguf from HuggingFace |

## CLI Usage

```bash
# Parse and check conformance (dry-run)
agentfs graphdocs conform ./docs/stories/ --dry-run

# Apply transformations
agentfs graphdocs conform ./docs/stories/

# With custom model
agentfs graphdocs conform ./docs/stories/ --model-path /path/to/model.gguf
```

## Implementation Order

1. **STORY-2.1.1** - Core parser (no external dependencies)
2. **STORY-2.1.2** - Variable detection (depends on 2.1.1)
3. **STORY-2.1.3** - Template conformance (depends on 2.1.1, 2.1.2)
4. **STORY-2.1.4** - Agent transformation (depends on 2.1.3, TEA external)

Stories 2.1.1 and 2.1.2 can be developed in parallel.
