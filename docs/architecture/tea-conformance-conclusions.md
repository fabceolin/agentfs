# TEA Conformance Testing Conclusions

**Date:** 2026-01-18
**Status:** Paused - Ready to continue later

## Executive Summary

Testing revealed that full document regeneration is unreliable for large documents (20K+ tokens). **Surgical field replacement** is the recommended approach for near-conformant documents.

## Model Comparison Results

| Model | Size | Status Mapping | Accuracy | Speed | Recommendation |
|-------|------|----------------|----------|-------|----------------|
| qwen3:8b | 5.2GB | WIP→InProgress | ⭐⭐⭐⭐⭐ | Slow (CPU) | Best semantic mapping |
| gemma3n:e4b | 7.5GB | WIP→Draft | ⭐⭐⭐⭐ | Fast | Good balance |
| gemma3:4b | 3.3GB | Partial | ⭐⭐ | Fast | Misses placeholders |
| deepseek-r1:1.5b | 1.1GB | Hallucinations | ⭐ | Very fast | Not recommended |

## Key Findings

### 1. Full Document Regeneration Issues
- **Token limits**: Models struggle with 20K+ token output
- **Content loss**: Summaries instead of verbatim reproduction
- **Semantic drift**: Structure changes despite instructions

### 2. Surgical Replacement Success
Tested with gemma3n:e4b:
```
Input:  | **Status** | **** Done **** |
Output: | **Status** | Done |
```
✅ Correctly extracted "Done" from malformed value

### 3. Hybrid Fix Strategy (Recommended)

```
Conformance Pipeline:
┌─────────────────────────────────────────────────────────┐
│  1. Run conformance check → Get list of invalid fields  │
│  2. For each invalid field:                             │
│     ├── Pattern-based? → sed/regex fix (instant)        │
│     └── Semantic?      → LLM single-field fix           │
│  3. Validate final document                             │
└─────────────────────────────────────────────────────────┘
```

#### When to use sed/regex:
```bash
# Known patterns with deterministic fixes
sed -i 's/\*\*\*\* Done \*\*\*\*/Done/g' file.md
sed -i 's/| WIP |/| Draft |/g' file.md
```

#### When to use LLM:
```json
{
  "prompt": "Map this value to the closest option from [Draft, Approved, InProgress, Review, Done]:\nInput: 'WIP - almost ready'\nOutput:",
  "options": { "temperature": 0.0, "num_predict": 10 }
}
```

## Known Issues

### TEA Conditional Routing Bug
The `goto` conditional in TEA agents doesn't work as expected:
```yaml
goto:
  - if: "state.conformance.is_conformant == false"
    to: build_system_instruction
  - to: passthrough
```
**Status**: Always goes to passthrough. Needs TEA framework investigation.

### Ollama API Key Handling
TEA wasn't using `base_url` from overlay settings. Workaround: Use Ollama API directly.

## Files Created/Modified

| File | Purpose |
|------|---------|
| `agents/overlay/ollama-gemma3n.yaml` | Ollama integration overlay |
| `agents/document-transformer-agent.yaml` | Fixed YAML parsing (Lua strings) |
| `docs/architecture/tea-conformance-model-comparison.md` | Model comparison report |
| `cli/test-conformance/tea-test-input.json` | Test input document |

## Next Steps (When Resuming)

1. **Implement hybrid pipeline in Rust SDK**
   - Add sed/regex handler for pattern-based fixes
   - Add LLM surgical handler for semantic fixes
   - Wire into conformance write handler

2. **Fix TEA conditional routing**
   - Investigate why `goto` conditions always fall through
   - Test with explicit boolean comparisons

3. **Test full pipeline end-to-end**
   - Mount FUSE with TEA conformance enabled
   - Write near-conformant document
   - Verify surgical fix applied correctly

## Test Commands (For Reference)

```bash
# Pull models
ollama pull gemma3n:e4b
ollama pull qwen3:8b

# Test surgical replacement
curl -s http://localhost:11434/api/generate -d '{
  "model": "gemma3n:e4b",
  "prompt": "Replace ONLY the invalid value with valid option from [Draft, Approved, InProgress, Review, Done]:\n\n| **Status** | **** Done **** |\n\nOutput only the corrected line:",
  "stream": false,
  "options": {"temperature": 0.0, "num_predict": 50}
}' | jq -r '.response'
```

## Conclusion

**Surgical field replacement is superior to full document regeneration** for near-conformant documents. The hybrid approach (sed for patterns + LLM for semantics) provides the best balance of speed, reliability, and accuracy.
