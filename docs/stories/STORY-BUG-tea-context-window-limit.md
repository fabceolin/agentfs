# Bug Report: TEA Default Context Window Too Small for gemma3n:e4b

| Field | Value |
|-------|-------|
| **Bug ID** | TEA-CTX-001 |
| **Project** | TEA (The Edge Agent) |
| **Component** | LLM Backend / llama-cpp integration |
| **Severity** | High |
| **Status** | Open |
| **Discovered** | 2026-01-18 |
| **Reporter** | AgentFS Team |

---

## Summary

TEA's default context window (`n_ctx`) is set to 4096 tokens, which is insufficient for modern models like `gemma3n:e4b` that support 32K context natively. This causes document transformation to fail with large documents.

## Environment

- **TEA Version**: 0.9.67 (tea-python)
- **Model**: gemma3n:e4b (gemma-3-1b-it-Q8_0)
- **Platform**: Linux x86_64

## Steps to Reproduce

1. Create a large document (~18K tokens, ~64KB markdown)
2. Run document transformation agent:
```bash
tea-python run document-transformer-agent.yaml --input @large-doc.json
```
3. Observe error in `transform_result`

## Expected Behavior

The model should process documents up to 32K tokens (native context window of gemma3n:e4b).

## Actual Behavior

```json
{
  "transform_result": {
    "error": "Requested tokens (18193) exceed context window of 4096",
    "success": false
  }
}
```

## Root Cause Analysis

### Model Specifications (gemma3n:e4b)

| Spec | Value |
|------|-------|
| **Native Context Window** | 32,768 tokens (32K) |
| **Architecture** | Mobile-first (NPU/CPU optimized) |
| **"E4B" meaning** | Effective 4 Billion parameters (compressed 8B sparse architecture) |

### TEA Configuration

The default `n_ctx: 4096` in `settings.llm` is a conservative default inherited from Ollama/llama.cpp safety defaults:

```yaml
# Current default in document-transformer-agent.yaml
settings:
  llm:
    backend: local
    n_ctx: 4096  # <-- Too small for gemma3n:e4b
```

## Proposed Fix

### Option 1: Increase default n_ctx for known models

TEA should detect model capabilities and set appropriate context window:

```yaml
# Model-aware defaults
settings:
  llm:
    backend: local
    n_ctx: auto  # Detect from model metadata, fallback to 8192
```

### Option 2: Expose CLI flag for context window

```bash
tea run agent.yaml --input '...' --n-ctx 32768
```

### Option 3: Allow YAML override per model

```yaml
settings:
  llm:
    backend: local
    model_overrides:
      gemma3n:
        n_ctx: 32768
      llama3.1:
        n_ctx: 131072
```

## Workaround

Currently, users must manually edit the agent YAML:

```yaml
settings:
  llm:
    backend: local
    n_ctx: 32768  # Override default
```

Or use the `--gguf` flag with explicit context:

```bash
tea run agent.yaml --gguf /path/to/model.gguf -C '{"llm": {"n_ctx": 32768}}'
```

## Impact

- Document transformation fails for any document > ~3K words
- Users must manually configure context window
- Default behavior doesn't utilize model capabilities

## References

- [Gemma 3n Model Card](https://ai.google.dev/gemma/docs/gemma3n)
- [Ollama Context Window Issue](https://github.com/ollama/ollama/issues/context-window)
- [llama.cpp n_ctx parameter](https://github.com/ggerganov/llama.cpp/blob/master/examples/main/README.md)

## Test Case

```bash
# Should succeed with 32K context
tea run document-transformer-agent.yaml \
  --input @/tmp/test-input.json \
  -C '{"llm": {"n_ctx": 32768}}'
```

## Related Files

| File | Description |
|------|-------------|
| `agents/document-transformer-agent.yaml` | Affected agent with n_ctx: 4096 |
| `agents/overlay/gemma3n-transformer.yaml` | Overlay that should set n_ctx: 32768 |

---

## Change Log

| Date | Description | Author |
|------|-------------|--------|
| 2026-01-18 | Bug discovered during conformance testing | AgentFS Team |
| 2026-01-18 | Bug report created | Quinn (Test Architect) |
