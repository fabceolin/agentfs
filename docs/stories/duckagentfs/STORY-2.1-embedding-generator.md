# STORY-2.1: Embedding Generator Trait

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-2.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 2 - Vector Similarity Search |
| **Status** | Done |
| **Priority** | High |
| **File** | `sdk/rust/src/embedding.rs` |

## User Story

**As a** developer
**I want** a trait for embedding generation
**So that** I can support different providers (OpenAI, local, etc)

## Technical Description

Embeddings are vector representations of text that capture semantic meaning. Files with similar meanings will have embeddings close together in vector space, enabling semantic search.

## Acceptance Criteria

- [x] Trait `EmbeddingGenerator` with async methods
- [x] NoOp implementation for tests
- [x] OpenAI implementation (conceptual)
- [x] Local/ONNX implementation (conceptual)
- [x] Chunked embedding strategy for large files

## Technical Specification

### Main Trait

```rust
#[async_trait]
pub trait EmbeddingGenerator: Send + Sync {
    /// Generate embedding from text
    async fn generate(&self, content: &str) -> Result<Vec<f32>>;

    /// Batch generation (default: sequential)
    async fn generate_batch(&self, contents: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(contents.len());
        for content in contents {
            results.push(self.generate(content).await?);
        }
        Ok(results)
    }

    /// Model name for tracking
    fn model_name(&self) -> &str;

    /// Embedding dimension
    fn dimension(&self) -> usize;

    /// Should this content be embedded?
    fn should_embed(&self, content: &str) -> bool {
        content.len() >= 10
    }

    /// Maximum content length
    fn max_content_length(&self) -> usize {
        8191
    }
}
```

### Implementations

#### NoOp (for tests)

```rust
pub struct NoOpEmbeddingGenerator;

#[async_trait]
impl EmbeddingGenerator for NoOpEmbeddingGenerator {
    async fn generate(&self, _: &str) -> Result<Vec<f32>> {
        Ok(vec![])
    }

    fn model_name(&self) -> &str { "none" }
    fn dimension(&self) -> usize { 0 }
    fn should_embed(&self, _: &str) -> bool { false }
}
```

#### OpenAI

```rust
pub struct OpenAIEmbedding {
    api_key: String,
    model: String,
    dimension: usize,
    client: reqwest::Client,
}

impl OpenAIEmbedding {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "text-embedding-ada-002".to_string(),
            dimension: 1536,
            client: reqwest::Client::new(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let model = model.into();
        self.dimension = match model.as_str() {
            "text-embedding-ada-002" => 1536,
            "text-embedding-3-small" => 1536,
            "text-embedding-3-large" => 3072,
            _ => 1536,
        };
        self.model = model;
        self
    }
}

#[async_trait]
impl EmbeddingGenerator for OpenAIEmbedding {
    async fn generate(&self, content: &str) -> Result<Vec<f32>> {
        let response = self.client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "input": content,
                "model": self.model
            }))
            .send()
            .await?;

        let data: EmbeddingResponse = response.json().await?;
        Ok(data.data[0].embedding.clone())
    }

    async fn generate_batch(&self, contents: &[&str]) -> Result<Vec<Vec<f32>>> {
        // OpenAI supports batch in single request
        let response = self.client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "input": contents,
                "model": self.model
            }))
            .send()
            .await?;

        let data: EmbeddingResponse = response.json().await?;
        Ok(data.data.into_iter().map(|d| d.embedding).collect())
    }

    fn model_name(&self) -> &str { &self.model }
    fn dimension(&self) -> usize { self.dimension }
}
```

#### Local ONNX

```rust
pub struct LocalEmbedding {
    session: ort::Session,
    tokenizer: tokenizers::Tokenizer,
    model_name: String,
    dimension: usize,
}

impl LocalEmbedding {
    pub fn new(model_path: &str) -> Result<Self> {
        let session = ort::Session::builder()?
            .with_model_from_file(model_path)?;

        // Detect dimension from output shape
        let dimension = session.outputs()[0].dimensions()[1]
            .unwrap_or(384) as usize;

        Ok(Self {
            session,
            tokenizer: load_tokenizer(model_path)?,
            model_name: "local-onnx".to_string(),
            dimension,
        })
    }
}

#[async_trait]
impl EmbeddingGenerator for LocalEmbedding {
    async fn generate(&self, content: &str) -> Result<Vec<f32>> {
        // Tokenize
        let encoding = self.tokenizer.encode(content, true)?;

        // Run inference
        let outputs = self.session.run(vec![
            ort::Value::from_array(encoding.get_ids())?,
            ort::Value::from_array(encoding.get_attention_mask())?,
        ])?;

        // Extract embedding
        let embedding = outputs[0].try_extract::<f32>()?;
        Ok(embedding.view().iter().copied().collect())
    }

    fn model_name(&self) -> &str { &self.model_name }
    fn dimension(&self) -> usize { self.dimension }
    fn max_content_length(&self) -> usize { 512 }
}
```

### Chunked Embedding

For large files that exceed `max_content_length`:

```rust
pub struct ChunkedEmbedding<G: EmbeddingGenerator> {
    generator: G,
    chunk_size: usize,
    overlap: usize,
}

impl<G: EmbeddingGenerator> ChunkedEmbedding<G> {
    pub fn new(generator: G, chunk_size: usize, overlap: usize) -> Self {
        Self { generator, chunk_size, overlap }
    }

    /// Split into overlapping chunks
    pub fn split_chunks(&self, content: &str) -> Vec<String> {
        let chars: Vec<char> = content.chars().collect();
        let mut chunks = Vec::new();
        let mut start = 0;

        while start < chars.len() {
            let end = (start + self.chunk_size).min(chars.len());
            chunks.push(chars[start..end].iter().collect());
            if end >= chars.len() { break; }
            start = end.saturating_sub(self.overlap);
        }

        chunks
    }

    /// Generate per-chunk embeddings
    pub async fn generate_chunks(&self, content: &str)
        -> Result<Vec<(usize, usize, Vec<f32>)>>
    {
        let chunks = self.split_chunks(content);
        let mut results = Vec::new();
        let mut offset = 0;

        for chunk in &chunks {
            let embedding = self.generator.generate(chunk).await?;
            results.push((offset, offset + chunk.len(), embedding));
            offset += chunk.len() - self.overlap;
        }

        Ok(results)
    }

    /// Average all chunks into single embedding
    pub async fn generate_aggregated(&self, content: &str) -> Result<Vec<f32>> {
        if content.len() <= self.generator.max_content_length() {
            return self.generator.generate(content).await;
        }

        let chunks = self.generate_chunks(content).await?;
        let dim = self.generator.dimension();
        let mut avg = vec![0.0; dim];

        for (_, _, emb) in &chunks {
            for (i, v) in emb.iter().enumerate() {
                avg[i] += v / chunks.len() as f32;
            }
        }

        Ok(avg)
    }
}
```

## Integration with DuckAgentFS

```rust
impl DuckAgentFS {
    /// Update embedding when file is written
    async fn update_embedding(&self, ino: i64, content: &[u8]) -> Result<()> {
        if !self.config.enable_vss {
            return Ok(());
        }

        // Only embed text files
        let text = match std::str::from_utf8(content) {
            Ok(t) => t,
            Err(_) => return Ok(()), // Skip binary
        };

        if !self.embedding_generator.should_embed(text) {
            return Ok(());
        }

        let embedding = self.embedding_generator.generate(text).await?;
        let content_hash = md5::compute(text);

        let conn = self.pool.get_write_connection().await?;
        conn.execute(
            r#"
            INSERT OR REPLACE INTO fs_embeddings
            (inode, embedding, model, content_hash, generated_at)
            VALUES (?, ?, ?, ?, current_timestamp)
            "#,
            params![
                ino,
                embedding,
                self.embedding_generator.model_name(),
                format!("{:x}", content_hash)
            ]
        )?;

        Ok(())
    }
}
```

## Tests

### Test 1: NoOp Generator
```rust
#[tokio::test]
async fn test_noop() {
    let gen = NoOpEmbeddingGenerator;
    let emb = gen.generate("test").await.unwrap();
    assert!(emb.is_empty());
    assert!(!gen.should_embed("test"));
}
```

### Test 2: Chunking
```rust
#[test]
fn test_chunking() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 10, 2);

    let chunks = chunked.split_chunks("hello world this is a test");
    assert!(chunks.len() > 1);
    assert!(chunks.iter().all(|c| c.len() <= 10));
}
```

### Test 3: OpenAI Dimensions
```rust
#[test]
fn test_openai_dimensions() {
    let gen = OpenAIEmbedding::new("test");
    assert_eq!(gen.dimension(), 1536);

    let gen = gen.with_model("text-embedding-3-large");
    assert_eq!(gen.dimension(), 3072);
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/embedding.rs` | Trait and implementations |
| `sdk/rust/src/filesystem/duckagentfs.rs` | Consumer |
| `schema/duckagentfs.sql` | fs_embeddings table |

## Implementation Notes

1. **Rate Limiting**: OpenAI has rate limits. Implement backoff/retry.

2. **Caching**: Cache embeddings to avoid re-generating unchanged content.

3. **Async**: Generate embeddings in background to not block writes.

4. **Recommended Local Models**:
   - `all-MiniLM-L6-v2` (384 dims, fast)
   - `bge-small-en-v1.5` (384 dims, high quality)
   - `e5-small-v2` (384 dims, good for search)
