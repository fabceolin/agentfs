#!/bin/bash
# test-harness.sh - Mass Conformance Validation Test Harness
# STORY-8.1: Validates TEA document transformation pipeline against corpus

set -euo pipefail

# Configuration
AGENTFS_CLI="${AGENTFS_CLI:-/home/fabricio/src/agentfs/cli/target/release/agentfs}"
AGENTS_DIR="${AGENTS_DIR:-/home/fabricio/src/agentfs/agents}"
OVERLAY="${OVERLAY:-$AGENTS_DIR/overlay/claude-transformer.yaml}"
CORPUS_DIR="${CORPUS_DIR:-/home/fabricio/src/the_edge_agent/docs/stories}"
RESULTS_DIR="${RESULTS_DIR:-/tmp/conformance-results}"
DB_NAME="${DB_NAME:-mass-test}"
MOUNT_POINT="${MOUNT_POINT:-$RESULTS_DIR/mnt}"
BATCH_SIZE="${BATCH_SIZE:-10}"
DELAY_BETWEEN_FILES="${DELAY_BETWEEN_FILES:-2}"
CONFORMANCE_TIMEOUT="${CONFORMANCE_TIMEOUT:-30}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[PASS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[FAIL]${NC} $1"; }

cleanup() {
    log_info "Cleaning up..."
    fusermount -u "$MOUNT_POINT" 2>/dev/null || true
    sleep 1
    rmdir "$MOUNT_POINT" 2>/dev/null || true
}

trap cleanup EXIT

usage() {
    echo "Usage: $0 [OPTIONS] [COMMAND]"
    echo ""
    echo "Commands:"
    echo "  smoke       Run 10-file smoke test"
    echo "  stress      Run 100-file stress test"
    echo "  full        Run all 345 files"
    echo "  single FILE Test a single file"
    echo "  sample N    Test N random files"
    echo ""
    echo "Options:"
    echo "  -r, --results DIR    Results directory (default: $RESULTS_DIR)"
    echo "  -b, --batch SIZE     Batch size (default: $BATCH_SIZE)"
    echo "  -d, --delay SECS     Delay between files (default: $DELAY_BETWEEN_FILES)"
    echo "  -t, --timeout SECS   Conformance timeout (default: $CONFORMANCE_TIMEOUT)"
    echo "  -h, --help           Show this help"
    exit 0
}

setup_environment() {
    log_info "Setting up environment..."

    # Create directories
    mkdir -p "$RESULTS_DIR"
    mkdir -p "$MOUNT_POINT"
    mkdir -p "$RESULTS_DIR/reports"

    # Initialize database if needed
    cd "$RESULTS_DIR"
    if [ ! -f ".agentfs/${DB_NAME}.duckdb" ]; then
        log_info "Creating DuckDB database: $DB_NAME"
        "$AGENTFS_CLI" duckdb init "$DB_NAME" --force
    fi

    # Copy template
    mkdir -p "$MOUNT_POINT"

    log_success "Environment ready"
}

start_fuse_mount() {
    log_info "Starting FUSE mount with TEA conformance..."

    cd "$RESULTS_DIR"

    # Start mount in background
    "$AGENTFS_CLI" mount "$DB_NAME" "$MOUNT_POINT" \
        --tea-conformance \
        --tea-agents-dir "$AGENTS_DIR" \
        --tea-overlay "$OVERLAY" \
        --foreground &

    MOUNT_PID=$!

    # Wait for mount to be ready
    sleep 2

    if ! mountpoint -q "$MOUNT_POINT"; then
        log_error "FUSE mount failed"
        exit 1
    fi

    # Create stories directory and copy template
    mkdir -p "$MOUNT_POINT/stories"
    cp "$AGENTS_DIR/../cli/test-conformance/stories/story-tmpl.yaml" "$MOUNT_POINT/stories/" 2>/dev/null || \
    cp "/home/fabricio/src/the_edge_agent/.bmad-core/templates/story-tmpl.yaml" "$MOUNT_POINT/stories/"

    log_success "FUSE mount ready at $MOUNT_POINT (PID: $MOUNT_PID)"
}

test_single_file() {
    local src_file="$1"
    local filename=$(basename "$src_file")
    local dest_file="$MOUNT_POINT/stories/$filename"
    local report_file="$RESULTS_DIR/reports/${filename%.md}.json"

    local start_time=$(date +%s.%N)

    # Copy file to trigger conformance
    cp "$src_file" "$dest_file"

    # Wait for conformance processing
    local conformant_file="${dest_file}.conformant"
    local failed_file="${dest_file}.failed.json"
    local elapsed=0

    while [ $elapsed -lt $CONFORMANCE_TIMEOUT ]; do
        if [ -f "$conformant_file" ]; then
            local end_time=$(date +%s.%N)
            local duration=$(echo "$end_time - $start_time" | bc)

            # Generate report
            cat > "$report_file" <<EOF
{
  "filename": "$filename",
  "status": "conformant",
  "duration_secs": $duration,
  "source_path": "$src_file",
  "conformant_path": "$conformant_file",
  "timestamp": "$(date -Iseconds)"
}
EOF
            log_success "$filename (${duration}s)"
            echo "conformant"
            return 0
        fi

        if [ -f "$failed_file" ]; then
            local end_time=$(date +%s.%N)
            local duration=$(echo "$end_time - $start_time" | bc)
            local error_msg=$(cat "$failed_file" 2>/dev/null || echo "unknown error")

            cat > "$report_file" <<EOF
{
  "filename": "$filename",
  "status": "failed",
  "duration_secs": $duration,
  "source_path": "$src_file",
  "error": $error_msg,
  "timestamp": "$(date -Iseconds)"
}
EOF
            log_error "$filename (${duration}s) - transformation failed"
            echo "failed"
            return 1
        fi

        sleep 1
        elapsed=$((elapsed + 1))
    done

    # Timeout
    local end_time=$(date +%s.%N)
    local duration=$(echo "$end_time - $start_time" | bc)

    cat > "$report_file" <<EOF
{
  "filename": "$filename",
  "status": "timeout",
  "duration_secs": $duration,
  "source_path": "$src_file",
  "timeout_secs": $CONFORMANCE_TIMEOUT,
  "timestamp": "$(date -Iseconds)"
}
EOF
    log_warn "$filename - timeout after ${CONFORMANCE_TIMEOUT}s"
    echo "timeout"
    return 2
}

run_batch_test() {
    local files=("$@")
    local total=${#files[@]}
    local conformant=0
    local failed=0
    local timeout=0
    local start_time=$(date +%s)

    log_info "Testing $total files..."
    echo ""

    for i in "${!files[@]}"; do
        local file="${files[$i]}"
        local progress=$((i + 1))
        echo -n "[$progress/$total] "

        local result=$(test_single_file "$file")

        case "$result" in
            conformant) ((conformant++)) ;;
            failed) ((failed++)) ;;
            timeout) ((timeout++)) ;;
        esac

        # Delay between files to avoid overwhelming Claude API
        if [ $progress -lt $total ]; then
            sleep "$DELAY_BETWEEN_FILES"
        fi
    done

    local end_time=$(date +%s)
    local total_duration=$((end_time - start_time))

    # Generate summary
    echo ""
    log_info "========== RESULTS =========="
    echo "Total files:    $total"
    echo "Conformant:     $conformant ($(echo "scale=1; $conformant * 100 / $total" | bc)%)"
    echo "Failed:         $failed ($(echo "scale=1; $failed * 100 / $total" | bc)%)"
    echo "Timeout:        $timeout ($(echo "scale=1; $timeout * 100 / $total" | bc)%)"
    echo "Total duration: ${total_duration}s"
    echo "Avg per file:   $(echo "scale=2; $total_duration / $total" | bc)s"
    echo ""

    # Write summary to JSONL
    local summary_file="$RESULTS_DIR/summary-$(date +%Y%m%d-%H%M%S).json"
    cat > "$summary_file" <<EOF
{
  "test_type": "batch",
  "total_files": $total,
  "conformant": $conformant,
  "failed": $failed,
  "timeout": $timeout,
  "conformance_rate": $(echo "scale=4; $conformant / $total" | bc),
  "total_duration_secs": $total_duration,
  "avg_duration_secs": $(echo "scale=2; $total_duration / $total" | bc),
  "timestamp": "$(date -Iseconds)"
}
EOF
    log_success "Summary written to: $summary_file"

    # Combine reports to JSONL
    local jsonl_file="$RESULTS_DIR/reports.jsonl"
    > "$jsonl_file"
    for report in "$RESULTS_DIR/reports"/*.json; do
        if [ -f "$report" ]; then
            cat "$report" >> "$jsonl_file"
            echo "" >> "$jsonl_file"
        fi
    done
    log_success "Reports written to: $jsonl_file"
}

cmd_smoke() {
    log_info "Running SMOKE TEST (10 representative files)"

    setup_environment
    start_fuse_mount

    # Select 10 representative files from different categories
    local files=(
        "$CORPUS_DIR/TEA-RUST-014-library-api.md"
        "$CORPUS_DIR/TEA-CLI-005-interactive-hitl-mode.md"
        "$CORPUS_DIR/TD.2.add-future-import.md"
        "$CORPUS_DIR/YE.3.langgraph-interrupt-behavior.md"
        "$CORPUS_DIR/BUG.001.hierarchical-ltm-yaml-config-mismatch.md"
        "$CORPUS_DIR/DOC-001.consolidate-yaml-docs.md"
        "$CORPUS_DIR/RUST.001.fix-goto-state-timing.md"
        "$CORPUS_DIR/TD.10.checkpoint-persistence.md"
        "$CORPUS_DIR/DOC-002.1-structure-setup.md"
        "$CORPUS_DIR/TD.11.split-test-stategraph.md"
    )

    # Filter to existing files
    local existing_files=()
    for f in "${files[@]}"; do
        if [ -f "$f" ]; then
            existing_files+=("$f")
        else
            log_warn "File not found: $f"
        fi
    done

    if [ ${#existing_files[@]} -eq 0 ]; then
        # Fallback: grab first 10 files
        mapfile -t existing_files < <(find "$CORPUS_DIR" -maxdepth 1 -name "*.md" -type f | head -10)
    fi

    run_batch_test "${existing_files[@]}"
}

cmd_stress() {
    log_info "Running STRESS TEST (100 files)"

    setup_environment
    start_fuse_mount

    # Get 100 random files
    mapfile -t files < <(find "$CORPUS_DIR" -maxdepth 1 -name "*.md" -type f | shuf | head -100)

    run_batch_test "${files[@]}"
}

cmd_full() {
    log_info "Running FULL TEST (all 345 files)"

    setup_environment
    start_fuse_mount

    # Get all files
    mapfile -t files < <(find "$CORPUS_DIR" -maxdepth 1 -name "*.md" -type f | sort)

    run_batch_test "${files[@]}"
}

cmd_single() {
    local file="$1"

    if [ ! -f "$file" ]; then
        log_error "File not found: $file"
        exit 1
    fi

    setup_environment
    start_fuse_mount

    test_single_file "$file"
}

cmd_sample() {
    local count="${1:-10}"

    log_info "Running SAMPLE TEST ($count random files)"

    setup_environment
    start_fuse_mount

    mapfile -t files < <(find "$CORPUS_DIR" -maxdepth 1 -name "*.md" -type f | shuf | head -"$count")

    run_batch_test "${files[@]}"
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -r|--results) RESULTS_DIR="$2"; shift 2 ;;
        -b|--batch) BATCH_SIZE="$2"; shift 2 ;;
        -d|--delay) DELAY_BETWEEN_FILES="$2"; shift 2 ;;
        -t|--timeout) CONFORMANCE_TIMEOUT="$2"; shift 2 ;;
        -h|--help) usage ;;
        smoke) cmd_smoke; exit 0 ;;
        stress) cmd_stress; exit 0 ;;
        full) cmd_full; exit 0 ;;
        single) cmd_single "$2"; exit 0 ;;
        sample) cmd_sample "$2"; exit 0 ;;
        *) log_error "Unknown option: $1"; usage ;;
    esac
done

usage
