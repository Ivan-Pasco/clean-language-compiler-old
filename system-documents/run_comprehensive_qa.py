#!/usr/bin/env python3
"""
Comprehensive QA Testing Script for Clean Language Compiler
Tests all .cln files, categorizes errors, and generates priority report
"""

import subprocess
import json
import os
import sys
from pathlib import Path
from collections import defaultdict
from datetime import datetime
import re

# Configuration
BASE_DIR = Path("/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler")
TEST_DIR = BASE_DIR / "tests" / "cln"
OUTPUT_DIR = BASE_DIR / "tests" / "output"
COMPILER_BIN = BASE_DIR / "target" / "debug" / "clean-language-compiler"
RUNNER_BIN = BASE_DIR / "target" / "debug" / "wasmtime_runner"
RESULTS_JSON = BASE_DIR / "system-documents" / "qa_test_results.json"

# ANSI colors
RED = '\033[0;31m'
GREEN = '\033[0;32m'
YELLOW = '\033[1;33m'
BLUE = '\033[0;34m'
NC = '\033[0m'  # No Color

def run_command(cmd, timeout=30):
    """Run a command and return output, status"""
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout
        )
        return result.stdout + result.stderr, result.returncode
    except subprocess.TimeoutExpired:
        return "TIMEOUT", 1
    except Exception as e:
        return f"ERROR: {e}", 1

def categorize_error(error_msg):
    """Categorize error based on message patterns"""
    error_msg_lower = error_msg.lower()

    # Parse errors
    if "parse error" in error_msg_lower or "unexpected token" in error_msg_lower:
        return "parse_error"

    # Semantic errors
    if "semantic error" in error_msg_lower or "undefined variable" in error_msg_lower:
        return "semantic_error"

    # Type errors
    if "type error" in error_msg_lower or "type mismatch" in error_msg_lower:
        return "type_error"

    # Codegen errors
    if "codegen" in error_msg_lower or "wasm" in error_msg_lower:
        return "codegen_error"

    # Runtime errors
    if "runtime" in error_msg_lower or "trap" in error_msg_lower:
        return "runtime_error"

    # Panics
    if "panic" in error_msg_lower or "thread" in error_msg_lower:
        return "panic"

    return "unknown"

def extract_error_pattern(error_msg):
    """Extract specific error pattern for grouping"""
    # Common error patterns
    patterns = [
        (r"undefined variable: (\w+)", "undefined_variable"),
        (r"expected (\w+), found (\w+)", "type_mismatch"),
        (r"parse error.*?at line (\d+)", "parse_error_specific"),
        (r"(not implemented|unimplemented)", "not_implemented"),
        (r"(precision modifiers)", "precision_modifiers"),
        (r"(class inheritance)", "class_inheritance"),
        (r"(async.*?await)", "async_await"),
        (r"(import.*?export)", "import_export"),
    ]

    for pattern, name in patterns:
        if re.search(pattern, error_msg, re.IGNORECASE):
            return name

    # Generic categorization
    first_line = error_msg.split('\n')[0][:100]
    return first_line

def test_file(file_path):
    """Test a single .cln file"""
    rel_path = file_path.relative_to(TEST_DIR)

    # Create output path
    output_file = OUTPUT_DIR / rel_path.with_suffix('.wasm')
    output_file.parent.mkdir(parents=True, exist_ok=True)

    # Compile
    compile_cmd = [str(COMPILER_BIN), "compile", "-i", str(file_path), "-o", str(output_file)]
    compile_output, compile_status = run_command(compile_cmd, timeout=60)

    if compile_status != 0:
        error_type = categorize_error(compile_output)
        error_pattern = extract_error_pattern(compile_output)
        return {
            "file": str(rel_path),
            "status": "COMPILE_FAIL",
            "error_type": error_type,
            "error_pattern": error_pattern,
            "error_message": compile_output.split('\n')[0][:200]
        }

    # Execute
    exec_cmd = [str(RUNNER_BIN), str(output_file)]
    exec_output, exec_status = run_command(exec_cmd, timeout=30)

    if exec_status != 0:
        error_type = categorize_error(exec_output)
        error_pattern = extract_error_pattern(exec_output)
        return {
            "file": str(rel_path),
            "status": "EXEC_FAIL",
            "error_type": error_type,
            "error_pattern": error_pattern,
            "error_message": exec_output.split('\n')[0][:200]
        }

    return {
        "file": str(rel_path),
        "status": "PASS",
        "error_type": None,
        "error_pattern": None,
        "error_message": None
    }

def main():
    """Main testing loop"""
    print(f"{BLUE}=== Clean Language Comprehensive QA Testing ==={NC}")
    print(f"Testing directory: {TEST_DIR}")
    print(f"Output directory: {OUTPUT_DIR}")
    print()

    # Ensure output directory exists
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    # Find all test files
    test_files = sorted(TEST_DIR.rglob("*.cln"))
    total_files = len(test_files)

    print(f"Found {total_files} test files\n")

    # Test all files
    results = []
    passed = 0
    compile_failed = 0
    exec_failed = 0

    for i, file_path in enumerate(test_files, 1):
        rel_path = file_path.relative_to(TEST_DIR)
        print(f"[{i}/{total_files}] Testing: {rel_path}", end=" ... ")

        result = test_file(file_path)
        results.append(result)

        if result["status"] == "PASS":
            passed += 1
            print(f"{GREEN}PASS{NC}")
        elif result["status"] == "COMPILE_FAIL":
            compile_failed += 1
            print(f"{RED}COMPILE_FAIL{NC} ({result['error_type']})")
        else:
            exec_failed += 1
            print(f"{YELLOW}EXEC_FAIL{NC} ({result['error_type']})")

    # Save results
    output_data = {
        "timestamp": datetime.utcnow().isoformat() + "Z",
        "total_files": total_files,
        "passed": passed,
        "compile_failed": compile_failed,
        "exec_failed": exec_failed,
        "pass_rate": round(passed / total_files * 100, 2),
        "results": results
    }

    with open(RESULTS_JSON, 'w') as f:
        json.dump(output_data, f, indent=2)

    # Print summary
    print(f"\n{BLUE}{'=' * 50}{NC}")
    print(f"{BLUE}COMPREHENSIVE QA TEST RESULTS{NC}")
    print(f"{BLUE}{'=' * 50}{NC}")
    print(f"Total Files:        {total_files}")
    print(f"{GREEN}Passed:            {passed} ({passed * 100 // total_files}%){NC}")
    print(f"{RED}Compile Failed:    {compile_failed} ({compile_failed * 100 // total_files}%){NC}")
    print(f"{YELLOW}Exec Failed:       {exec_failed} ({exec_failed * 100 // total_files}%){NC}")
    print(f"{BLUE}{'=' * 50}{NC}")
    print(f"Results saved to: {RESULTS_JSON}")

    return 0 if passed == total_files else 1

if __name__ == "__main__":
    sys.exit(main())
