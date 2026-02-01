#!/bin/bash
# test-controlled.sh - Rate-limited conformance testing
# STORY-8.1: Mass Conformance Validation with 4 files/minute limit
#
# Usage: ./test-controlled.sh [start_index] [count]
#   start_index: Where to start in the sorted file list (default: 0)
#   count: How many files to process (default: 100 for stress test)

set -euo pipefail

# Configuration
AGENTFS_CLI="${AGENTFS_CLI:-/home/fabricio/src/agentfs/cli/target/release/agentfs}"
AGENTS_DIR="${AGENTS_DIR:-/home/fabricio/src/agentfs/agents}"
OVERLAY="${OVERLAY:-$AGENTS_DIR/overlay/claude-transformer.yaml}"
CORPUS_DIR="${CORPUS_DIR:-/home/fabricio/src/the_edge_agent/docs/stories}"
RESULTS_DIR="${RESULTS_DIR:-/tmp/conformance-results}"
DB_NAME="${DB_NAME:-mass-test-controlled}"
MOUNT_POINT="${MOUNT_POINT:-$RESULTS_DIR/mnt}"
CONFORMANCE_TIMEOUT="${CONFORMANCE_TIMEOUT:-180}"

# Rate limit: 4 files per minute = 15 seconds between files
DELAY_BETWEEN_FILES=15

# Arguments
START_INDEX=${1:-0}
COUNT=${2:-100}

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[PASS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[FAIL]${NC} $1"; }
log_cyan() { echo -e "${CYAN}$1${NC}"; }

MOUNT_PID=""

cleanup() {
    log_info "Cleaning up..."
    if [ -n "$MOUNT_PID" ] && kill -0 "$MOUNT_PID" 2>/dev/null; then
        kill "$MOUNT_PID" 2>/dev/null || true
        sleep 2
    fi
    fusermount -u "$MOUNT_POINT" 2>/dev/null || true
    sleep 1
}

trap cleanup EXIT

setup_environment() {
    log_info "Setting up environment..."

    mkdir -p "$RESULTS_DIR/reports"
    mkdir -p "$RESULTS_DIR/fixed-markdown"
    mkdir -p "$MOUNT_POINT"

    cd "$RESULTS_DIR"
    if [ ! -f ".agentfs/${DB_NAME}.duckdb" ]; then
        log_info "Creating DuckDB database: $DB_NAME"
        "$AGENTFS_CLI" duckdb init "$DB_NAME" --force
    fi

    log_success "Environment ready"
}

start_fuse_mount() {
    log_info "Starting FUSE mount with TEA conformance..."

    cd "$RESULTS_DIR"

    # Export TEA_BINARY for conformance pipeline
    export TEA_BINARY=/home/fabricio/src/the_edge_agent/.venv/bin/tea

    # Start mount in background
    "$AGENTFS_CLI" mount "$DB_NAME" "$MOUNT_POINT" \
        --tea-conformance \
        --tea-agents-dir "$AGENTS_DIR" \
        --tea-overlay "$OVERLAY" \
        --tea-timeout 120 \
        --foreground &

    MOUNT_PID=$!

    # Wait for mount to be ready
    local wait_count=0
    while ! mountpoint -q "$MOUNT_POINT" && [ $wait_count -lt 10 ]; do
        sleep 1
        wait_count=$((wait_count + 1))
    done

    if ! mountpoint -q "$MOUNT_POINT"; then
        log_error "FUSE mount failed"
        exit 1
    fi

    # Create stories directory and copy template
    mkdir -p "$MOUNT_POINT/stories"
    cp "/home/fabricio/src/the_edge_agent/.bmad-core/templates/story-tmpl.yaml" "$MOUNT_POINT/stories/" 2>/dev/null || true

    log_success "FUSE mount ready at $MOUNT_POINT (PID: $MOUNT_PID)"
}

test_single_file() {
    local src_file="$1"
    local filename=$(basename "$src_file")
    local dest_file="$MOUNT_POINT/stories/$filename"
    local report_file="$RESULTS_DIR/reports/${filename%.md}.json"
    local fixed_file="$RESULTS_DIR/fixed-markdown/$filename"

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
            local output_size=$(stat -c%s "$conformant_file" 2>/dev/null || echo "0")

            # Copy conformant file to fixed-markdown directory
            cp "$conformant_file" "$fixed_file"

            # Generate report
            cat > "$report_file" <<EOF
{
  "filename": "$filename",
  "status": "conformant",
  "duration_secs": $duration,
  "source_path": "$src_file",
  "conformant_path": "$conformant_file",
  "output_size_bytes": $output_size,
  "timestamp": "$(date -Iseconds)"
}
EOF
            log_success "$filename (${duration}s, ${output_size}b)"
            echo "conformant"
            return 0
        fi

        if [ -f "$failed_file" ]; then
            local end_time=$(date +%s.%N)
            local duration=$(echo "$end_time - $start_time" | bc)
            local error_msg=$(cat "$failed_file" 2>/dev/null || echo '{"error": "unknown"}')

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

run_controlled_test() {
    local -a all_files
    mapfile -t all_files < <(find "$CORPUS_DIR" -maxdepth 1 -name "*.md" -type f | sort)

    local total_available=${#all_files[@]}
    log_info "Total files in corpus: $total_available"
    log_info "Starting from index: $START_INDEX, processing: $COUNT files"
    log_cyan "Rate limit: 4 files/minute (${DELAY_BETWEEN_FILES}s delay between files)"
    echo ""

    # Calculate end index
    local end_index=$((START_INDEX + COUNT))
    if [ $end_index -gt $total_available ]; then
        end_index=$total_available
    fi

    # Extract files to process
    local -a files=("${all_files[@]:$START_INDEX:$COUNT}")
    local actual_count=${#files[@]}

    log_info "Processing files $START_INDEX to $((end_index - 1)) ($actual_count files)"

    local conformant=0
    local failed=0
    local timeout=0
    local start_time=$(date +%s)

    for i in "${!files[@]}"; do
        local file="${files[$i]}"
        local global_idx=$((START_INDEX + i))
        local progress=$((i + 1))

        echo -n "[$progress/$actual_count, global: $global_idx] "

        local result=$(test_single_file "$file")

        case "$result" in
            conformant) ((conformant++)) ;;
            failed) ((failed++)) ;;
            timeout) ((timeout++)) ;;
        esac

        # Rate limiting (skip delay on last file)
        if [ $progress -lt $actual_count ]; then
            log_info "Waiting ${DELAY_BETWEEN_FILES}s (rate limit: 4/min)..."
            sleep "$DELAY_BETWEEN_FILES"
        fi
    done

    local end_time=$(date +%s)
    local total_duration=$((end_time - start_time))

    # Generate summary
    echo ""
    log_cyan "========== RESULTS ($START_INDEX to $((end_index - 1))) =========="
    echo "Processed:      $actual_count"
    echo "Conformant:     $conformant ($(echo "scale=1; $conformant * 100 / $actual_count" | bc)%)"
    echo "Failed:         $failed ($(echo "scale=1; $failed * 100 / $actual_count" | bc)%)"
    echo "Timeout:        $timeout ($(echo "scale=1; $timeout * 100 / $actual_count" | bc)%)"
    echo "Total duration: ${total_duration}s"
    echo "Avg per file:   $(echo "scale=2; $total_duration / $actual_count" | bc)s"
    echo ""
    echo "Fixed markdown files: $RESULTS_DIR/fixed-markdown/"
    echo ""

    # Write batch summary
    local summary_file="$RESULTS_DIR/batch-${START_INDEX}-${end_index}-$(date +%Y%m%d-%H%M%S).json"
    cat > "$summary_file" <<EOF
{
  "batch_start": $START_INDEX,
  "batch_end": $end_index,
  "processed": $actual_count,
  "conformant": $conformant,
  "failed": $failed,
  "timeout": $timeout,
  "conformance_rate": $(echo "scale=4; $conformant / $actual_count" | bc),
  "total_duration_secs": $total_duration,
  "avg_duration_secs": $(echo "scale=2; $total_duration / $actual_count" | bc),
  "timestamp": "$(date -Iseconds)"
}
EOF
    log_success "Batch summary: $summary_file"

    # Merge all reports to JSONL
    local jsonl_file="$RESULTS_DIR/reports.jsonl"
    > "$jsonl_file"
    for report in "$RESULTS_DIR/reports"/*.json; do
        if [ -f "$report" ]; then
            cat "$report" >> "$jsonl_file"
            echo "" >> "$jsonl_file"
        fi
    done
    log_success "All reports: $jsonl_file"
}

# Main
setup_environment
start_fuse_mount
run_controlled_test
