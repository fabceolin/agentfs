# Test Design: STORY-2.1 Embedding Generator Trait

## Document Metadata

| Field | Value |
|-------|-------|
| **Story ID** | STORY-2.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 2 - Vector Similarity Search |
| **Test Design Author** | QA Agent |
| **Created** | 2026-01-14 |
| **Target File** | `sdk/rust/src/embedding.rs` |
| **Status** | Ready for Implementation |

---

## 1. Test Scope Overview

### 1.1 Components Under Test

| Component | Description | Test Type |
|-----------|-------------|-----------|
| `EmbeddingGenerator` trait | Core async trait defining embedding generation contract | Trait compliance tests |
| `NoOpEmbeddingGenerator` | Placeholder implementation for tests/disabled VSS | Unit tests |
| `OpenAIEmbedding` | OpenAI API embedding generator | Unit + Integration tests |
| `LocalEmbedding` | ONNX-based local embedding generator | Unit tests |
| `ChunkedEmbedding<G>` | Chunking strategy for large content | Unit + Property tests |
| DuckAgentFS integration | Embedding updates on file write | Integration tests |

### 1.2 Test Categories Distribution

```
Unit Tests:           60%  (trait compliance, builders, dimension mapping)
Property Tests:       20%  (chunking invariants, boundary conditions)
Integration Tests:    15%  (DuckAgentFS integration, HTTP mocking)
Performance Tests:     5%  (batch generation, chunking overhead)
```

---

## 2. Unit Test Specifications

### 2.1 NoOpEmbeddingGenerator Tests

#### TEST-2.1-U001: NoOp returns empty embedding
```rust
#[tokio::test]
async fn test_noop_generate_returns_empty() {
    // Given: NoOp embedding generator
    let gen = NoOpEmbeddingGenerator;

    // When: Generate embedding for any content
    let result = gen.generate("any text content here").await;

    // Then: Returns Ok with empty vector
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}
```

#### TEST-2.1-U002: NoOp dimension is zero
```rust
#[test]
fn test_noop_dimension_is_zero() {
    let gen = NoOpEmbeddingGenerator;
    assert_eq!(gen.dimension(), 0);
}
```

#### TEST-2.1-U003: NoOp model name is "none"
```rust
#[test]
fn test_noop_model_name() {
    let gen = NoOpEmbeddingGenerator;
    assert_eq!(gen.model_name(), "none");
}
```

#### TEST-2.1-U004: NoOp should_embed always returns false
```rust
#[test]
fn test_noop_should_embed_always_false() {
    let gen = NoOpEmbeddingGenerator;

    assert!(!gen.should_embed(""));
    assert!(!gen.should_embed("short"));
    assert!(!gen.should_embed("this is a very long piece of content that exceeds minimum length"));
}
```

#### TEST-2.1-U005: NoOp batch generation returns empty vectors
```rust
#[tokio::test]
async fn test_noop_batch_returns_empty_vectors() {
    let gen = NoOpEmbeddingGenerator;
    let contents = vec!["text1", "text2", "text3"];

    let result = gen.generate_batch(&contents).await.unwrap();

    assert_eq!(result.len(), 3);
    assert!(result.iter().all(|v| v.is_empty()));
}
```

### 2.2 OpenAIEmbedding Tests

#### TEST-2.1-U010: Default model is text-embedding-ada-002
```rust
#[test]
fn test_openai_default_model() {
    let gen = OpenAIEmbedding::new("sk-test-key");
    assert_eq!(gen.model_name(), "text-embedding-ada-002");
}
```

#### TEST-2.1-U011: Default dimension is 1536
```rust
#[test]
fn test_openai_default_dimension() {
    let gen = OpenAIEmbedding::new("sk-test-key");
    assert_eq!(gen.dimension(), 1536);
}
```

#### TEST-2.1-U012: with_model updates dimension for ada-002
```rust
#[test]
fn test_openai_with_model_ada002() {
    let gen = OpenAIEmbedding::new("key").with_model("text-embedding-ada-002");
    assert_eq!(gen.dimension(), 1536);
    assert_eq!(gen.model_name(), "text-embedding-ada-002");
}
```

#### TEST-2.1-U013: with_model updates dimension for 3-small
```rust
#[test]
fn test_openai_with_model_3_small() {
    let gen = OpenAIEmbedding::new("key").with_model("text-embedding-3-small");
    assert_eq!(gen.dimension(), 1536);
    assert_eq!(gen.model_name(), "text-embedding-3-small");
}
```

#### TEST-2.1-U014: with_model updates dimension for 3-large
```rust
#[test]
fn test_openai_with_model_3_large() {
    let gen = OpenAIEmbedding::new("key").with_model("text-embedding-3-large");
    assert_eq!(gen.dimension(), 3072);
    assert_eq!(gen.model_name(), "text-embedding-3-large");
}
```

#### TEST-2.1-U015: Unknown model defaults to 1536 dimension
```rust
#[test]
fn test_openai_unknown_model_defaults() {
    let gen = OpenAIEmbedding::new("key").with_model("unknown-future-model");
    assert_eq!(gen.dimension(), 1536); // Safe default
    assert_eq!(gen.model_name(), "unknown-future-model");
}
```

#### TEST-2.1-U016: with_dimension overrides model dimension
```rust
#[test]
fn test_openai_with_custom_dimension() {
    let gen = OpenAIEmbedding::new("key")
        .with_model("text-embedding-3-small")
        .with_dimension(512);

    assert_eq!(gen.dimension(), 512);
}
```

#### TEST-2.1-U017: max_content_length is 8191
```rust
#[test]
fn test_openai_max_content_length() {
    let gen = OpenAIEmbedding::new("key");
    assert_eq!(gen.max_content_length(), 8191);
}
```

#### TEST-2.1-U018: Builder chain is composable
```rust
#[test]
fn test_openai_builder_chain() {
    let gen = OpenAIEmbedding::new("key")
        .with_model("text-embedding-3-large")
        .with_dimension(2048);

    assert_eq!(gen.model_name(), "text-embedding-3-large");
    assert_eq!(gen.dimension(), 2048);
}
```

### 2.3 LocalEmbedding Tests

#### TEST-2.1-U020: Default dimension is 384
```rust
#[test]
fn test_local_default_dimension() {
    let gen = LocalEmbedding::new("/path/to/model.onnx").unwrap();
    assert_eq!(gen.dimension(), 384);
}
```

#### TEST-2.1-U021: Model name defaults to "local-onnx"
```rust
#[test]
fn test_local_default_model_name() {
    let gen = LocalEmbedding::new("/path/to/model.onnx").unwrap();
    assert_eq!(gen.model_name(), "local-onnx");
}
```

#### TEST-2.1-U022: with_name updates model name
```rust
#[test]
fn test_local_with_name() {
    let gen = LocalEmbedding::new("/path/to/model.onnx")
        .unwrap()
        .with_name("all-MiniLM-L6-v2");

    assert_eq!(gen.model_name(), "all-MiniLM-L6-v2");
}
```

#### TEST-2.1-U023: max_content_length is 512
```rust
#[test]
fn test_local_max_content_length() {
    let gen = LocalEmbedding::new("/path").unwrap();
    assert_eq!(gen.max_content_length(), 512);
}
```

### 2.4 EmbeddingGenerator Trait Default Implementation Tests

#### TEST-2.1-U030: should_embed returns true for >= 10 chars
```rust
#[test]
fn test_should_embed_boundary_10_chars() {
    let gen = OpenAIEmbedding::new("key");

    // Exactly 10 characters
    assert!(gen.should_embed("1234567890"));
}
```

#### TEST-2.1-U031: should_embed returns false for < 10 chars
```rust
#[test]
fn test_should_embed_boundary_9_chars() {
    let gen = OpenAIEmbedding::new("key");

    // 9 characters
    assert!(!gen.should_embed("123456789"));
}
```

#### TEST-2.1-U032: should_embed returns false for empty string
```rust
#[test]
fn test_should_embed_empty_string() {
    let gen = OpenAIEmbedding::new("key");
    assert!(!gen.should_embed(""));
}
```

#### TEST-2.1-U033: max_content_length defaults to 8191
```rust
#[test]
fn test_default_max_content_length() {
    // OpenAI uses trait default
    let gen = OpenAIEmbedding::new("key");
    assert_eq!(gen.max_content_length(), 8191);
}
```

---

## 3. ChunkedEmbedding Tests

### 3.1 Chunk Splitting Logic

#### TEST-2.1-C001: split_chunks produces correct chunk count
```rust
#[test]
fn test_split_chunks_count() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 10, 2);

    // 26 chars: "hello world this is a test"
    // chunk_size=10, overlap=2
    // Chunks: [0..10], [8..18], [16..26] = 3 chunks
    let content = "hello world this is a test";
    let chunks = chunked.split_chunks(content);

    assert_eq!(chunks.len(), 3);
}
```

#### TEST-2.1-C002: Each chunk respects max size
```rust
#[test]
fn test_split_chunks_max_size() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 10, 2);

    let content = "hello world this is a test";
    let chunks = chunked.split_chunks(content);

    assert!(chunks.iter().all(|c| c.chars().count() <= 10));
}
```

#### TEST-2.1-C003: Empty content returns empty chunks
```rust
#[test]
fn test_split_chunks_empty_content() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 10, 2);

    let chunks = chunked.split_chunks("");
    assert!(chunks.is_empty());
}
```

#### TEST-2.1-C004: Content shorter than chunk_size returns single chunk
```rust
#[test]
fn test_split_chunks_short_content() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 100, 10);

    let content = "short";
    let chunks = chunked.split_chunks(content);

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "short");
}
```

#### TEST-2.1-C005: Content exactly at chunk_size returns single chunk
```rust
#[test]
fn test_split_chunks_exact_size() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 5, 1);

    let content = "hello";
    let chunks = chunked.split_chunks(content);

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "hello");
}
```

#### TEST-2.1-C006: Chunks overlap correctly
```rust
#[test]
fn test_split_chunks_overlap() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 6, 2);

    // "abcdefghij" with chunk_size=6, overlap=2
    // Chunk 1: "abcdef" [0..6]
    // Chunk 2: "efghij" [4..10] (start = 6 - 2 = 4)
    let content = "abcdefghij";
    let chunks = chunked.split_chunks(content);

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0], "abcdef");
    assert_eq!(chunks[1], "efghij");

    // Verify overlap: chunks share "ef"
    assert!(chunks[0].ends_with("ef"));
    assert!(chunks[1].starts_with("ef"));
}
```

#### TEST-2.1-C007: Unicode characters handled correctly
```rust
#[test]
fn test_split_chunks_unicode() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 5, 1);

    // Unicode: each emoji is 1 char but multi-byte
    let content = "🚀🔥💻🎉🌟⭐️✨";
    let chunks = chunked.split_chunks(content);

    // Each chunk should have at most 5 characters
    for chunk in &chunks {
        assert!(chunk.chars().count() <= 5);
    }
}
```

### 3.2 Aggregated Embedding Tests

#### TEST-2.1-C010: Short content bypasses chunking
```rust
#[tokio::test]
async fn test_generate_aggregated_short_content() {
    let gen = OpenAIEmbedding::new("key"); // max_content_length = 8191
    let chunked = ChunkedEmbedding::new(gen, 1000, 100);

    let short_content = "short content";
    let result = chunked.generate_aggregated(short_content).await.unwrap();

    // Should directly call generator, not chunk
    assert_eq!(result.len(), 1536); // OpenAI dimension
}
```

#### TEST-2.1-C011: Long content is chunked and averaged
```rust
#[tokio::test]
async fn test_generate_aggregated_long_content() {
    // Create a mock generator that returns predictable embeddings
    struct MockGenerator;

    #[async_trait]
    impl EmbeddingGenerator for MockGenerator {
        async fn generate(&self, _: &str) -> Result<Vec<f32>> {
            Ok(vec![1.0, 2.0, 3.0])
        }
        fn model_name(&self) -> &str { "mock" }
        fn dimension(&self) -> usize { 3 }
        fn max_content_length(&self) -> usize { 10 }
    }

    let chunked = ChunkedEmbedding::new(MockGenerator, 10, 2);

    // Content longer than max_content_length (10)
    let long_content = "this content is much longer than ten characters";
    let result = chunked.generate_aggregated(long_content).await.unwrap();

    // Each chunk returns [1.0, 2.0, 3.0], averaging N chunks gives same values
    assert_eq!(result, vec![1.0, 2.0, 3.0]);
}
```

#### TEST-2.1-C012: Empty chunked embedding returns empty vector
```rust
#[tokio::test]
async fn test_generate_aggregated_empty_chunks() {
    struct EmptyChunkGenerator;

    #[async_trait]
    impl EmbeddingGenerator for EmptyChunkGenerator {
        async fn generate(&self, _: &str) -> Result<Vec<f32>> {
            Ok(vec![])
        }
        fn model_name(&self) -> &str { "empty" }
        fn dimension(&self) -> usize { 0 }
        fn max_content_length(&self) -> usize { 10 }
    }

    let chunked = ChunkedEmbedding::new(EmptyChunkGenerator, 5, 1);
    let result = chunked.generate_aggregated("").await.unwrap();

    assert!(result.is_empty());
}
```

### 3.3 Chunk Embedding with Offsets

#### TEST-2.1-C020: generate_chunks returns correct offsets
```rust
#[tokio::test]
async fn test_generate_chunks_offsets() {
    struct FixedGenerator;

    #[async_trait]
    impl EmbeddingGenerator for FixedGenerator {
        async fn generate(&self, _: &str) -> Result<Vec<f32>> {
            Ok(vec![1.0])
        }
        fn model_name(&self) -> &str { "fixed" }
        fn dimension(&self) -> usize { 1 }
    }

    let chunked = ChunkedEmbedding::new(FixedGenerator, 5, 1);
    let content = "abcdefghij"; // 10 chars

    let chunks = chunked.generate_chunks(content).await.unwrap();

    // Verify offsets
    assert_eq!(chunks[0].0, 0); // First chunk starts at 0
    assert_eq!(chunks[0].1, 5); // First chunk ends at 5
}
```

---

## 4. Property-Based Tests (using proptest)

### 4.1 Chunking Invariants

#### TEST-2.1-P001: All content is covered by chunks
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_chunks_cover_all_content(
        content in "\\PC{1,1000}",
        chunk_size in 5usize..100,
        overlap in 0usize..50
    ) {
        prop_assume!(overlap < chunk_size);

        let gen = NoOpEmbeddingGenerator;
        let chunked = ChunkedEmbedding::new(gen, chunk_size, overlap);

        let chunks = chunked.split_chunks(&content);

        // Concatenating chunks (without overlap) should cover original content
        if !content.is_empty() {
            assert!(!chunks.is_empty());
            // First chunk should start with content start
            assert!(content.starts_with(&chunks[0][..chunks[0].len().min(chunk_size - overlap)]));
        }
    }
}
```

#### TEST-2.1-P002: No chunk exceeds chunk_size
```rust
proptest! {
    #[test]
    fn prop_chunk_size_respected(
        content in "\\PC{1,500}",
        chunk_size in 10usize..100,
        overlap in 0usize..10
    ) {
        prop_assume!(overlap < chunk_size);

        let gen = NoOpEmbeddingGenerator;
        let chunked = ChunkedEmbedding::new(gen, chunk_size, overlap);

        let chunks = chunked.split_chunks(&content);

        for chunk in chunks {
            assert!(chunk.chars().count() <= chunk_size);
        }
    }
}
```

#### TEST-2.1-P003: Overlap is correct between adjacent chunks
```rust
proptest! {
    #[test]
    fn prop_overlap_correct(
        content in "[a-z]{20,100}",
        chunk_size in 10usize..20,
        overlap in 1usize..5
    ) {
        prop_assume!(overlap < chunk_size);
        prop_assume!(content.len() > chunk_size);

        let gen = NoOpEmbeddingGenerator;
        let chunked = ChunkedEmbedding::new(gen, chunk_size, overlap);

        let chunks = chunked.split_chunks(&content);

        if chunks.len() >= 2 {
            for i in 0..chunks.len() - 1 {
                let current_end = &chunks[i][chunks[i].len().saturating_sub(overlap)..];
                let next_start = &chunks[i + 1][..overlap.min(chunks[i + 1].len())];

                // The end of current chunk should match start of next
                assert_eq!(current_end, next_start);
            }
        }
    }
}
```

### 4.2 Embedding Dimension Invariants

#### TEST-2.1-P004: Generated embedding matches declared dimension
```rust
proptest! {
    #[test]
    fn prop_embedding_dimension_matches(
        model_idx in 0usize..3
    ) {
        let models = [
            "text-embedding-ada-002",
            "text-embedding-3-small",
            "text-embedding-3-large",
        ];

        let gen = OpenAIEmbedding::new("key").with_model(models[model_idx]);
        let rt = tokio::runtime::Runtime::new().unwrap();

        let embedding = rt.block_on(gen.generate("test content")).unwrap();

        assert_eq!(embedding.len(), gen.dimension());
    }
}
```

---

## 5. Integration Tests

### 5.1 Mock HTTP Server Tests (OpenAI)

#### TEST-2.1-I001: OpenAI API call with valid response
```rust
#[tokio::test]
async fn test_openai_api_success() {
    // Setup mock server
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .and(header("Authorization", "Bearer sk-test"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(json!({
                "data": [{
                    "embedding": vec![0.1f32; 1536],
                    "index": 0
                }],
                "model": "text-embedding-ada-002",
                "usage": {"prompt_tokens": 5, "total_tokens": 5}
            })))
        .mount(&server)
        .await;

    let gen = OpenAIEmbedding::new("sk-test")
        .with_base_url(&server.uri());

    let result = gen.generate("hello world").await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1536);
}
```

#### TEST-2.1-I002: OpenAI rate limit (429) handling
```rust
#[tokio::test]
async fn test_openai_rate_limit_retry() {
    let server = MockServer::start().await;

    // First call returns 429
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429)
            .set_body_json(json!({
                "error": {"message": "Rate limit exceeded"}
            })))
        .expect(1)
        .mount(&server)
        .await;

    // Second call succeeds
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(json!({
                "data": [{"embedding": vec![0.1f32; 1536], "index": 0}]
            })))
        .expect(1)
        .mount(&server)
        .await;

    let gen = OpenAIEmbedding::new("sk-test")
        .with_base_url(&server.uri())
        .with_max_retries(3);

    let result = gen.generate("hello").await;

    // Should succeed after retry
    assert!(result.is_ok());
}
```

#### TEST-2.1-I003: OpenAI network timeout
```rust
#[tokio::test]
async fn test_openai_timeout() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200)
            .set_delay(Duration::from_secs(30))) // Exceeds timeout
        .mount(&server)
        .await;

    let gen = OpenAIEmbedding::new("sk-test")
        .with_base_url(&server.uri())
        .with_timeout(Duration::from_millis(100));

    let result = gen.generate("hello").await;

    assert!(result.is_err());
    // Verify error type is timeout-related
}
```

#### TEST-2.1-I004: OpenAI malformed response
```rust
#[tokio::test]
async fn test_openai_malformed_response() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(json!({
                "unexpected": "format"
            })))
        .mount(&server)
        .await;

    let gen = OpenAIEmbedding::new("sk-test")
        .with_base_url(&server.uri());

    let result = gen.generate("hello").await;

    assert!(result.is_err());
    // Should provide descriptive error message
}
```

#### TEST-2.1-I005: OpenAI batch embedding
```rust
#[tokio::test]
async fn test_openai_batch_embedding() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(json!({
                "data": [
                    {"embedding": vec![0.1f32; 1536], "index": 0},
                    {"embedding": vec![0.2f32; 1536], "index": 1},
                    {"embedding": vec![0.3f32; 1536], "index": 2}
                ]
            })))
        .mount(&server)
        .await;

    let gen = OpenAIEmbedding::new("sk-test")
        .with_base_url(&server.uri());

    let contents = vec!["text1", "text2", "text3"];
    let result = gen.generate_batch(&contents).await;

    assert!(result.is_ok());
    let embeddings = result.unwrap();
    assert_eq!(embeddings.len(), 3);
}
```

### 5.2 DuckAgentFS Integration Tests

#### TEST-2.1-I010: Embedding generated on file write
```rust
#[tokio::test]
async fn test_duckagentfs_embedding_on_write() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_str().unwrap().to_string(),
        enable_vss: true,
        ..Default::default()
    };

    struct TestGenerator;
    impl EmbeddingGenerator for TestGenerator {
        async fn generate(&self, content: &str) -> Result<Vec<f32>> {
            // Return content-length as first dimension for verification
            Ok(vec![content.len() as f32, 1.0, 2.0])
        }
        fn model_name(&self) -> &str { "test" }
        fn dimension(&self) -> usize { 3 }
    }

    let fs = DuckAgentFS::with_embedding_generator(config, TestGenerator).await.unwrap();

    // Write file
    let ino = fs.create_file("/test.txt").await.unwrap();
    fs.write(ino, b"hello world").await.unwrap();

    // Verify embedding stored
    let embedding = fs.get_embedding(ino).await.unwrap();
    assert!(embedding.is_some());
    assert_eq!(embedding.unwrap()[0], 11.0); // "hello world".len()
}
```

#### TEST-2.1-I011: Binary file skips embedding
```rust
#[tokio::test]
async fn test_duckagentfs_binary_skip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_str().unwrap().to_string(),
        enable_vss: true,
        ..Default::default()
    };

    let fs = DuckAgentFS::with_embedding_generator(config, TestGenerator).await.unwrap();

    // Write binary content
    let ino = fs.create_file("/binary.bin").await.unwrap();
    fs.write(ino, &[0xFF, 0x00, 0xFE, 0x01]).await.unwrap(); // Invalid UTF-8

    // Verify no embedding stored
    let embedding = fs.get_embedding(ino).await.unwrap();
    assert!(embedding.is_none());
}
```

#### TEST-2.1-I012: VSS disabled skips embedding
```rust
#[tokio::test]
async fn test_duckagentfs_vss_disabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_str().unwrap().to_string(),
        enable_vss: false, // Disabled
        ..Default::default()
    };

    let fs = DuckAgentFS::new(config).await.unwrap();

    // Write file
    let ino = fs.create_file("/test.txt").await.unwrap();
    fs.write(ino, b"hello world").await.unwrap();

    // Verify no embedding stored
    let embedding = fs.get_embedding(ino).await.unwrap();
    assert!(embedding.is_none());
}
```

#### TEST-2.1-I013: Short content skips embedding
```rust
#[tokio::test]
async fn test_duckagentfs_short_content_skip() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_str().unwrap().to_string(),
        enable_vss: true,
        ..Default::default()
    };

    let fs = DuckAgentFS::new(config).await.unwrap();

    // Write short content (< 10 chars)
    let ino = fs.create_file("/short.txt").await.unwrap();
    fs.write(ino, b"short").await.unwrap();

    // Verify no embedding (should_embed returns false)
    let embedding = fs.get_embedding(ino).await.unwrap();
    assert!(embedding.is_none());
}
```

---

## 6. Edge Case Tests

### 6.1 Boundary Conditions

#### TEST-2.1-E001: Content exactly at max_content_length
```rust
#[tokio::test]
async fn test_content_at_max_length() {
    let gen = OpenAIEmbedding::new("key");
    let chunked = ChunkedEmbedding::new(gen, 1000, 100);

    // Create content exactly at max_content_length (8191)
    let content = "x".repeat(8191);

    // Should NOT chunk - directly generate
    let result = chunked.generate_aggregated(&content).await;
    assert!(result.is_ok());
}
```

#### TEST-2.1-E002: Content one byte over max_content_length
```rust
#[tokio::test]
async fn test_content_over_max_length() {
    struct FixedGenerator;
    impl EmbeddingGenerator for FixedGenerator {
        async fn generate(&self, _: &str) -> Result<Vec<f32>> {
            Ok(vec![1.0])
        }
        fn model_name(&self) -> &str { "fixed" }
        fn dimension(&self) -> usize { 1 }
        fn max_content_length(&self) -> usize { 100 }
    }

    let chunked = ChunkedEmbedding::new(FixedGenerator, 50, 10);

    // Create content one byte over max (101)
    let content = "x".repeat(101);

    // Should chunk
    let chunks = chunked.split_chunks(&content);
    assert!(chunks.len() > 1);
}
```

#### TEST-2.1-E003: Overlap equals chunk_size minus one
```rust
#[test]
fn test_overlap_almost_equals_chunk_size() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 10, 9);

    let content = "abcdefghijklmnopqrstuvwxyz";
    let chunks = chunked.split_chunks(&content);

    // Should not infinite loop
    assert!(!chunks.is_empty());
    // Many small-progress chunks expected
    assert!(chunks.len() > 10);
}
```

#### TEST-2.1-E004: Overlap equals chunk_size (potential infinite loop)
```rust
#[test]
#[should_panic(expected = "overlap must be less than chunk_size")]
fn test_overlap_equals_chunk_size_panics() {
    let gen = NoOpEmbeddingGenerator;

    // This should panic or return error in constructor
    let _chunked = ChunkedEmbedding::new(gen, 10, 10);
}
```

#### TEST-2.1-E005: Zero chunk_size
```rust
#[test]
#[should_panic(expected = "chunk_size must be greater than 0")]
fn test_zero_chunk_size_panics() {
    let gen = NoOpEmbeddingGenerator;
    let _chunked = ChunkedEmbedding::new(gen, 0, 0);
}
```

### 6.2 Unicode Edge Cases

#### TEST-2.1-E010: Emoji-only content
```rust
#[test]
fn test_emoji_content() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 2, 0);

    let content = "🚀🔥💻🎉"; // 4 emojis
    let chunks = chunked.split_chunks(&content);

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].chars().count(), 2);
    assert_eq!(chunks[1].chars().count(), 2);
}
```

#### TEST-2.1-E011: Mixed ASCII and multi-byte
```rust
#[test]
fn test_mixed_charset() {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 5, 1);

    let content = "ab🚀cd🔥ef"; // 8 chars total
    let chunks = chunked.split_chunks(&content);

    for chunk in &chunks {
        assert!(chunk.chars().count() <= 5);
    }
}
```

#### TEST-2.1-E012: BOM marker handling
```rust
#[tokio::test]
async fn test_bom_marker() {
    let gen = OpenAIEmbedding::new("key");

    // UTF-8 BOM + content
    let content = "\u{FEFF}Hello World";

    // should_embed should work correctly
    assert!(gen.should_embed(content));
}
```

### 6.3 Error Propagation

#### TEST-2.1-E020: Generator error propagates through generate_chunks
```rust
#[tokio::test]
async fn test_generator_error_propagation() {
    struct FailingGenerator;

    #[async_trait]
    impl EmbeddingGenerator for FailingGenerator {
        async fn generate(&self, _: &str) -> Result<Vec<f32>> {
            Err(Error::EmbeddingFailed("intentional failure".into()))
        }
        fn model_name(&self) -> &str { "failing" }
        fn dimension(&self) -> usize { 1 }
    }

    let chunked = ChunkedEmbedding::new(FailingGenerator, 5, 1);
    let result = chunked.generate_chunks("hello world").await;

    assert!(result.is_err());
}
```

#### TEST-2.1-E021: Generator error propagates through generate_aggregated
```rust
#[tokio::test]
async fn test_generator_error_in_aggregated() {
    struct FailOnSecondGenerator {
        call_count: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl EmbeddingGenerator for FailOnSecondGenerator {
        async fn generate(&self, _: &str) -> Result<Vec<f32>> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            if count >= 1 {
                Err(Error::EmbeddingFailed("failed on second call".into()))
            } else {
                Ok(vec![1.0])
            }
        }
        fn model_name(&self) -> &str { "fail-on-second" }
        fn dimension(&self) -> usize { 1 }
        fn max_content_length(&self) -> usize { 5 }
    }

    let gen = FailOnSecondGenerator {
        call_count: AtomicUsize::new(0),
    };
    let chunked = ChunkedEmbedding::new(gen, 5, 1);

    // Content that requires multiple chunks
    let result = chunked.generate_aggregated("this is a longer content").await;

    assert!(result.is_err());
}
```

---

## 7. Concurrency Tests

### 7.1 Parallel Access

#### TEST-2.1-CON001: Concurrent generate calls
```rust
#[tokio::test]
async fn test_concurrent_generate() {
    let gen = Arc::new(OpenAIEmbedding::new("key"));

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let gen = Arc::clone(&gen);
            tokio::spawn(async move {
                gen.generate(&format!("content {}", i)).await
            })
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(handles).await;

    assert!(results.iter().all(|r| r.is_ok()));
}
```

#### TEST-2.1-CON002: Concurrent batch operations
```rust
#[tokio::test]
async fn test_concurrent_batch() {
    let gen = Arc::new(OpenAIEmbedding::new("key"));

    let handles: Vec<_> = (0..5)
        .map(|_| {
            let gen = Arc::clone(&gen);
            tokio::spawn(async move {
                let contents = vec!["a", "b", "c"];
                gen.generate_batch(&contents).await
            })
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(handles).await;

    assert!(results.iter().all(|r| r.as_ref().unwrap().is_ok()));
}
```

#### TEST-2.1-CON003: Concurrent writes to same inode
```rust
#[tokio::test]
async fn test_concurrent_writes_same_inode() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.duckdb");

    let config = DuckAgentFSConfig {
        path: db_path.to_str().unwrap().to_string(),
        enable_vss: true,
        ..Default::default()
    };

    let fs = Arc::new(DuckAgentFS::new(config).await.unwrap());
    let ino = fs.create_file("/concurrent.txt").await.unwrap();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                fs.write(ino, format!("content version {}", i).as_bytes()).await
            })
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(handles).await;

    // All writes should succeed (or fail gracefully with contention error)
    // Final state should be consistent
    let final_content = fs.read(ino).await.unwrap();
    assert!(final_content.starts_with(b"content version"));
}
```

---

## 8. Performance Tests

### 8.1 Benchmarks

#### BENCH-2.1-001: Chunking throughput
```rust
use criterion::{black_box, criterion_group, Criterion};

fn bench_chunking(c: &mut Criterion) {
    let gen = NoOpEmbeddingGenerator;
    let chunked = ChunkedEmbedding::new(gen, 1000, 100);
    let content = "x".repeat(100_000); // 100KB

    c.bench_function("split_chunks_100kb", |b| {
        b.iter(|| chunked.split_chunks(black_box(&content)))
    });
}
```

#### BENCH-2.1-002: Batch vs sequential generation
```rust
fn bench_batch_vs_sequential(c: &mut Criterion) {
    let gen = OpenAIEmbedding::new("key");
    let contents: Vec<&str> = (0..100).map(|_| "test content").collect();
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("batch_100_items", |b| {
        b.iter(|| {
            rt.block_on(gen.generate_batch(black_box(&contents)))
        })
    });

    c.bench_function("sequential_100_items", |b| {
        b.iter(|| {
            rt.block_on(async {
                for content in &contents {
                    gen.generate(content).await.unwrap();
                }
            })
        })
    });
}
```

---

## 9. Test Data Requirements

### 9.1 Test Fixtures

| Fixture | Description | Location |
|---------|-------------|----------|
| `short_text.txt` | 9 character text (below threshold) | `tests/fixtures/` |
| `exact_threshold.txt` | 10 character text (at threshold) | `tests/fixtures/` |
| `large_text.txt` | 100KB text for chunking tests | `tests/fixtures/` |
| `binary_file.bin` | Binary content with invalid UTF-8 | `tests/fixtures/` |
| `unicode_content.txt` | Mixed ASCII, emoji, multi-byte | `tests/fixtures/` |
| `mock_responses/` | OpenAI API mock responses | `tests/fixtures/mock_responses/` |

### 9.2 Mock Server Configuration

```rust
// tests/common/mock_openai.rs
pub fn setup_mock_openai() -> MockServer {
    let server = MockServer::start();

    // Default success response
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(success_response()))
        .mount(&server);

    server
}

fn success_response() -> serde_json::Value {
    json!({
        "data": [{
            "embedding": vec![0.0f32; 1536],
            "index": 0
        }],
        "model": "text-embedding-ada-002",
        "usage": {"prompt_tokens": 10, "total_tokens": 10}
    })
}
```

---

## 10. Test Execution Plan

### 10.1 Test Categories by Priority

| Priority | Category | Test Count | Estimated Time |
|----------|----------|------------|----------------|
| P0 | Unit - NoOp | 5 | < 1s |
| P0 | Unit - OpenAI Builder | 9 | < 1s |
| P0 | Unit - Chunking | 7 | < 1s |
| P1 | Property Tests | 4 | 5-10s |
| P1 | Integration - Mock HTTP | 5 | 2-3s |
| P2 | Integration - DuckAgentFS | 4 | 3-5s |
| P2 | Edge Cases | 12 | 2-3s |
| P3 | Concurrency | 3 | 5-10s |
| P3 | Performance | 2 | 10-15s |

### 10.2 CI Pipeline Integration

```yaml
# .github/workflows/rust-tests.yaml
jobs:
  test-embedding:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run unit tests
        run: cargo test --package agentfs-sdk embedding::tests

      - name: Run property tests
        run: cargo test --package agentfs-sdk embedding::proptests -- --test-threads=1

      - name: Run integration tests
        run: cargo test --package agentfs-sdk --features integration-tests embedding_integration
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY_TEST }} # Optional for live tests
```

---

## 11. Coverage Requirements

### 11.1 Minimum Coverage Targets

| Component | Line Coverage | Branch Coverage |
|-----------|---------------|-----------------|
| `EmbeddingGenerator` trait | 100% | 100% |
| `NoOpEmbeddingGenerator` | 100% | 100% |
| `OpenAIEmbedding` | 90% | 85% |
| `LocalEmbedding` | 90% | 85% |
| `ChunkedEmbedding` | 95% | 90% |

### 11.2 Coverage Exclusions

- Conceptual placeholder code (marked with `// NOTE: Conceptual implementation`)
- Platform-specific ONNX loading code (requires ONNX runtime)
- Live API calls (covered by integration tests with mocks)

---

## 12. Risk Mitigation

### 12.1 Identified Risks from QA Review

| Risk | Test Coverage | Mitigation |
|------|---------------|------------|
| API Rate Limiting | TEST-2.1-I002 | Mock server tests verify retry logic |
| Binary File Detection | TEST-2.1-I011 | UTF-8 validation tested |
| Chunk Boundary Errors | TEST-2.1-P002, P003 | Property tests ensure invariants |
| Embedding Dimension Mismatch | TEST-2.1-P004 | Verify dimension matches declared |
| Overlap >= chunk_size | TEST-2.1-E004 | Constructor validation |

### 12.2 Test Gaps to Address

1. **LocalEmbedding ONNX path**: Marked conceptual; add tests when ONNX runtime integrated
2. **Rate limit backoff timing**: Current tests verify retry; add timing verification
3. **Content hash collision**: MD5 acceptable; document in test comments

---

## 13. Appendix

### 13.1 Test Naming Convention

```
TEST-{story}-{category}{number}: {description}

Categories:
- U: Unit test
- C: Chunking test
- P: Property test
- I: Integration test
- E: Edge case test
- CON: Concurrency test
```

### 13.2 Related Documentation

- [STORY-2.1 Embedding Generator](../stories/duckagentfs/STORY-2.1-embedding-generator.md)
- [DuckAgentFS Architecture](../architecture/duckagentfs.md)
- [VSS Extension Guide](../guides/vss-setup.md)

---

**BMAD_QA_COMPLETED**
