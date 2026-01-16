# STORY-2.1.4: Agent-Based Transformation

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.1.4 |
| **Parent** | STORY-2.1 |
| **Epic** | EPIC-GRAPHDOCS-001 |
| **Phase** | 2 - Parsing and Population |
| **Status** | Ready for Development |
| **Priority** | Medium |
| **Files** | `sdk/rust/src/graphdocs/agent_transformer.rs`, `agents/*.yaml` |
| **Dependencies** | STORY-2.1.3, TEA Agent (external) |

## User Story

**As a** developer
**I want** non-conforming documents to be automatically transformed using AI agents
**So that** I can fix documentation inconsistencies at scale

## Acceptance Criteria

- [ ] Use local YAML agents with GGUF model (gemma3:e4b) to transform non-conforming documents
- [ ] Call TEA binary as subprocess (not embedded)
- [ ] Support dry-run mode to preview changes
- [ ] Provide CLI interface for batch transformation

## Architecture Decision

**Decision**: Use TEA binary as subprocess (not embedded library)

**Rationale**:
- Minimal changes to agentfs codebase
- Unix philosophy - composable tools
- GGUF models are large; no need to embed llama-cpp in agentfs
- TEA updates are independent of agentfs releases
- Simpler build process (no CUDA/Vulkan feature flags in agentfs)

## Technical Specification

### TEA Agent Definitions

#### Document Conformance Agent

```yaml
# agents/document-conformance-agent.yaml
# Prerequisite: TEA with llm-local feature
# BUILD: cd /path/to/tea && cargo build --release --features llm-local
# RUN: tea run document-conformance-agent.yaml --input '{"raw_status": "[**Done**]"}'

name: document-conformance-fixer
description: Normalizes status text using embeddings + local LLM fallback

state_schema:
  raw_status: str
  normalized_status: str
  match_score: float
  needs_llm: bool

settings:
  llm:
    backend: local
    model_path: ~/.cache/tea/models/gemma-3n-E4B-it-Q4_K_M.gguf
    n_ctx: 2048
    n_gpu_layers: 0  # CPU only, use -1 for GPU

config:
  raise_exceptions: true

nodes:
  # Step 1: Generate embeddings for known statuses
  - name: init_embeddings
    uses: memory.embed
    with:
      texts:
        - "Done"
        - "Complete"
        - "Finished"
        - "In Progress"
        - "Work in progress"
        - "Draft"
        - "Not started"
        - "Review"
        - "Dev Complete"
        - "Approved"
      model: model2vec
    output: status_embeddings

  # Step 2: Embed the raw status from document
  - name: embed_raw_status
    uses: memory.embed
    with:
      texts:
        - "{{ state.raw_status }}"
      model: model2vec
    output: raw_embedding

  # Step 3: Find closest matching status via vector search
  - name: match_status
    uses: memory.vector_search
    with:
      query_embedding: "{{ state.raw_embedding[0] }}"
      embeddings: "{{ state.status_embeddings }}"
      top_k: 1
      threshold: 0.6
    output: matched_status

  # Step 4: Check if embedding match is good enough
  - name: check_match
    language: lua
    run: |
      local matched = state.matched_status or {}
      local score = matched.score or 0
      if score > 0.6 then
        return { normalized_status = matched.text, needs_llm = false, match_score = score }
      end
      return { needs_llm = true, match_score = score }
    goto:
      - if: "state.needs_llm"
        to: llm_classify
      - to: format_output

  # Step 5: Use local LLM for classification (fallback)
  - name: llm_classify
    uses: llm.chat
    with:
      backend: local
      system: |
        You are a status classifier. Classify the following status text into one of:
        Draft, Approved, InProgress, Review, Done
        Return ONLY the status word, nothing else.
      prompt: "Status: {{ state.raw_status }}"
      max_tokens: 16
      temperature: 0.1
    output: llm_response

  # Step 6: Parse LLM response
  - name: parse_llm_response
    language: lua
    run: |
      local resp = state.llm_response or {}
      local content = resp.content or "Draft"
      -- Extract first word
      local status = content:match("^%s*(%w+)") or "Draft"
      -- Validate against known statuses
      local valid = { Draft=true, Approved=true, InProgress=true, Review=true, Done=true }
      if not valid[status] then
        status = "Draft"
      end
      return { normalized_status = status }

  # Step 7: Map to canonical status
  - name: format_output
    language: lua
    run: |
      local status_map = {
        Done = "Done", Complete = "Done", Finished = "Done",
        ["In Progress"] = "InProgress", ["Work in progress"] = "InProgress",
        Draft = "Draft", ["Not started"] = "Draft",
        Review = "Review", ["Dev Complete"] = "Review",
        Approved = "Approved"
      }
      local raw = state.normalized_status or "Draft"
      local normalized = status_map[raw] or raw
      return { normalized_status = normalized }
```

#### Document Transformer Agent

```yaml
# agents/document-transformer-agent.yaml
# RUN: tea run document-transformer-agent.yaml --input '{"document": {...}, "template": {...}}'

name: document-transformer
description: Transforms documents to match template structure using local LLM

state_schema:
  document: object
  template: object
  conformance: object
  output_content: str
  needs_transform: bool

settings:
  llm:
    backend: local
    model_path: ~/.cache/tea/models/gemma-3n-E4B-it-Q4_K_M.gguf
    n_ctx: 4096  # Larger context for documents
    n_gpu_layers: 0

config:
  raise_exceptions: true

nodes:
  - name: analyze_gaps
    language: lua
    run: |
      local conformance = state.conformance or {}
      local missing = conformance.missing_sections or {}
      local type_issues = conformance.type_mismatches or {}
      local needs_transform = #missing > 0 or #type_issues > 0
      return {
        gaps = {
          missing_sections = missing,
          type_mismatches = type_issues
        },
        needs_transform = needs_transform
      }
    goto:
      - if: "state.needs_transform"
        to: generate_transform_prompt
      - to: passthrough

  - name: generate_transform_prompt
    language: lua
    run: |
      local doc = state.document or {}
      local template = state.template or {}
      local gaps = state.gaps or {}

      local template_sections = {}
      for _, s in ipairs(template.sections or {}) do
        if s.section_type == "heading" then
          table.insert(template_sections, s.content)
        end
      end

      local doc_content = {}
      for _, s in ipairs(doc.sections or {}) do
        table.insert(doc_content, s.content)
      end

      local prompt = string.format([[Transform this document to match the template structure.

Current Document Title: %s

Missing Sections: %s

Template Sections:
%s

Current Document Content:
%s

Output the transformed document in markdown format.
Preserve existing content, add placeholders for missing sections.]],
        doc.title or "",
        table.concat(gaps.missing_sections or {}, ", "),
        table.concat(template_sections, "\n"),
        table.concat(doc_content, "\n\n")
      )
      return { transform_prompt = prompt }

  - name: transform_with_llm
    uses: llm.chat
    with:
      backend: local
      system: You are a document structure expert. Transform documents to match templates while preserving content.
      prompt: "{{ state.transform_prompt }}"
      max_tokens: 2048
      temperature: 0.2
    output: transform_result

  - name: extract_transformed
    language: lua
    run: |
      local resp = state.transform_result or {}
      local content = resp.content or ""
      return { output_content = content }

  - name: passthrough
    language: lua
    run: |
      local doc = state.document or {}
      local content_parts = {}
      for _, s in ipairs(doc.sections or {}) do
        table.insert(content_parts, s.content or "")
      end
      return { output_content = table.concat(content_parts, "\n\n") }
```

### Rust Implementation (Subprocess)

```rust
// sdk/rust/src/graphdocs/agent_transformer.rs

use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use anyhow::{Result, anyhow, Context};
use serde_json::{json, Value};

use super::parser::ParsedDocument;
use super::conformance::ConformanceResult;
use super::normalizer::ExtendedStatus;

/// Agent transformer using TEA subprocess
pub struct AgentTransformer {
    tea_binary: String,
    agents_dir: PathBuf,
    model_path: Option<PathBuf>,
}

impl AgentTransformer {
    /// Create new transformer
    pub fn new(agents_dir: PathBuf) -> Self {
        Self {
            tea_binary: std::env::var("TEA_BINARY").unwrap_or_else(|_| "tea".to_string()),
            agents_dir,
            model_path: None,
        }
    }

    /// Set custom model path
    pub fn with_model_path(mut self, path: PathBuf) -> Self {
        self.model_path = Some(path);
        self
    }

    /// Check if TEA is available
    pub async fn check_tea_available(&self) -> Result<bool> {
        let output = Command::new(&self.tea_binary)
            .arg("--version")
            .output()
            .await;

        match output {
            Ok(o) => Ok(o.status.success()),
            Err(_) => Ok(false),
        }
    }

    /// Run TEA agent with input
    async fn run_agent(&self, agent_file: &str, input: Value) -> Result<Value> {
        let agent_path = self.agents_dir.join(agent_file);

        if !agent_path.exists() {
            return Err(anyhow!("Agent file not found: {}", agent_path.display()));
        }

        let mut cmd = Command::new(&self.tea_binary);
        cmd.arg("run")
           .arg(&agent_path)
           .arg("--input")
           .arg(input.to_string())
           .stdout(Stdio::piped())
           .stderr(Stdio::piped());

        // Set model path env var if specified
        if let Some(ref model_path) = self.model_path {
            cmd.env("GGUF_MODEL_PATH", model_path.display().to_string());
        }

        let output = cmd.output().await
            .context("Failed to execute TEA")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("TEA agent failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&stdout)
            .context("Failed to parse TEA output as JSON")
    }

    /// Normalize status using TEA agent
    pub async fn normalize_status(&self, raw_status: &str) -> Result<ExtendedStatus> {
        let input = json!({
            "raw_status": raw_status
        });

        let result = self.run_agent("document-conformance-agent.yaml", input).await?;

        let status_str = result
            .get("normalized_status")
            .and_then(|v| v.as_str())
            .unwrap_or("Draft");

        Ok(match status_str {
            "Draft" => ExtendedStatus::Draft,
            "Approved" => ExtendedStatus::Approved,
            "InProgress" => ExtendedStatus::InProgress,
            "Review" => ExtendedStatus::Review,
            "Done" => ExtendedStatus::Done,
            _ => ExtendedStatus::Draft,
        })
    }

    /// Transform document to conform to template
    pub async fn transform_to_template(
        &self,
        doc: &ParsedDocument,
        template: &ParsedDocument,
        conformance: &ConformanceResult,
    ) -> Result<String> {
        let input = json!({
            "document": doc,
            "template": template,
            "conformance": {
                "missing_sections": conformance.missing_sections,
                "type_mismatches": conformance.type_mismatches,
            }
        });

        let result = self.run_agent("document-transformer-agent.yaml", input).await?;

        let content = result
            .get("output_content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            // Fallback to rule-based if LLM returns empty
            return self.transform_rule_based(doc, template, conformance);
        }

        Ok(content)
    }

    /// Rule-based transformation fallback
    fn transform_rule_based(
        &self,
        doc: &ParsedDocument,
        template: &ParsedDocument,
        _conformance: &ConformanceResult,
    ) -> Result<String> {
        use std::collections::HashMap;
        use super::parser::SectionType;

        let mut output = String::new();

        // Add title if present
        if let Some(title) = &doc.title {
            output.push_str(&format!("# {}\n\n", title));
        }

        // Build map of document sections by name
        let doc_sections: HashMap<String, _> = doc.sections
            .iter()
            .filter(|s| s.section_type == SectionType::Heading)
            .map(|s| (s.content.to_lowercase(), s))
            .collect();

        // Follow template structure
        for template_section in &template.sections {
            if template_section.section_type != SectionType::Heading {
                continue;
            }

            let section_name = template_section.content.to_lowercase();
            let level = template_section.level.unwrap_or(2);
            let prefix = "#".repeat(level as usize);

            if let Some(doc_section) = doc_sections.get(&section_name) {
                // Use existing content
                output.push_str(&format!("{} {}\n\n", prefix, doc_section.content));
            } else {
                // Add placeholder for missing section
                output.push_str(&format!("{} {}\n\n<!-- TODO: Add content -->\n\n",
                    prefix, template_section.content));
            }
        }

        Ok(output)
    }
}

/// CLI arguments for conform command
#[derive(Debug, Clone)]
pub struct ConformArgs {
    pub dir: PathBuf,
    pub model_path: Option<PathBuf>,
    pub agents_dir: Option<PathBuf>,
    pub dry_run: bool,
}

/// Result of transformation
#[derive(Debug)]
pub struct TransformResult {
    pub file_path: String,
    pub original_issues: usize,
    pub transformed: bool,
    pub dry_run: bool,
    pub new_content: Option<String>,
}

/// Batch transform all non-conforming documents
pub async fn batch_transform(args: &ConformArgs) -> Result<Vec<TransformResult>> {
    use super::conformance::{scan_directory, TemplateManager};
    use super::parser::MarkdownParser;

    let agents_dir = args.agents_dir.clone()
        .unwrap_or_else(|| PathBuf::from("agents"));

    let mut transformer = AgentTransformer::new(agents_dir);
    if let Some(ref model_path) = args.model_path {
        transformer = transformer.with_model_path(model_path.clone());
    }

    // Check TEA availability
    if !transformer.check_tea_available().await? {
        return Err(anyhow!(
            "TEA binary not found. Install with: cargo install --path /path/to/tea --features llm-local"
        ));
    }

    let conformance_results = scan_directory(&args.dir).await?;
    let mut results = Vec::new();

    // Load template
    let template_path = TemplateManager::detect_template(&args.dir)
        .ok_or_else(|| anyhow!("No template found in directory"))?;

    let template_content = tokio::fs::read_to_string(&template_path).await?;
    let template = MarkdownParser::new().parse(&template_content)?;

    for conformance in conformance_results {
        if !conformance.is_conformant {
            let doc_content = tokio::fs::read_to_string(&conformance.file_path).await?;
            let doc = MarkdownParser::new().parse(&doc_content)?;

            let transformed = transformer.transform_to_template(
                &doc,
                &template,
                &conformance,
            ).await?;

            if !args.dry_run {
                tokio::fs::write(&conformance.file_path, &transformed).await?;
            }

            results.push(TransformResult {
                file_path: conformance.file_path,
                original_issues: conformance.suggestions.len(),
                transformed: true,
                dry_run: args.dry_run,
                new_content: if args.dry_run { Some(transformed) } else { None },
            });
        }
    }

    Ok(results)
}
```

### CLI Interface

```rust
// cli/src/cmd/graphdocs.rs

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
pub struct GraphDocsCmd {
    #[command(subcommand)]
    command: GraphDocsSubcommand,
}

#[derive(Subcommand)]
enum GraphDocsSubcommand {
    /// Check and fix document conformance
    Conform {
        /// Directory to scan
        dir: PathBuf,

        /// Path to GGUF model (default: ~/.cache/tea/models/gemma-3n-E4B-it-Q4_K_M.gguf)
        #[arg(long)]
        model_path: Option<PathBuf>,

        /// Directory containing agent YAML files (default: ./agents)
        #[arg(long)]
        agents_dir: Option<PathBuf>,

        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },
}

impl GraphDocsCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            GraphDocsSubcommand::Conform { dir, model_path, agents_dir, dry_run } => {
                let args = graphdocs::agent_transformer::ConformArgs {
                    dir,
                    model_path,
                    agents_dir,
                    dry_run,
                };

                let results = graphdocs::agent_transformer::batch_transform(&args).await?;

                for result in &results {
                    if result.dry_run {
                        println!("[DRY-RUN] Would transform: {} ({} issues)",
                            result.file_path, result.original_issues);
                    } else {
                        println!("Transformed: {} ({} issues fixed)",
                            result.file_path, result.original_issues);
                    }
                }

                println!("\nTotal: {} files processed", results.len());
                Ok(())
            }
        }
    }
}
```

### Usage

```bash
# Install TEA with local LLM support
cd /path/to/the_edge_agent/rust
cargo install --path . --features llm-local

# Download GGUF model
mkdir -p ~/.cache/tea/models
wget -O ~/.cache/tea/models/gemma-3n-E4B-it-Q4_K_M.gguf \
  https://huggingface.co/google/gemma-3n-E4B-it-GGUF/resolve/main/gemma-3n-E4B-it-Q4_K_M.gguf

# Run conformance check (dry-run)
agentfs graphdocs conform ./docs/stories/ --dry-run

# Run conformance check (apply changes)
agentfs graphdocs conform ./docs/stories/

# Use custom model path
agentfs graphdocs conform ./docs/stories/ --model-path /path/to/custom.gguf

# Use environment variable
GGUF_MODEL_PATH=/path/to/model.gguf agentfs graphdocs conform ./docs/stories/
```

## Building TEA with Local LLM Support (Linux)

```bash
cd /path/to/the_edge_agent/rust

# CPU-only (multi-threaded)
cargo build --release --features llm-local

# With CUDA GPU acceleration (NVIDIA)
cargo build --release --features llm-local-cuda

# With Vulkan GPU acceleration (AMD/Intel/NVIDIA)
cargo build --release --features llm-local-vulkan
```

| Feature | Description |
|---------|-------------|
| `llm-local` | Base local LLM support via llama.cpp (CPU multi-threaded) |
| `llm-local-cuda` | CUDA GPU acceleration (NVIDIA GPUs) |
| `llm-local-vulkan` | Vulkan GPU acceleration (AMD/Intel/NVIDIA) |

## Tests

### Test 1: TEA Availability Check
```rust
#[tokio::test]
async fn test_tea_available() {
    let transformer = AgentTransformer::new(PathBuf::from("agents"));
    // This will pass if TEA is installed, skip otherwise
    let _ = transformer.check_tea_available().await;
}
```

### Test 2: Status Normalization via Agent
```rust
#[tokio::test]
#[ignore] // Requires TEA + model
async fn test_normalize_status_agent() {
    let transformer = AgentTransformer::new(PathBuf::from("agents"));
    let status = transformer.normalize_status("[**Done**]").await.unwrap();
    assert_eq!(status, ExtendedStatus::Done);
}
```

### Test 3: Rule-Based Fallback
```rust
#[test]
fn test_rule_based_transform() {
    let transformer = AgentTransformer::new(PathBuf::from("agents"));
    let template = MarkdownParser::new().parse("# Template\n## Status\n## Description").unwrap();
    let doc = MarkdownParser::new().parse("# My Doc\n## Status\nDone").unwrap();
    let conformance = ConformanceResult {
        file_path: String::new(),
        template_path: None,
        is_conformant: false,
        missing_sections: vec!["description".to_string()],
        extra_sections: vec![],
        type_mismatches: vec![],
        suggestions: vec![],
    };

    let result = transformer.transform_rule_based(&doc, &template, &conformance).unwrap();
    assert!(result.contains("## Description"));
    assert!(result.contains("<!-- TODO: Add content -->"));
}
```

### Test 4: Dry Run Mode
```rust
#[tokio::test]
async fn test_dry_run() {
    let dir = tempdir().unwrap();

    // Create template
    tokio::fs::write(dir.path().join("story-tmpl.md"), "# {{title}}\n## Status\n## Description").await.unwrap();

    // Create non-conforming doc
    tokio::fs::write(dir.path().join("story-1.md"), "# Story 1\n## Status\nDone").await.unwrap();

    let args = ConformArgs {
        dir: dir.path().to_path_buf(),
        model_path: None,
        agents_dir: Some(PathBuf::from("agents")),
        dry_run: true,
    };

    // This would fail without TEA, but demonstrates the API
    // let results = batch_transform(&args).await;
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/graphdocs/agent_transformer.rs` | Subprocess-based agent transformer |
| `agents/document-conformance-agent.yaml` | Status normalization agent |
| `agents/document-transformer-agent.yaml` | Document transformation agent |
| `cli/src/cmd/graphdocs.rs` | CLI interface |

## Dependencies

```toml
[dependencies]
tokio = { version = "1", features = ["fs", "process", "rt-multi-thread"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
clap = { version = "4", features = ["derive"] }

# External dependency (not in Cargo.toml):
# TEA binary must be installed separately
```

## External Dependencies

| Dependency | Installation |
|------------|--------------|
| TEA binary | `cargo install --path /path/to/tea --features llm-local` |
| GGUF model | Download from HuggingFace (gemma-3n-E4B-it-Q4_K_M.gguf) |
