# Manual Testing: TEA Document Conformance

This guide covers how to manually test the document conformance workflow using TEA (The Edge Agent) with local LLM inference.

## Overview

The conformance workflow:
1. Add a markdown file via FUSE mount (or directly to filesystem)
2. Check/fix document conformance against a template using TEA agents
3. Verify the document is transformed to match the template structure

## Prerequisites

### 1. System Requirements

- Linux system with FUSE support
- Built CLI: `cargo build --release` from `cli/` directory
- TEA binary with Lua support (`tea-rust` recommended)

### 2. TEA Runtime

**Option A: tea-rust (recommended)**
```bash
# tea-rust has native Lua support and bundled LLM
~/bin/tea-rust --version
```

**Option B: tea-python with external model**
```bash
# tea-python requires --gguf for local LLM
~/bin/tea-python --version
```

### 3. GGUF Model (if using external model)

Download a GGUF model for local LLM inference:

```bash
# Create models directory
mkdir -p ~/.cache/tea/models

# Download Gemma 3 4B (recommended, ~2.4GB)
wget -c "https://huggingface.co/ggml-org/gemma-3-4b-it-GGUF/resolve/main/gemma-3-4b-it-Q4_K_M.gguf" \
  -O ~/.cache/tea/models/gemma-3-4b-it-Q4_K_M.gguf
```

### 4. TEA Agent Configuration

Ensure agent YAML files exist:

```bash
ls -la agents/
# Should show:
#   document-conformance-agent.yaml
#   document-transformer-agent.yaml
```

## Test Procedure

### Step 1: Create Test Directory

```bash
mkdir -p /tmp/conform-test
```

### Step 2: Create a Near-Conformant File

Create a markdown file that partially matches the story template:

```bash
cat > /tmp/conform-test/story-dark-mode.md << 'EOF'
# STORY-001: Add Dark Mode

## Status
Draft

## Description
Add dark mode toggle to the application settings.

## Acceptance Criteria
- [ ] User can toggle dark mode
- [ ] Preference is saved
EOF
```

**Note:** This file is missing sections required by the story template:
- Story (As a... I want... so that...)
- Tasks / Subtasks
- Dev Notes
- Change Log
- Dev Agent Record

### Step 3: Copy Template to Test Directory

```bash
cp .bmad-core/templates/story-tmpl.yaml /tmp/conform-test/
```

### Step 4: Test Conformance Agent Directly (Optional)

Verify the TEA agent works before running the full workflow:

```bash
# Using tea-rust (has bundled model)
~/bin/tea-rust run agents/document-conformance-agent.yaml \
  --input '{"raw_status": "[**Done**]"}'

# Or using tea-python with external model
~/bin/tea-python run agents/document-conformance-agent.yaml \
  --input '{"raw_status": "[**Done**]"}' \
  --gguf ~/.cache/tea/models/gemma-3-4b-it-Q4_K_M.gguf
```

**Expected output:**
```json
{
  "raw_status": "[**Done**]",
  "normalized_status": "Done"
}
```

### Step 5: Run Conformance Check (Dry Run)

```bash
cd cli

./target/release/agentfs graph-docs demo/demo.duckdb conform \
  --dry-run \
  --agents-dir ../agents \
  --model-path ~/.cache/tea/models/gemma-3-4b-it-Q4_K_M.gguf \
  /tmp/conform-test
```

**Expected output:**
```
Scanning directory: "/tmp/conform-test"
(dry-run mode - no files will be modified)

[DRY-RUN] Would transform: /tmp/conform-test/story-dark-mode.md (X issues)
--- Preview ---
# STORY-001: Add Dark Mode
...
--- End Preview ---

Total: 1 files processed
```

### Step 6: Run Conformance Fix (Apply Changes)

```bash
./target/release/agentfs graph-docs demo/demo.duckdb conform \
  --agents-dir ../agents \
  --model-path ~/.cache/tea/models/gemma-3-4b-it-Q4_K_M.gguf \
  /tmp/conform-test
```

**Expected output:**
```
Scanning directory: "/tmp/conform-test"
Transformed: /tmp/conform-test/story-dark-mode.md (X issues fixed)

Total: 1 files processed
```

### Step 7: Verify Transformed Document

```bash
cat /tmp/conform-test/story-dark-mode.md
```

**Expected:** Document should now include all template sections.

## Alternative: FUSE + Conformance Workflow

### Step 1: Mount Database

```bash
mkdir -p /tmp/agentfs-mount
./target/release/agentfs mount demo/demo.duckdb /tmp/agentfs-mount --foreground &
sleep 3
```

### Step 2: Create File via FUSE

```bash
cat > /tmp/agentfs-mount/story-dark-mode.md << 'EOF'
# STORY-001: Add Dark Mode

## Status
Draft

## Description
Add dark mode toggle to the application settings.
EOF
```

### Step 3: Verify File Was Written

```bash
cat /tmp/agentfs-mount/story-dark-mode.md
ls -la /tmp/agentfs-mount/story-dark-mode.md
```

### Step 4: Run Conformance on Mounted Directory

```bash
./target/release/agentfs graph-docs demo/demo.duckdb conform \
  --dry-run \
  --agents-dir ../agents \
  --model-path ~/.cache/tea/models/gemma-3-4b-it-Q4_K_M.gguf \
  /tmp/agentfs-mount
```

### Step 5: Cleanup

```bash
fusermount -u /tmp/agentfs-mount
```

## Testing Different Status Values

Test the conformance agent's ability to normalize various status formats:

```bash
# Test various status inputs
for status in "In Progress" "**WIP**" "[Draft]" "ready for review" "APPROVED" "[**Done**]"; do
  echo "--- Input: '$status' ---"
  ~/bin/tea-rust run agents/document-conformance-agent.yaml \
    --input "{\"raw_status\": \"$status\"}" 2>&1 | grep -A1 "normalized_status"
done
```

**Expected mappings:**
| Input | Normalized |
|-------|------------|
| `In Progress` | `InProgress` |
| `**WIP**` | `InProgress` |
| `[Draft]` | `Draft` |
| `ready for review` | `Review` |
| `APPROVED` | `Approved` |
| `[**Done**]` | `Done` |

## Troubleshooting

### "lupa not found" Error (tea-python)

The tea-python AppImage may be missing the lupa (Lua) library. Use tea-rust instead:

```bash
# tea-rust has native Lua support
~/bin/tea-rust run agents/document-conformance-agent.yaml --input '...'
```

### "Model not found" Error

Ensure the GGUF model exists:

```bash
ls -lh ~/.cache/tea/models/
```

If missing, download it (see Prerequisites section).

### YAML Parse Errors

Check agent YAML syntax:

```bash
~/bin/tea-rust validate agents/document-conformance-agent.yaml
```

### LLM Output Truncation

If transformed documents are truncated, increase `max_tokens` in the agent YAML:

```yaml
- name: transform_with_llm
  uses: llm.chat
  with:
    max_tokens: 4096  # Increase from default
```

### Debug Logging

Run with verbose output:

```bash
~/bin/tea-rust run agents/document-conformance-agent.yaml \
  --input '{"raw_status": "Done"}' \
  -vvv
```

## Verification Checklist

- [ ] TEA runtime is installed (`tea-rust` or `tea-python`)
- [ ] GGUF model downloaded (if using external model)
- [ ] Agent YAML files exist in `agents/` directory
- [ ] Conformance agent correctly classifies status values
- [ ] `graph-docs conform --dry-run` shows issues detected
- [ ] `graph-docs conform` transforms documents
- [ ] Transformed documents include all template sections
- [ ] FUSE write-then-read works (fs_current fix verified)

## Agent Configuration Reference

### document-conformance-agent.yaml

Classifies status text into canonical values using local LLM.

**Input state:**
```json
{"raw_status": "[**Done**]"}
```

**Output state:**
```json
{"raw_status": "[**Done**]", "normalized_status": "Done"}
```

### document-transformer-agent.yaml

Transforms documents to match template structure using local LLM.

**Input state:**
```json
{
  "document": {"title": "...", "sections": [...]},
  "template": {"sections": [...]},
  "conformance": {"missing_sections": [...]}
}
```

**Output state:**
```json
{
  "output_content": "# Transformed markdown...",
  "needs_transform": true
}
```

## Related Documentation

- [GraphDocs FUSE Mount Testing](manual-test-graphdocs-fuse.md)
- [TEA Agent YAML Reference](https://github.com/anthropics/the-edge-agent/docs)
- [STORY-BUG-001: fs_current View Fix](../../docs/stories/STORY-BUG-fs-current-view-coalesce.md)
