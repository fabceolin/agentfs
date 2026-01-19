# TEA Conformance Transformation Prompt Example

This document shows an example of the structured prompt sent to the LLM when transforming a non-conformant document.

## Source Data (EnhancedConformanceResult)

The prompt is constructed from the `EnhancedConformanceResult` JSON passed to TEA:

```json
{
  "document": {
    "title": "STORY-1.1: User Authentication",
    "sections": [
      { "section_type": "heading", "level": 2, "content": "Status" },
      { "section_type": "paragraph", "content": "WIP" },
      { "section_type": "heading", "level": 2, "content": "Story" },
      { "section_type": "paragraph", "content": "As a user, I want to login so that I can access my account." },
      { "section_type": "heading", "level": 2, "content": "Tasks" },
      { "section_type": "paragraph", "content": "- Implement login form\n- Add validation\n- Connect to API" },
      { "section_type": "heading", "level": 2, "content": "Random Notes" },
      { "section_type": "paragraph", "content": "Some developer notes here..." }
    ]
  },
  "template": {
    "sections": [
      { "section_type": "heading", "level": 2, "content": "Status" },
      { "section_type": "heading", "level": 2, "content": "Story" },
      { "section_type": "heading", "level": 2, "content": "Acceptance Criteria" },
      { "section_type": "heading", "level": 2, "content": "Tasks" },
      { "section_type": "heading", "level": 2, "content": "QA Results" }
    ]
  },
  "conformance": {
    "file_path": "docs/stories/STORY-1.1-user-authentication.md",
    "template_id": "story-template-v2",
    "is_conformant": false,
    "missing_sections": [
      { "section_id": "acceptance-criteria", "section_title": "Acceptance Criteria", "is_required": true },
      { "section_id": "qa-results", "section_title": "QA Results", "is_required": false }
    ],
    "type_violations": [
      { "section_id": "tasks", "section_title": "Tasks", "expected_type": "checklist", "actual_content": "- Implement login form...", "suggestion": "Use checklist format" }
    ],
    "choice_violations": [
      { "section_id": "status", "section_title": "Status", "expected_choices": ["Draft", "Approved", "InProgress", "Review", "Done"], "actual_value": "WIP" }
    ],
    "extra_sections": ["Random Notes"],
    "suggestions": [
      { "kind": "add_section", "description": "Add required section", "auto_fixable": true }
    ]
  }
}
```

## Final Prompt Sent to LLM

The Lua nodes in `document-transformer-agent.yaml` build a structured prompt with five sections:

---

```
### SYSTEM INSTRUCTION ###
You are a strict Document Transformation Engine. Your goal is to refactor the INPUT_TEXT into the TARGET_STRUCTURE following the BUSINESS_RULES exactly.

CRITICAL CONSTRAINTS:
- Output ONLY the final Markdown document
- No explanations, no comments, no code block markers (```)
- No conversational text before or after the document
- Start directly with the document title (# Title)

### BUSINESS_RULES ###
1. **Fidelity**: Preserve ALL original textual content exactly. Do not summarize, paraphrase, or omit any text.
2. **Status Mapping**: The value 'WIP' is invalid. Map it to the closest valid option from: [Draft, Approved, InProgress, Review, Done].
3. **Tasks Formatting**: Convert to Markdown checklist format: '- [ ] Item'
4. **Missing Required Sections**: Create these sections with placeholder '[TODO: Add content]': Acceptance Criteria
5. **Missing Optional Sections**: Create these sections with placeholder '[Optional: Add if needed]': QA Results
6. **Extra Sections**: The following sections are not in the template. Move them to the end under '## Additional Notes': Random Notes
7. **Section Order**: Maintain the exact order of sections as shown in TARGET_STRUCTURE.
8. **Output Format**: Output ONLY the raw Markdown. No preamble, no summary, no code fences.

### TARGET_STRUCTURE ###
```
# [Document Title]

## Status
[Content]

## Story
[Content]

## Acceptance Criteria
[Content]

## Tasks
[Content]

## QA Results
[Content]

## Additional Notes
[Moved extra content here]

```

### INPUT_TEXT ###
```markdown
# STORY-1.1: User Authentication

## Status
WIP

## Story
As a user, I want to login so that I can access my account.

## Tasks
- Implement login form
- Add validation
- Connect to API

## Random Notes
Some developer notes here...

```

### OUTPUT ###
Transform the INPUT_TEXT now. Output only the Markdown document:
```

---

## Expected LLM Response

The LLM should output the transformed markdown document:

```markdown
# STORY-1.1: User Authentication

## Status
InProgress

## Story
As a user, I want to login so that I can access my account.

## Acceptance Criteria
[TODO: Add content]

## Tasks
- [ ] Implement login form
- [ ] Add validation
- [ ] Connect to API

## QA Results
[Optional: Add if needed]

## Additional Notes
Some developer notes here...
```

## Prompt Construction Flow (STORY-7.5 Enhanced)

```
┌─────────────────────────────┐
│  EnhancedConformanceResult  │
│  (from Rust SDK)            │
│  + document JSON            │
│  + template JSON            │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  TEA: analyze_gaps          │
│  (Lua node)                 │
│  - Separates required       │
│    vs optional sections     │
│  - Extracts auto_fixable    │
│  - Categorizes violations   │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  TEA: build_system_instruction │
│  (Lua node)                 │
│  - Role definition          │
│  - Critical constraints     │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  TEA: build_business_rules  │
│  (Lua node)                 │
│  - Dynamic rules from       │
│    conformance violations   │
│  - Choice mappings          │
│  - Type formatting rules    │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  TEA: build_target_structure│
│  (Lua node)                 │
│  - Template visualization   │
│  - Section order            │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  TEA: build_input_text      │
│  (Lua node)                 │
│  - Current document content │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  TEA: assemble_prompt       │
│  (Lua node)                 │
│  - Combines all sections    │
│  - Adds OUTPUT instruction  │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  llm.chat                   │
│  - Sends to local LLM       │
│  - max_tokens: 4096         │
│  - temperature: 0.1         │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  TEA: extract_transformed   │
│  (Lua node)                 │
│  - Remove code fences       │
│  - Trim preamble            │
│  - Clean whitespace         │
└───────────┬─────────────────┘
            │
            ▼
┌─────────────────────────────┐
│  Transformed Document       │
│  (clean markdown output)    │
└─────────────────────────────┘
```

## Key Benefits of Enhanced Conformance Data

| Data Field | BUSINESS_RULES Usage | LLM Benefit |
|------------|----------------------|-------------|
| `is_required` | Separate rules for required vs optional sections | Prioritizes critical sections with `[TODO:]` vs `[Optional:]` placeholders |
| `expected_type` | Dynamic formatting rule (e.g., "Convert to checklist format") | Knows exact format needed with examples |
| `expected_choices` | Status Mapping rule with valid options list | Can pick closest match from enumerated values |
| `extra_sections` | Rule to move to "Additional Notes" section | Preserves content while maintaining structure |
| `auto_fixable` | Used by Lua for pre-processing | Reduces LLM workload by handling simple fixes |

## Prompt Structure Benefits

| Section | Purpose | Why It Works |
|---------|---------|--------------|
| `SYSTEM INSTRUCTION` | Role definition + constraints | Clear persona and output expectations |
| `BUSINESS_RULES` | Dynamic rules from violations | Only includes relevant rules, not boilerplate |
| `TARGET_STRUCTURE` | Visual template reference | Shows exact section order and structure |
| `INPUT_TEXT` | Document to transform | Clean markdown with clear boundaries |
| `OUTPUT` | Final instruction | Reinforces output-only requirement |

## Related Files

- `agents/document-transformer-agent.yaml` - TEA agent definition with 8 Lua nodes
- `sdk/rust/src/graphdocs/agent_transformer.rs` - Rust bridge to TEA, `transform_to_template_enhanced()`
- `sdk/rust/src/graphdocs/conformance.rs` - Conformance types with serde derives
- `cli/src/handler.rs` - Handler that invokes transformation via `run_conformance_pipeline()`

## References

- STORY-7.5: Enhanced Conformance Reporting for TEA Agents
