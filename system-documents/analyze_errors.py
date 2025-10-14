#!/usr/bin/env python3
"""
Analyze QA test results and categorize errors by priority
"""

import json
from pathlib import Path
from collections import defaultdict

BASE_DIR = Path("/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler")
RESULTS_JSON = BASE_DIR / "system-documents" / "qa_test_results.json"

def main():
    with open(RESULTS_JSON) as f:
        data = json.load(f)

    total = data["total_files"]
    passed = data["passed"]
    compile_failed = data["compile_failed"]
    exec_failed = data["exec_failed"]

    print(f"=== OVERALL SUMMARY ===")
    print(f"Total Files: {total}")
    print(f"Passed: {passed} ({passed * 100 // total}%)")
    print(f"Compile Failed: {compile_failed} ({compile_failed * 100 // total}%)")
    print(f"Exec Failed: {exec_failed} ({exec_failed * 100 // total}%)")
    print()

    # Categorize by error type
    error_types = defaultdict(list)
    error_patterns = defaultdict(list)
    category_stats = defaultdict(lambda: {"total": 0, "pass": 0, "compile_fail": 0, "exec_fail": 0})

    for result in data["results"]:
        file = result["file"]
        status = result["status"]
        category = file.split("/")[0]

        category_stats[category]["total"] += 1
        if status == "PASS":
            category_stats[category]["pass"] += 1
        elif status == "COMPILE_FAIL":
            category_stats[category]["compile_fail"] += 1
            error_types[result["error_type"]].append(file)
            error_patterns[result["error_pattern"]].append(file)
        else:
            category_stats[category]["exec_fail"] += 1
            error_types[result["error_type"]].append(file)
            error_patterns[result["error_pattern"]].append(file)

    print("=== ERROR TYPES ===")
    for error_type, files in sorted(error_types.items(), key=lambda x: len(x[1]), reverse=True):
        count = len(files)
        pct = count * 100 // total
        priority = "🔴 CRITICAL" if pct > 20 else "🟡 HIGH" if pct > 10 else "🟢 MEDIUM" if pct > 5 else "⚪ LOW"
        print(f"{priority}: {error_type} - {count} files ({pct}%)")

    print("\n=== ERROR PATTERNS (Top 20) ===")
    for pattern, files in sorted(error_patterns.items(), key=lambda x: len(x[1]), reverse=True)[:20]:
        count = len(files)
        pct = count * 100 // total
        priority = "🔴 CRITICAL" if pct > 20 else "🟡 HIGH" if pct > 10 else "🟢 MEDIUM" if pct > 5 else "⚪ LOW"
        print(f"{priority}: {pattern[:80]} - {count} files ({pct}%)")

    print("\n=== CATEGORY STATISTICS ===")
    for category, stats in sorted(category_stats.items()):
        total_cat = stats["total"]
        pass_cat = stats["pass"]
        pass_rate = pass_cat * 100 // total_cat if total_cat > 0 else 0
        print(f"{category:20} - Total: {total_cat:3}, Pass: {pass_cat:3} ({pass_rate:2}%), "
              f"Compile Fail: {stats['compile_fail']:3}, Exec Fail: {stats['exec_fail']:3}")

    # Generate detailed breakdown
    print("\n=== DETAILED ERROR BREAKDOWN ===")
    for error_type, files in sorted(error_types.items(), key=lambda x: len(x[1]), reverse=True):
        count = len(files)
        pct = count * 100 // total
        print(f"\n{error_type} ({count} files, {pct}%):")
        for file in sorted(files)[:5]:
            print(f"  - {file}")
        if len(files) > 5:
            print(f"  ... and {len(files) - 5} more")

if __name__ == "__main__":
    main()
