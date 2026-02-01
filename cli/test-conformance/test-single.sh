#!/bin/bash
# test-single.sh - Test single file conformance without FUSE
# Uses agentfs graph-docs commands directly

set -euo pipefail

AGENTFS_CLI="${AGENTFS_CLI:-/home/fabricio/src/agentfs/cli/target/release/agentfs}"
TEMPLATE="${TEMPLATE:-/home/fabricio/src/the_edge_agent/.bmad-core/templates/story-tmpl.yaml}"
AGENTS_DIR="${AGENTS_DIR:-/home/fabricio/src/agentfs/agents}"
OVERLAY="${OVERLAY:-$AGENTS_DIR/overlay/claude-transformer.yaml}"

if [ $# -lt 1 ]; then
    echo "Usage: $0 <markdown-file> [--check-only]"
    echo ""
    echo "Options:"
    echo "  --check-only    Only check conformance, don't transform"
    exit 1
fi

FILE="$1"
CHECK_ONLY="${2:-}"

if [ ! -f "$FILE" ]; then
    echo "Error: File not found: $FILE"
    exit 1
fi

# Create temp directory with the file and template
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

cp "$FILE" "$TMPDIR/"
cp "$TEMPLATE" "$TMPDIR/"

echo "=== Testing: $(basename "$FILE") ==="
echo ""

# Check conformance using conformance-report (STORY-7.5)
echo "--- Conformance Check ---"
"$AGENTFS_CLI" graph-docs :memory: conformance-report --template "$TEMPLATE" --json "$FILE" 2>&1 || true
echo ""

if [ "$CHECK_ONLY" = "--check-only" ]; then
    exit 0
fi

# Transform with Claude (uses TEA agents)
echo "--- Conformance Transform ---"
"$AGENTFS_CLI" graph-docs :memory: conform \
    --agents-dir "$AGENTS_DIR" \
    --overlay "$OVERLAY" \
    "$TMPDIR" 2>&1

echo ""
echo "--- Transformed content ---"
if [ -f "$TMPDIR/$(basename "$FILE")" ]; then
    head -50 "$TMPDIR/$(basename "$FILE")"
else
    echo "(no changes)"
fi
