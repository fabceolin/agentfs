# Story: Migrate TypeScript and Python SDKs to markdownfs naming

## Status

Approved (Future)

## Story

**As a** developer integrating markdownfs into TypeScript or Python applications,
**I want** the SDKs renamed from `agentfs-sdk` to `markdownfs-sdk` with consistent naming conventions,
**so that** the SDK names align with the CLI and project identity.

## Story Context

**Existing System Integration:**
- Integrates with: npm registry (TypeScript), PyPI (Python), existing examples
- Technology: TypeScript/Node.js, Python
- Follows pattern: Standard npm/PyPI package naming
- Touch points: package.json, pyproject.toml, import statements, examples

**Rationale:**
After CLI and SDK Rust are renamed (STORY-RENAME-CLI), the TypeScript and Python SDKs need to follow suit for consistency across the entire project.

**Dependencies:**
- **Blocked by:** STORY-RENAME-CLI must be completed first
- Data directory convention (`.markdownfs/`) established by STORY-RENAME-CLI

## Acceptance Criteria

### TypeScript SDK Requirements

1. Package name changed from `agentfs-sdk` to `markdownfs-sdk` in package.json
2. All source code references to `agentfs` updated to `markdownfs`
3. Default data directory is `.markdownfs/` instead of `.agentfs/`
4. All TypeScript tests pass
5. Export names updated if they reference `agentfs`

### Python SDK Requirements

6. Package name changed from `agentfs-sdk` to `markdownfs-sdk` in pyproject.toml
7. Module directory renamed from `agentfs_sdk/` to `markdownfs_sdk/`
8. All source code references to `agentfs` updated to `markdownfs`
9. Default data directory is `.markdownfs/` instead of `.agentfs/`
10. All Python tests pass

### Examples Requirements

11. All examples in `examples/` updated to use new SDK names
12. Import statements updated (`agentfs-sdk` → `markdownfs-sdk`)
13. Examples still function correctly after migration

### Documentation Requirements

14. SDK READMEs updated with new naming
15. Any API documentation updated

## Tasks / Subtasks

### TypeScript SDK Tasks

- [ ] **Task 1: Update TypeScript package.json**
  - [ ] Change `name` from `agentfs-sdk` to `markdownfs-sdk`
  - [ ] Update `description`
  - [ ] Update `repository` URL if changed

- [ ] **Task 2: Update TypeScript Source Code**
  - [ ] Update `index_node.ts`: `.agentfs` → `.markdownfs`
  - [ ] Update `index_browser.ts` if applicable
  - [ ] Search and replace `agentfs` references in all `.ts` files
  - [ ] Update any class/type names if they reference `agentfs`

- [ ] **Task 3: Update TypeScript Tests**
  - [ ] Run `npm test` and fix failures
  - [ ] Update test fixtures if they reference `.agentfs`

- [ ] **Task 4: Update TypeScript Examples**
  - [ ] Update `sdk/typescript/examples/filesystem/package.json`
  - [ ] Update `sdk/typescript/examples/kvstore/package.json`
  - [ ] Update import statements in example code

### Python SDK Tasks

- [ ] **Task 5: Rename Python Module Directory**
  - [ ] Rename `sdk/python/agentfs_sdk/` → `sdk/python/markdownfs_sdk/`
  - [ ] Update `pyproject.toml` package discovery

- [ ] **Task 6: Update Python pyproject.toml**
  - [ ] Change `name` from `agentfs-sdk` to `markdownfs-sdk`
  - [ ] Update `description`
  - [ ] Update URLs

- [ ] **Task 7: Update Python Source Code**
  - [ ] Update `__init__.py` with new module name
  - [ ] Update `agentfs.py` → rename if needed
  - [ ] Search and replace `agentfs` references
  - [ ] Update `.agentfs` → `.markdownfs` directory references

- [ ] **Task 8: Update Python Tests**
  - [ ] Update import statements in tests
  - [ ] Run `uv run pytest` and fix failures

### Examples Directory Tasks

- [ ] **Task 9: Update Main Examples**
  - [ ] `examples/claude-agent/research-assistant/package.json`
  - [ ] `examples/cloudflare/package.json`
  - [ ] `examples/ai-sdk-just-bash/package.json`
  - [ ] `examples/mastra/research-assistant/package.json`
  - [ ] `examples/openai-agents/research-assistant/package.json`

- [ ] **Task 10: Update Example Source Code**
  - [ ] Search for `agentfs` imports in all example files
  - [ ] Update to `markdownfs-sdk` imports

### Documentation Tasks

- [ ] **Task 11: Update SDK Documentation**
  - [ ] Update TypeScript SDK README
  - [ ] Update Python SDK README
  - [ ] Update any integration guides

## Dev Notes

### Relevant Source Tree

```
sdk/
├── typescript/
│   ├── package.json                    # name: agentfs-sdk → markdownfs-sdk
│   ├── src/
│   │   ├── index_node.ts               # .agentfs → .markdownfs
│   │   ├── index_browser.ts
│   │   ├── agentfs.ts                  # May need rename
│   │   ├── filesystem/
│   │   ├── kvstore.ts
│   │   └── toolcalls.ts
│   └── examples/
│       ├── filesystem/package.json
│       └── kvstore/package.json
│
├── python/
│   ├── pyproject.toml                  # name: agentfs-sdk → markdownfs-sdk
│   ├── agentfs_sdk/                    # Rename to markdownfs_sdk/
│   │   ├── __init__.py
│   │   ├── agentfs.py
│   │   ├── filesystem.py
│   │   ├── kvstore.py
│   │   └── toolcalls.py
│   └── tests/

examples/
├── claude-agent/research-assistant/
├── cloudflare/
├── ai-sdk-just-bash/
├── mastra/research-assistant/
└── openai-agents/research-assistant/
```

### Key TypeScript Changes

```typescript
// BEFORE (index_node.ts line 47-48)
const dir = '.agentfs';
dbPath = `${dir}/${id}.db`;

// AFTER
const dir = '.markdownfs';
dbPath = `${dir}/${id}.db`;
```

### Key Python Changes

```python
# BEFORE
from agentfs_sdk import AgentFS

# AFTER
from markdownfs_sdk import AgentFS  # or MarkdownFS if class renamed
```

### Testing

- **TypeScript:** `cd sdk/typescript && npm test`
- **Python:** `cd sdk/python && uv run pytest`
- **Examples:** Manual verification that examples still work

## Risk Assessment

**Primary Risk:** Breaking existing external users of the SDKs
**Mitigation:**
- SDKs are not currently published to npm/PyPI
- This is internal project rename, not public API change
- If published later, use new name from start

**Secondary Risk:** Import statement changes in many files
**Mitigation:** Use find-and-replace with verification

## Definition of Done

- [ ] TypeScript SDK package name is `markdownfs-sdk`
- [ ] Python SDK package name is `markdownfs-sdk`
- [ ] Python module directory is `markdownfs_sdk/`
- [ ] All SDK tests pass
- [ ] All examples updated and functional
- [ ] No `agentfs` references remain (except git history)

## Change Log

| Date | Version | Description | Author |
|------|---------|-------------|--------|
| 2026-01-16 | 0.1 | Initial story draft (future work) | Sarah (PO Agent) |
