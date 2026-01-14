# STORY-3.2: Code Analyzer Integration

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-3.2 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 3 - Property Graphs (DuckPGQ) |
| **Status** | Todo |
| **Priority** | Medium |
| **File** | `sdk/rust/src/analyzer/mod.rs` (new) |
| **Dependencies** | STORY-3.1 |

## User Story

**As a** developer
**I want** to integrate with code analyzers
**So that** the graph is populated automatically

## Technical Description

Code analyzers parse source files and extract:
- Symbol definitions (functions, classes, etc.)
- Symbol relationships (calls, imports, etc.)

We use tree-sitter for fast, incremental parsing across multiple languages.

## Acceptance Criteria

- [ ] Trait `CodeAnalyzer` for different languages
- [ ] Implementation for Rust (tree-sitter)
- [ ] Implementation for TypeScript
- [ ] Hooks in `write_file` to update graph
- [ ] Batch analysis for initial indexing

## Technical Specification

### CodeAnalyzer Trait

```rust
use crate::code_graph::{CodeSymbol, CodeDependency};

/// Result of analyzing a single file
#[derive(Debug)]
pub struct AnalysisResult {
    /// Symbols defined in this file
    pub symbols: Vec<CodeSymbol>,

    /// Dependencies between symbols
    pub dependencies: Vec<CodeDependency>,
}

/// Trait for language-specific code analyzers
pub trait CodeAnalyzer: Send + Sync {
    /// Supported file extensions
    fn extensions(&self) -> &[&str];

    /// Language identifier
    fn language(&self) -> &str;

    /// Check if file should be analyzed
    fn should_analyze(&self, path: &str) -> bool {
        self.extensions().iter().any(|ext| path.ends_with(ext))
    }

    /// Analyze source code and extract symbols/dependencies
    fn analyze(&self, path: &str, content: &str) -> Result<AnalysisResult>;
}
```

### Rust Analyzer

```rust
use tree_sitter::{Parser, Query, QueryCursor};

pub struct RustAnalyzer {
    parser: Parser,
    symbols_query: Query,
    calls_query: Query,
}

impl RustAnalyzer {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        parser.set_language(tree_sitter_rust::language())?;

        // Query for function/struct/impl definitions
        let symbols_query = Query::new(
            tree_sitter_rust::language(),
            r#"
            (function_item
                name: (identifier) @func_name
                parameters: (parameters) @params
                return_type: (_)? @return_type
            ) @function

            (struct_item
                name: (type_identifier) @struct_name
            ) @struct

            (impl_item
                type: (type_identifier) @impl_type
                body: (declaration_list
                    (function_item
                        name: (identifier) @method_name
                    ) @method
                )
            ) @impl
            "#
        )?;

        // Query for function calls
        let calls_query = Query::new(
            tree_sitter_rust::language(),
            r#"
            (call_expression
                function: (identifier) @callee
            )
            (call_expression
                function: (field_expression
                    field: (field_identifier) @method_callee
                )
            )
            "#
        )?;

        Ok(Self { parser, symbols_query, calls_query })
    }
}

impl CodeAnalyzer for RustAnalyzer {
    fn extensions(&self) -> &[&str] {
        &[".rs"]
    }

    fn language(&self) -> &str {
        "rust"
    }

    fn analyze(&self, path: &str, content: &str) -> Result<AnalysisResult> {
        let tree = self.parser.parse(content, None)
            .ok_or_else(|| Error::Custom("Failed to parse".into()))?;

        let mut symbols = Vec::new();
        let mut dependencies = Vec::new();

        // Extract symbols
        let mut cursor = QueryCursor::new();
        for match_ in cursor.matches(&self.symbols_query, tree.root_node(), content.as_bytes()) {
            for capture in match_.captures {
                match self.symbols_query.capture_names()[capture.index as usize] {
                    "function" => {
                        let name = get_capture_text(content, match_, "func_name");
                        let params = get_capture_text(content, match_, "params");
                        let start_line = capture.node.start_position().row as u32;
                        let end_line = capture.node.end_position().row as u32;

                        symbols.push(CodeSymbol {
                            id: format!("{}:{}", path, name),
                            name: name.to_string(),
                            kind: "function".to_string(),
                            language: "rust".to_string(),
                            start_line,
                            end_line,
                            signature: Some(format!("fn {}{}", name, params)),
                            ..Default::default()
                        });
                    }
                    "struct" => {
                        let name = get_capture_text(content, match_, "struct_name");
                        symbols.push(CodeSymbol {
                            id: format!("{}:{}", path, name),
                            name: name.to_string(),
                            kind: "struct".to_string(),
                            language: "rust".to_string(),
                            ..Default::default()
                        });
                    }
                    // ... handle impl, method, etc.
                    _ => {}
                }
            }
        }

        // Extract calls
        let mut current_function: Option<String> = None;
        // Walk tree to find which function each call is in
        // Then create dependencies

        Ok(AnalysisResult { symbols, dependencies })
    }
}
```

### TypeScript Analyzer

```rust
pub struct TypeScriptAnalyzer {
    parser: Parser,
    symbols_query: Query,
}

impl TypeScriptAnalyzer {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        parser.set_language(tree_sitter_typescript::language_typescript())?;

        let symbols_query = Query::new(
            tree_sitter_typescript::language_typescript(),
            r#"
            (function_declaration
                name: (identifier) @func_name
            ) @function

            (class_declaration
                name: (type_identifier) @class_name
            ) @class

            (interface_declaration
                name: (type_identifier) @interface_name
            ) @interface

            (method_definition
                name: (property_identifier) @method_name
            ) @method
            "#
        )?;

        Ok(Self { parser, symbols_query })
    }
}

impl CodeAnalyzer for TypeScriptAnalyzer {
    fn extensions(&self) -> &[&str] {
        &[".ts", ".tsx"]
    }

    fn language(&self) -> &str {
        "typescript"
    }

    fn analyze(&self, path: &str, content: &str) -> Result<AnalysisResult> {
        // Similar to Rust analyzer
        todo!()
    }
}
```

### Analyzer Registry

```rust
pub struct AnalyzerRegistry {
    analyzers: Vec<Box<dyn CodeAnalyzer>>,
}

impl AnalyzerRegistry {
    pub fn new() -> Self {
        Self { analyzers: Vec::new() }
    }

    pub fn register(&mut self, analyzer: Box<dyn CodeAnalyzer>) {
        self.analyzers.push(analyzer);
    }

    pub fn with_defaults() -> Result<Self> {
        let mut registry = Self::new();
        registry.register(Box::new(RustAnalyzer::new()?));
        registry.register(Box::new(TypeScriptAnalyzer::new()?));
        Ok(registry)
    }

    pub fn find_analyzer(&self, path: &str) -> Option<&dyn CodeAnalyzer> {
        self.analyzers.iter()
            .find(|a| a.should_analyze(path))
            .map(|a| a.as_ref())
    }

    pub fn analyze(&self, path: &str, content: &str) -> Option<Result<AnalysisResult>> {
        self.find_analyzer(path).map(|a| a.analyze(path, content))
    }
}
```

### Integration with DuckAgentFS

```rust
impl DuckAgentFS {
    /// Called after write_file to update code graph
    async fn update_code_graph(&self, path: &str, content: &[u8]) -> Result<()> {
        if !self.config.enable_pgq {
            return Ok(());
        }

        // Get text content
        let text = match std::str::from_utf8(content) {
            Ok(t) => t,
            Err(_) => return Ok(()), // Skip binary files
        };

        // Find analyzer for this file type
        let result = match self.analyzer_registry.analyze(path, text) {
            Some(Ok(r)) => r,
            Some(Err(e)) => {
                tracing::warn!("Failed to analyze {}: {}", path, e);
                return Ok(());
            }
            None => return Ok(()), // No analyzer for this file type
        };

        // Get connection and update graph
        let conn = self.pool.get_write_connection().await?;

        // Delete old symbols for this file
        let inode = self.resolve_path(path, true).await?.unwrap();
        conn.execute(
            "DELETE FROM code_symbols WHERE inode = ?",
            params![inode]
        )?;

        // Insert new symbols
        for symbol in result.symbols {
            conn.execute(
                r#"
                INSERT INTO code_symbols
                (id, inode, name, kind, language, start_line, end_line, signature, visibility)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                params![
                    symbol.id,
                    inode,
                    symbol.name,
                    symbol.kind,
                    symbol.language,
                    symbol.start_line,
                    symbol.end_line,
                    symbol.signature,
                    symbol.visibility
                ]
            )?;
        }

        // Insert dependencies
        for dep in result.dependencies {
            conn.execute(
                r#"
                INSERT OR REPLACE INTO code_dependencies
                (source_id, target_id, dep_type, weight)
                VALUES (?, ?, ?, ?)
                "#,
                params![dep.source_id, dep.target_id, dep.dep_type, dep.weight]
            )?;
        }

        Ok(())
    }
}
```

### Batch Analysis CLI

```bash
# Analyze entire codebase
agentfs analyze my-agent --dir /src

# Analyze specific file
agentfs analyze my-agent --file /src/main.rs

# Re-analyze all
agentfs analyze my-agent --all --force
```

## Tests

### Test 1: Rust Function Detection
```rust
#[test]
fn test_rust_function() {
    let analyzer = RustAnalyzer::new().unwrap();
    let result = analyzer.analyze("test.rs", r#"
        fn hello(name: &str) -> String {
            format!("Hello, {}!", name)
        }
    "#).unwrap();

    assert_eq!(result.symbols.len(), 1);
    assert_eq!(result.symbols[0].name, "hello");
    assert_eq!(result.symbols[0].kind, "function");
}
```

### Test 2: TypeScript Class Detection
```rust
#[test]
fn test_typescript_class() {
    let analyzer = TypeScriptAnalyzer::new().unwrap();
    let result = analyzer.analyze("test.ts", r#"
        class User {
            constructor(public name: string) {}

            greet(): string {
                return `Hello, ${this.name}`;
            }
        }
    "#).unwrap();

    assert!(result.symbols.iter().any(|s| s.name == "User" && s.kind == "class"));
    assert!(result.symbols.iter().any(|s| s.name == "greet" && s.kind == "method"));
}
```

### Test 3: Call Detection
```rust
#[test]
fn test_call_detection() {
    let analyzer = RustAnalyzer::new().unwrap();
    let result = analyzer.analyze("test.rs", r#"
        fn main() {
            helper();
        }

        fn helper() {}
    "#).unwrap();

    assert!(result.dependencies.iter().any(|d|
        d.source_id.contains("main") &&
        d.target_id.contains("helper") &&
        d.dep_type == "calls"
    ));
}
```

## Related Files

| File | Description |
|------|-------------|
| `sdk/rust/src/analyzer/mod.rs` | Analyzer trait and registry |
| `sdk/rust/src/analyzer/rust.rs` | Rust analyzer |
| `sdk/rust/src/analyzer/typescript.rs` | TypeScript analyzer |
| `sdk/rust/src/filesystem/duckagentfs.rs` | Integration point |

## Implementation Notes

1. **tree-sitter**: Use tree-sitter for all parsing. It's fast, incremental, and supports many languages.

2. **Dependencies**:
   ```toml
   tree-sitter = "0.22"
   tree-sitter-rust = "0.21"
   tree-sitter-typescript = "0.21"
   ```

3. **Incremental Parsing**: tree-sitter supports incremental parsing for edits. Consider caching parse trees.

4. **Cross-file Dependencies**: For imports, need to resolve module paths. This is language-specific and complex.
