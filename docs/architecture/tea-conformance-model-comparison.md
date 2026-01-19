# TEA Conformance Transformation - Model Comparison Report

**Date:** 2026-01-18
**Test:** Document conformance transformation using structured prompt

## Overview

This report documents the testing of the TEA document transformer agent with various LLM models to evaluate prompt effectiveness for document conformance transformation.

## Test Setup

### Original Document (Non-Conformant)

```markdown
# STORY-TEST: Login Feature

## Status
WIP

## Story
As a user, I want to login with my credentials so that I can access my dashboard.

## Tasks
- Create login form component
- Add form validation
- Implement API integration
- Add error handling

## Dev Notes
Remember to use the existing auth service. Check with backend team about token refresh.
```

### Conformance Violations Detected

| Violation Type | Details |
|----------------|---------|
| **choice_violation** | Status "WIP" not in [Draft, Approved, InProgress, Review, Done] |
| **missing_section** (required) | Acceptance Criteria |
| **missing_section** (optional) | QA Results |
| **type_violation** | Tasks should be checklist format `- [ ] Item` |
| **extra_section** | Dev Notes (not in template) |

### Expected Template Structure

```markdown
# [Document Title]
## Status
## Story
## Acceptance Criteria
## Tasks / Subtasks
## QA Results
## Additional Notes
```

## Prompt Structure

The agent uses a structured prompt with 5 sections:

1. **SYSTEM INSTRUCTION** - Role definition and constraints
2. **BUSINESS_RULES** - Dynamically generated from conformance violations
3. **TARGET_STRUCTURE** - Template visualization
4. **INPUT_TEXT** - Document to transform
5. **OUTPUT** - Final instruction

### Generated BUSINESS_RULES (Dynamic)

```
1. **Fidelity**: Preserve ALL original textual content exactly. Do not summarize, paraphrase, or omit any text.
2. **Status Mapping**: The value 'WIP' is invalid. Map it to the closest valid option from: [Draft, Approved, InProgress, Review, Done].
3. **Tasks / Subtasks Formatting**: Convert to Markdown checklist format: '- [ ] Item'
4. **Missing Required Sections**: Create these sections with placeholder '[TODO: Add content]': Acceptance Criteria
5. **Missing Optional Sections**: Create these sections with placeholder '[Optional: Add if needed]': QA Results
6. **Extra Sections**: The following sections are not in the template. Move them to the end under '## Additional Notes': Dev Notes
7. **Section Order**: Maintain the exact order of sections as shown in TARGET_STRUCTURE.
8. **Output Format**: Output ONLY the raw Markdown. No preamble, no summary, no code fences.
```

## Model Comparison Results

| Model | Size | Status Mapping | Acceptance Criteria | Tasks Format | Dev Notes | Overall |
|-------|------|----------------|---------------------|--------------|-----------|---------|
| **qwen3:8b** | 5.2GB | ✅ WIP → **InProgress** | ✅ [TODO: Add content] | ✅ - [ ] Item | ✅ Moved correctly | **PERFECT** |
| **gemma3n:e4b** | 7.5GB | ✅ WIP → Draft | ✅ [TODO: Add content] | ✅ - [ ] Item | ✅ Moved correctly | SUCCESS |
| gemma3:4b | 3.3GB | ✅ WIP → InProgress | ⚠️ Empty | ✅ - [ ] Item | ⚠️ Partial | PARTIAL |
| deepseek-r1:1.5b | 1.1GB | ❌ Not changed | ❌ Hallucinated | ❌ Not converted | ❌ Wrong | FAILED |

## Best Result: qwen3:8b

```markdown
# STORY-TEST: Login Feature

## Status
InProgress

## Story
As a user, I want to login with my credentials so that I can access my dashboard.

## Acceptance Criteria
[TODO: Add content]

## Tasks / Subtasks
- [ ] Create login form component
- [ ] Add form validation
- [ ] Implement API integration
- [ ] Add error handling

## QA Results
[Optional: Add if needed]

## Additional Notes
Remember to use the existing auth service. Check with backend team about token refresh.
```

### Why qwen3:8b is Best

1. **Correct semantic mapping**: "WIP" (Work In Progress) → "InProgress" (most accurate interpretation)
2. **Perfect placeholder usage**: Required sections get `[TODO: Add content]`, optional get `[Optional: Add if needed]`
3. **Correct checklist conversion**: All tasks converted to `- [ ]` format
4. **Proper section relocation**: Dev Notes moved to Additional Notes
5. **Clear reasoning**: Model shows step-by-step thinking process

## Detailed Model Analysis

### qwen3:8b (5.2GB) - PERFECT ✅

- **Strengths**: Best semantic understanding, perfect rule following, clear reasoning
- **Weaknesses**: None observed
- **Recommended for**: Production use

### gemma3n:e4b (7.5GB) - SUCCESS ✅

- **Strengths**: Good rule following, correct structure
- **Weaknesses**: Mapped "WIP" → "Draft" (less accurate than InProgress)
- **Minor issue**: Small typo in Story content ("dashboaard")
- **Recommended for**: Production use (alternative to qwen3:8b)

### gemma3:4b (3.3GB) - PARTIAL ⚠️

- **Strengths**: Correct status mapping, correct checklist format
- **Weaknesses**: Missing placeholder text for Acceptance Criteria, partial Dev Notes handling
- **Recommended for**: Development/testing only

### deepseek-r1:1.5b (1.1GB) - FAILED ❌

- **Strengths**: Shows reasoning process
- **Weaknesses**: Failed to follow most rules, hallucinated content
- **Not recommended**: Too small for this task

## Conclusions

### Prompt Effectiveness

1. **The prompt is GENERIC** - Works with any template structure, not just story templates
2. **Dynamic BUSINESS_RULES** - Rules are generated based on specific conformance violations
3. **Clear structure** - 5-section format provides clear context for LLM

### Model Size Recommendations

| Use Case | Minimum Model Size | Recommended Model |
|----------|-------------------|-------------------|
| Production | 5B+ parameters | qwen3:8b |
| Development | 4B+ parameters | gemma3:4b or gemma3n:e4b |
| Not suitable | <2B parameters | - |

### Known Issues

1. **TEA conditional routing**: The `goto` condition evaluation has a bug - always goes to passthrough even when `is_conformant == false`. This needs investigation in TEA's Rust implementation.

## Test Commands

### Using Ollama API directly

```bash
# Test with qwen3:8b (recommended)
curl -s http://localhost:11434/api/generate -d @request.json | jq -r '.response'

# Test with gemma3n:e4b
ollama run gemma3n:e4b "$(cat prompt.txt)"
```

### Using TEA Docker (when conditional routing is fixed)

```bash
docker run --rm \
  -v $(pwd)/agents:/agents:ro \
  ghcr.io/fabceolin/tea:rust-gemma3n-e4b \
  run /agents/document-transformer-agent.yaml \
  --input "$(cat test-input.json)"
```

## Related Files

- `agents/document-transformer-agent.yaml` - TEA agent definition
- `agents/overlay/ollama-gemma3n.yaml` - Ollama overlay configuration
- `docs/architecture/tea-conformance-prompt-example.md` - Detailed prompt example
- `cli/test-conformance/tea-test-input.json` - Test input data

## References

- STORY-7.5: Enhanced Conformance Reporting for TEA Agents
- TEA (The Edge Agent) documentation
