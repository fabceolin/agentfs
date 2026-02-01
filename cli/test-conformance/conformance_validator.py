#!/usr/bin/env python3
"""
conformance_validator.py - Analyze TEA conformance test results
STORY-8.1: Mass Conformance Validation

Analyzes JSONL reports from test-harness.sh to:
1. Calculate conformance rates
2. Identify failure patterns
3. Categorize non-conformant stories
4. Generate recommendations
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class TestResult:
    """Single test result from JSONL."""

    filename: str
    status: str  # conformant, failed, timeout
    duration_secs: float
    source_path: str
    timestamp: str
    error: dict[str, Any] | None = None
    conformant_path: str | None = None


@dataclass
class AnalysisReport:
    """Aggregated analysis results."""

    total_files: int = 0
    conformant: int = 0
    failed: int = 0
    timeout: int = 0

    # Timing
    total_duration: float = 0.0
    avg_duration: float = 0.0
    min_duration: float = float("inf")
    max_duration: float = 0.0

    # Failure patterns
    failure_patterns: Counter = field(default_factory=Counter)
    failures_by_category: dict[str, list[str]] = field(default_factory=lambda: defaultdict(list))

    # Story categories
    story_categories: Counter = field(default_factory=Counter)
    conformance_by_category: dict[str, dict[str, int]] = field(
        default_factory=lambda: defaultdict(lambda: {"total": 0, "conformant": 0})
    )


def categorize_story(filename: str) -> str:
    """Categorize story by filename pattern."""
    patterns = [
        (r"^TEA-", "TEA (Core)"),
        (r"^BUG\.", "Bug Fix"),
        (r"^DOC[-.]", "Documentation"),
        (r"^TD\.", "Technical Debt"),
        (r"^RUST\.", "Rust Specific"),
        (r"^YE\.", "Yagna/Edge"),
        (r"^[A-Z]+\d+\.", "Numbered Story"),
        (r"^\d+\.", "Legacy Numbered"),
    ]

    for pattern, category in patterns:
        if re.match(pattern, filename):
            return category

    return "Other"


def extract_failure_pattern(error: dict[str, Any] | None) -> str:
    """Extract failure pattern from error object."""
    if not error:
        return "unknown"

    error_type = error.get("error_type", "")
    message = error.get("message", "")

    # Common patterns
    if "timeout" in message.lower():
        return "api_timeout"
    if "rate limit" in message.lower():
        return "rate_limited"
    if "parse" in message.lower():
        return "parse_error"
    if "template" in message.lower():
        return "template_error"
    if "conformance" in error_type:
        return "conformance_check_failed"
    if "transform" in message.lower():
        return "transformation_failed"

    return error_type or "unknown"


def load_results(jsonl_path: Path) -> list[TestResult]:
    """Load test results from JSONL file."""
    results = []

    with open(jsonl_path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue

            try:
                data = json.loads(line)
                results.append(
                    TestResult(
                        filename=data.get("filename", ""),
                        status=data.get("status", "unknown"),
                        duration_secs=float(data.get("duration_secs", 0)),
                        source_path=data.get("source_path", ""),
                        timestamp=data.get("timestamp", ""),
                        error=data.get("error"),
                        conformant_path=data.get("conformant_path"),
                    )
                )
            except json.JSONDecodeError as e:
                print(f"Warning: Failed to parse line: {e}", file=sys.stderr)

    return results


def analyze_results(results: list[TestResult]) -> AnalysisReport:
    """Analyze test results and generate report."""
    report = AnalysisReport()
    report.total_files = len(results)

    for r in results:
        # Count by status
        if r.status == "conformant":
            report.conformant += 1
        elif r.status == "failed":
            report.failed += 1
        elif r.status == "timeout":
            report.timeout += 1

        # Timing stats
        report.total_duration += r.duration_secs
        report.min_duration = min(report.min_duration, r.duration_secs)
        report.max_duration = max(report.max_duration, r.duration_secs)

        # Categorize story
        category = categorize_story(r.filename)
        report.story_categories[category] += 1
        report.conformance_by_category[category]["total"] += 1
        if r.status == "conformant":
            report.conformance_by_category[category]["conformant"] += 1

        # Track failure patterns
        if r.status == "failed":
            pattern = extract_failure_pattern(r.error)
            report.failure_patterns[pattern] += 1
            report.failures_by_category[category].append(r.filename)

    if report.total_files > 0:
        report.avg_duration = report.total_duration / report.total_files

    return report


def print_report(report: AnalysisReport, verbose: bool = False) -> None:
    """Print analysis report to stdout."""
    print("=" * 60)
    print("  TEA CONFORMANCE VALIDATION REPORT")
    print("=" * 60)
    print()

    # Summary
    print("SUMMARY")
    print("-" * 40)
    print(f"  Total files:     {report.total_files}")
    print(f"  Conformant:      {report.conformant} ({report.conformant * 100 / report.total_files:.1f}%)")
    print(f"  Failed:          {report.failed} ({report.failed * 100 / report.total_files:.1f}%)")
    print(f"  Timeout:         {report.timeout} ({report.timeout * 100 / report.total_files:.1f}%)")
    print()

    # Timing
    print("TIMING")
    print("-" * 40)
    print(f"  Total duration:  {report.total_duration:.1f}s")
    print(f"  Avg per file:    {report.avg_duration:.2f}s")
    print(f"  Min duration:    {report.min_duration:.2f}s")
    print(f"  Max duration:    {report.max_duration:.2f}s")
    print()

    # Conformance by category
    print("CONFORMANCE BY CATEGORY")
    print("-" * 40)
    for category, stats in sorted(report.conformance_by_category.items()):
        total = stats["total"]
        conformant = stats["conformant"]
        rate = conformant * 100 / total if total > 0 else 0
        bar = "#" * int(rate / 5) + "." * (20 - int(rate / 5))
        print(f"  {category:<20} [{bar}] {rate:5.1f}% ({conformant}/{total})")
    print()

    # Failure patterns
    if report.failure_patterns:
        print("FAILURE PATTERNS")
        print("-" * 40)
        for pattern, count in report.failure_patterns.most_common(10):
            print(f"  {pattern:<25} {count:4d}")
        print()

    # Recommendations
    print("RECOMMENDATIONS")
    print("-" * 40)

    conformance_rate = report.conformant * 100 / report.total_files if report.total_files > 0 else 0

    if conformance_rate >= 70:
        print("  [OK] Conformance rate meets target (>70%)")
    else:
        print(f"  [WARN] Conformance rate below target: {conformance_rate:.1f}% < 70%")

    if report.timeout > report.total_files * 0.05:
        print(f"  [WARN] High timeout rate ({report.timeout}/{report.total_files})")
        print("         Consider increasing timeout or optimizing pipeline")

    if "api_timeout" in report.failure_patterns:
        print("  [WARN] API timeouts detected - check Claude rate limits")

    if "parse_error" in report.failure_patterns:
        print("  [WARN] Parse errors detected - review document structure")

    # Verbose: list failures by category
    if verbose and report.failures_by_category:
        print()
        print("FAILURES BY CATEGORY (verbose)")
        print("-" * 40)
        for category, files in sorted(report.failures_by_category.items()):
            print(f"  {category}:")
            for f in files[:5]:  # Limit to 5 per category
                print(f"    - {f}")
            if len(files) > 5:
                print(f"    ... and {len(files) - 5} more")
            print()


def export_json(report: AnalysisReport, output_path: Path) -> None:
    """Export report as JSON."""
    data = {
        "summary": {
            "total_files": report.total_files,
            "conformant": report.conformant,
            "failed": report.failed,
            "timeout": report.timeout,
            "conformance_rate": report.conformant / report.total_files if report.total_files > 0 else 0,
        },
        "timing": {
            "total_duration_secs": report.total_duration,
            "avg_duration_secs": report.avg_duration,
            "min_duration_secs": report.min_duration,
            "max_duration_secs": report.max_duration,
        },
        "conformance_by_category": {
            k: {"total": v["total"], "conformant": v["conformant"], "rate": v["conformant"] / v["total"] if v["total"] > 0 else 0}
            for k, v in report.conformance_by_category.items()
        },
        "failure_patterns": dict(report.failure_patterns),
        "failures_by_category": dict(report.failures_by_category),
    }

    with open(output_path, "w") as f:
        json.dump(data, f, indent=2)

    print(f"JSON report exported to: {output_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Analyze TEA conformance test results")
    parser.add_argument("jsonl_file", type=Path, help="Path to reports.jsonl file")
    parser.add_argument("-v", "--verbose", action="store_true", help="Show detailed failures")
    parser.add_argument("-o", "--output", type=Path, help="Export JSON report to file")
    parser.add_argument("--json", action="store_true", help="Output as JSON to stdout")

    args = parser.parse_args()

    if not args.jsonl_file.exists():
        print(f"Error: File not found: {args.jsonl_file}", file=sys.stderr)
        sys.exit(1)

    results = load_results(args.jsonl_file)
    if not results:
        print("Error: No results found in file", file=sys.stderr)
        sys.exit(1)

    report = analyze_results(results)

    if args.json:
        data = {
            "summary": {
                "total_files": report.total_files,
                "conformant": report.conformant,
                "failed": report.failed,
                "timeout": report.timeout,
                "conformance_rate": report.conformant / report.total_files if report.total_files > 0 else 0,
            },
            "timing": {
                "total_duration_secs": report.total_duration,
                "avg_duration_secs": report.avg_duration,
            },
            "failure_patterns": dict(report.failure_patterns),
        }
        print(json.dumps(data, indent=2))
    else:
        print_report(report, verbose=args.verbose)

    if args.output:
        export_json(report, args.output)


if __name__ == "__main__":
    main()
