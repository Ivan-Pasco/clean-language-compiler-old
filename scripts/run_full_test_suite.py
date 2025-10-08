#!/usr/bin/env python3
"""
REAL Comprehensive Test Suite Runner
Tests ALL .cln files in tests/cln/ directory

NO OPTIMISTIC LANGUAGE - BRUTAL HONESTY ONLY
"""

import subprocess
import glob
import json
import time
from pathlib import Path
from datetime import datetime
from collections import defaultdict

def run_compilation_test(file_path):
    """Test if a single file compiles successfully"""
    try:
        result = subprocess.run(
            ["cargo", "run", "--bin", "clean-language-compiler",
             "compile", "-i", file_path, "-o", "/tmp/test_output.wasm"],
            capture_output=True,
            text=True,
            timeout=30
        )

        success = "Successfully compiled" in result.stderr or "Successfully compiled" in result.stdout
        error_msg = None

        if not success:
            # Extract error message
            if "Error:" in result.stderr:
                error_msg = result.stderr.split("Error:")[1].split("\n")[0].strip()
            elif "error:" in result.stderr:
                error_msg = result.stderr.split("error:")[1].split("\n")[0].strip()
            else:
                error_msg = "Unknown compilation error"

        return {
            "success": success,
            "error": error_msg,
            "stdout": result.stdout[:200],
            "stderr": result.stderr[:200]
        }
    except subprocess.TimeoutExpired:
        return {
            "success": False,
            "error": "Compilation timeout (30s)",
            "stdout": "",
            "stderr": ""
        }
    except Exception as e:
        return {
            "success": False,
            "error": f"Test runner error: {str(e)}",
            "stdout": "",
            "stderr": ""
        }

def categorize_file(file_path):
    """Categorize test file by directory structure"""
    parts = Path(file_path).parts
    if "tests" in parts and "cln" in parts:
        cln_idx = parts.index("cln")
        if cln_idx + 1 < len(parts):
            return parts[cln_idx + 1]
    return "uncategorized"

def main():
    print("=" * 80)
    print("REAL COMPREHENSIVE TEST SUITE")
    print("Testing ALL .cln files - NO SAMPLES, NO OPTIMISM")
    print("=" * 80)
    print()

    # Find all test files
    test_files = sorted(glob.glob("tests/cln/**/*.cln", recursive=True))
    total = len(test_files)

    if total == 0:
        print("❌ ERROR: No test files found in tests/cln/")
        return 1

    print(f"📁 Found {total} test files")
    print(f"⏱️  Estimated time: ~{total * 3} seconds")
    print()

    # Run all tests
    results = {}
    passed = 0
    failed = 0
    categories = defaultdict(lambda: {"passed": 0, "failed": 0})

    start_time = time.time()

    for i, file_path in enumerate(test_files, 1):
        file_name = Path(file_path).name
        category = categorize_file(file_path)

        # Progress indicator
        print(f"[{i}/{total}] Testing {file_name}...", end=" ", flush=True)

        result = run_compilation_test(file_path)
        results[file_path] = result

        if result["success"]:
            print("✓")
            passed += 1
            categories[category]["passed"] += 1
        else:
            print(f"✗ {result['error']}")
            failed += 1
            categories[category]["failed"] += 1

    elapsed = time.time() - start_time

    # Generate results
    print()
    print("=" * 80)
    print("RESULTS - BRUTAL HONESTY")
    print("=" * 80)
    print()

    pass_rate = (passed / total) * 100

    print(f"✅ PASSED: {passed}/{total} ({pass_rate:.1f}%)")
    print(f"❌ FAILED: {failed}/{total} ({100-pass_rate:.1f}%)")
    print(f"⏱️  Time: {elapsed:.1f} seconds")
    print()

    # Category breakdown
    print("CATEGORY BREAKDOWN:")
    print("-" * 80)
    for category in sorted(categories.keys()):
        cat_passed = categories[category]["passed"]
        cat_failed = categories[category]["failed"]
        cat_total = cat_passed + cat_failed
        cat_rate = (cat_passed / cat_total) * 100 if cat_total > 0 else 0
        print(f"  {category:20s}: {cat_passed:3d}/{cat_total:3d} ({cat_rate:5.1f}%)")
    print()

    # Failed files list
    if failed > 0:
        print("FAILED FILES:")
        print("-" * 80)
        failed_files = [(f, r) for f, r in results.items() if not r["success"]]
        for file_path, result in sorted(failed_files)[:50]:  # Show first 50
            print(f"  ✗ {Path(file_path).name}")
            print(f"    Error: {result['error']}")

        if len(failed_files) > 50:
            print(f"  ... and {len(failed_files) - 50} more failures")
        print()

    # Save detailed results
    results_dir = Path("tests/results")
    results_dir.mkdir(exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")

    # JSON results
    detailed_results = {
        "timestamp": timestamp,
        "total": total,
        "passed": passed,
        "failed": failed,
        "pass_rate": pass_rate,
        "elapsed_seconds": elapsed,
        "categories": dict(categories),
        "results": results
    }

    json_path = results_dir / f"run_{timestamp}.json"
    with open(json_path, "w") as f:
        json.dump(detailed_results, f, indent=2)

    # Also save as latest
    latest_path = results_dir / "latest_run.json"
    with open(latest_path, "w") as f:
        json.dump(detailed_results, f, indent=2)

    print(f"📊 Detailed results saved to: {json_path}")
    print(f"📊 Latest results: {latest_path}")
    print()

    # Status assessment
    print("=" * 80)
    print("HONEST STATUS ASSESSMENT")
    print("=" * 80)

    if pass_rate == 100:
        print("✅ COMPILER IS READY - All tests pass")
    elif pass_rate >= 95:
        print(f"⚠️  NEARLY READY - {failed} failures remaining")
    elif pass_rate >= 80:
        print(f"⚠️  PARTIAL FUNCTIONALITY - {failed} failures blocking readiness")
    elif pass_rate >= 50:
        print(f"❌ NOT READY - Major functionality gaps ({failed} failures)")
    else:
        print(f"❌ SEVERELY BROKEN - Most features failing ({failed} failures)")

    print()
    print("Do NOT claim 'ready' until 100% pass rate achieved")
    print("=" * 80)

    return 0 if passed == total else 1

if __name__ == "__main__":
    exit(main())
