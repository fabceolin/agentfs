# STORY-4.1: MCP Tools for DuckAgentFS

> **NOTE**: This documentation is conceptual. Changes may be made during the implementation phase.

## Metadata

| Field | Value |
|-------|-------|
| **ID** | STORY-4.1 |
| **Epic** | EPIC-DUCKAGENTFS-001 |
| **Phase** | 4 - MCP Server Integration |
| **Status** | Todo |
| **Priority** | Medium |
| **File** | `cli/src/cmd/mcp_server.rs` |
| **Dependencies** | STORY-2.3, STORY-1.4, STORY-3.3 |

## User Story

**As an** AI agent
**I want** MCP tools for DuckAgentFS
**So that** I can interact via the standard protocol

## Technical Description

MCP (Model Context Protocol) provides a standardized way for AI models to interact with external tools. We extend the existing AgentFS MCP server with DuckAgentFS-specific capabilities:

- Semantic search
- Time-travel queries
- Code graph navigation
- Impact analysis

## Acceptance Criteria

- [ ] Tool `duckagentfs_search`: semantic search
- [ ] Tool `duckagentfs_snapshot`: time-travel
- [ ] Tool `duckagentfs_graph_query`: graph queries
- [ ] Tool `duckagentfs_analyze`: impact analysis
- [ ] Integration with existing MCP server

## Technical Specification

### Tool Definitions

```rust
// Tools to add to MCP server

pub fn duck_agentfs_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "duckagentfs_search".to_string(),
            description: "Search files by semantic similarity. \
                         Find files based on meaning, not exact keywords.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results (default: 10)",
                        "default": 10
                    },
                    "directory": {
                        "type": "string",
                        "description": "Filter by directory prefix"
                    },
                    "extensions": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by file extensions"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "duckagentfs_snapshot".to_string(),
            description: "Access filesystem at a specific point in time. \
                         Useful for viewing historical state or recovering deleted files.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "event_id": {
                        "type": "integer",
                        "description": "Event ID for the snapshot"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["list_events", "read_file", "list_dir", "diff"],
                        "description": "Action to perform"
                    },
                    "path": {
                        "type": "string",
                        "description": "File or directory path (for read_file, list_dir)"
                    },
                    "from_event": {
                        "type": "integer",
                        "description": "Start event for diff"
                    },
                    "to_event": {
                        "type": "integer",
                        "description": "End event for diff"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            name: "duckagentfs_graph_query".to_string(),
            description: "Query code dependency graph. \
                         Find callers, callees, and navigate code relationships.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query_type": {
                        "type": "string",
                        "enum": ["callers", "callees", "file_deps", "dead_code"],
                        "description": "Type of graph query"
                    },
                    "symbol_id": {
                        "type": "string",
                        "description": "Symbol ID (for callers, callees)"
                    },
                    "path": {
                        "type": "string",
                        "description": "File path (for file_deps)"
                    }
                },
                "required": ["query_type"]
            }),
        },
        Tool {
            name: "duckagentfs_analyze".to_string(),
            description: "Analyze code impact. \
                         Find what would be affected if a symbol changes.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol_id": {
                        "type": "string",
                        "description": "Symbol to analyze"
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum dependency depth",
                        "default": 5
                    }
                },
                "required": ["symbol_id"]
            }),
        },
    ]
}
```

### Tool Handlers

```rust
impl DuckAgentFSMcpHandler {
    pub async fn handle_search(&self, args: &Value) -> Result<Value> {
        let query = args["query"].as_str()
            .ok_or_else(|| Error::Custom("query required".into()))?;

        let options = SearchOptions {
            limit: args["limit"].as_u64().unwrap_or(10) as usize,
            directory: args["directory"].as_str().map(String::from),
            extensions: args["extensions"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()),
            ..Default::default()
        };

        let results = self.fs.search(query, options).await?;

        Ok(json!({
            "results": results.iter().map(|r| json!({
                "path": r.path,
                "score": r.score,
                "preview": r.preview.chars().take(200).collect::<String>()
            })).collect::<Vec<_>>()
        }))
    }

    pub async fn handle_snapshot(&self, args: &Value) -> Result<Value> {
        let action = args["action"].as_str()
            .ok_or_else(|| Error::Custom("action required".into()))?;

        match action {
            "list_events" => {
                let events = self.fs.list_events(20, 0).await?;
                Ok(json!({
                    "events": events.iter().map(|e| json!({
                        "event_id": e.event_id,
                        "type": e.event_type,
                        "time": e.event_time,
                        "name": e.name
                    })).collect::<Vec<_>>()
                }))
            }
            "read_file" => {
                let event_id = args["event_id"].as_i64()
                    .ok_or_else(|| Error::Custom("event_id required".into()))?;
                let path = args["path"].as_str()
                    .ok_or_else(|| Error::Custom("path required".into()))?;

                let snapshot = self.fs.snapshot_at(event_id).await?;
                let content = snapshot.read_file(path).await?
                    .ok_or_else(|| Error::Custom("file not found".into()))?;

                let text = String::from_utf8_lossy(&content);
                Ok(json!({ "content": text }))
            }
            "diff" => {
                let from = args["from_event"].as_i64()
                    .ok_or_else(|| Error::Custom("from_event required".into()))?;
                let to = args["to_event"].as_i64()
                    .ok_or_else(|| Error::Custom("to_event required".into()))?;

                let diffs = self.fs.diff(from, to).await?;
                Ok(json!({
                    "changes": diffs.iter().map(|d| json!({
                        "path": d.path,
                        "change": format!("{:?}", d.change_type)
                    })).collect::<Vec<_>>()
                }))
            }
            _ => Err(Error::Custom(format!("unknown action: {}", action)))
        }
    }

    pub async fn handle_graph_query(&self, args: &Value) -> Result<Value> {
        let query_type = args["query_type"].as_str()
            .ok_or_else(|| Error::Custom("query_type required".into()))?;

        let graph = CodeGraph::new(&self.fs);

        match query_type {
            "callers" => {
                let symbol_id = args["symbol_id"].as_str()
                    .ok_or_else(|| Error::Custom("symbol_id required".into()))?;

                let callers = graph.get_callers(symbol_id).await?;
                Ok(json!({
                    "callers": callers.iter().map(|c| json!({
                        "name": c.name,
                        "kind": c.kind,
                        "path": c.path
                    })).collect::<Vec<_>>()
                }))
            }
            "callees" => {
                let symbol_id = args["symbol_id"].as_str()
                    .ok_or_else(|| Error::Custom("symbol_id required".into()))?;

                let callees = graph.get_callees(symbol_id).await?;
                Ok(json!({
                    "callees": callees.iter().map(|c| json!({
                        "name": c.name,
                        "kind": c.kind,
                        "path": c.path
                    })).collect::<Vec<_>>()
                }))
            }
            "file_deps" => {
                let path = args["path"].as_str()
                    .ok_or_else(|| Error::Custom("path required".into()))?;

                let deps = graph.get_file_dependencies(path).await?;
                Ok(json!({
                    "symbols": deps.symbols.len(),
                    "depends_on": deps.depends_on,
                    "dependents": deps.dependents
                }))
            }
            "dead_code" => {
                let dead = graph.find_dead_code().await?;
                Ok(json!({
                    "dead_code": dead.iter().map(|s| json!({
                        "name": s.name,
                        "path": s.path
                    })).collect::<Vec<_>>()
                }))
            }
            _ => Err(Error::Custom(format!("unknown query_type: {}", query_type)))
        }
    }

    pub async fn handle_analyze(&self, args: &Value) -> Result<Value> {
        let symbol_id = args["symbol_id"].as_str()
            .ok_or_else(|| Error::Custom("symbol_id required".into()))?;
        let max_depth = args["max_depth"].as_u64().unwrap_or(5) as usize;

        let graph = CodeGraph::new(&self.fs);
        let impact = graph.get_impact(symbol_id, max_depth).await?;

        Ok(json!({
            "target": impact.target_symbol,
            "affected_symbols": impact.affected_symbols.len(),
            "affected_files": impact.affected_file_count,
            "details": impact.affected_symbols.iter().take(20).map(|s| json!({
                "name": s.name,
                "path": s.path,
                "distance": s.distance
            })).collect::<Vec<_>>()
        }))
    }
}
```

### MCP Server Configuration

```bash
# Start MCP server with DuckAgentFS tools
agentfs serve mcp my-agent \
    --tools fs,kv,tools,search,snapshot,graph,analyze
```

## Example Usage

### Semantic Search
```json
{
  "method": "tools/call",
  "params": {
    "name": "duckagentfs_search",
    "arguments": {
      "query": "error handling for database connections",
      "limit": 5,
      "extensions": [".rs", ".ts"]
    }
  }
}
```

### Time Travel
```json
{
  "method": "tools/call",
  "params": {
    "name": "duckagentfs_snapshot",
    "arguments": {
      "action": "read_file",
      "event_id": 150,
      "path": "/src/config.rs"
    }
  }
}
```

### Impact Analysis
```json
{
  "method": "tools/call",
  "params": {
    "name": "duckagentfs_analyze",
    "arguments": {
      "symbol_id": "src/db/connection.rs:ConnectionPool",
      "max_depth": 3
    }
  }
}
```

## Tests

### Test 1: Search Tool
```rust
#[tokio::test]
async fn test_mcp_search_tool() {
    let handler = setup_mcp_handler().await;

    let result = handler.handle_search(&json!({
        "query": "test",
        "limit": 5
    })).await.unwrap();

    assert!(result["results"].is_array());
}
```

## Related Files

| File | Description |
|------|-------------|
| `cli/src/cmd/mcp_server.rs` | MCP server implementation |
| `sdk/rust/src/filesystem/duckagentfs.rs` | DuckAgentFS methods |
| `sdk/rust/src/code_graph.rs` | Graph API |

## Implementation Notes

1. **Error Handling**: MCP errors should be clear and actionable for AI agents.

2. **Rate Limiting**: Consider rate limiting expensive operations (search, graph traversal).

3. **Response Size**: Limit response sizes to avoid overwhelming the model context.

4. **Caching**: Cache frequent queries to improve response time.
