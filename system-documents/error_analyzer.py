#!/usr/bin/env python3
"""
Systematic error analyzer for Clean Language compiler tests
Tests files and categorizes errors by root cause
"""

import subprocess
import sys
import os
from pathlib import Path
from collections import defaultdict
import re

def test_file(file_path):
    """Test a single file and return (success, error_info)"""
    output_file = f"tests/output/{Path(file_path).stem}.wasm"

    try:
        result = subprocess.run(
            ['cargo', 'run', '--quiet', '--bin', 'clean-language-compiler', 'compile',
             '-i', file_path, '-o', output_file],
            timeout=15,
            capture_output=True,
            text=True
        )

        if result.returncode == 0:
            return (True, None)
        else:
            # Extract error from stderr
            stderr = result.stderr
            stdout = result.stdout
            combined = stdout + "\n" + stderr

            # Look for key error patterns
            error_type = "Unknown"
            error_detail = ""

            if "ParseError" in combined or "parsing failed" in combined.lower():
                error_type = "ParseError"
                # Extract specific parse error
                match = re.search(r'(ParseError|parsing failed)[:\s]+(.+)', combined, re.IGNORECASE)
                if match:
                    error_detail = match.group(2).strip()[:200]
            elif "Preprocessor" in combined:
                error_type = "PreprocessorError"
                match = re.search(r'Preprocessor[:\s]+(.+)', combined)
                if match:
                    error_detail = match.group(1).strip()[:200]
            elif "TypeError" in combined or "type error" in combined.lower():
                error_type = "TypeError"
                match = re.search(r'(TypeError|type error)[:\s]+(.+)', combined, re.IGNORECASE)
                if match:
                    error_detail = match.group(2).strip()[:200]
            elif "Semantic" in combined:
                error_type = "SemanticError"
                match = re.search(r'Semantic[:\s]+(.+)', combined)
                if match:
                    error_detail = match.group(1).strip()[:200]
            elif "UndefinedVariable" in combined or "undefined variable" in combined.lower():
                error_type = "UndefinedVariable"
                match = re.search(r'(UndefinedVariable|undefined variable)[:\s]+(.+)', combined, re.IGNORECASE)
                if match:
                    error_detail = match.group(2).strip()[:200]
            elif "timeout" in combined.lower():
                error_type = "Timeout"
                error_detail = "Compilation timed out after 15 seconds"
            else:
                # Grab first error line
                lines = combined.split('\n')
                for line in lines:
                    if 'error' in line.lower() or 'failed' in line.lower():
                        error_detail = line.strip()[:200]
                        break

            return (False, {
                'type': error_type,
                'detail': error_detail,
                'full_output': combined[:500]  # Keep first 500 chars
            })

    except subprocess.TimeoutExpired:
        return (False, {
            'type': 'Timeout',
            'detail': 'Compilation timed out after 15 seconds',
            'full_output': ''
        })
    except Exception as e:
        return (False, {
            'type': 'SystemError',
            'detail': str(e),
            'full_output': ''
        })

def categorize_errors(test_results):
    """Categorize errors by type and count"""
    error_categories = defaultdict(list)

    for file_path, (success, error_info) in test_results.items():
        if not success and error_info:
            error_categories[error_info['type']].append({
                'file': file_path,
                'detail': error_info['detail']
            })

    return error_categories

def main():
    """Main execution"""
    # Find all test files
    test_dir = Path('tests/cln')
    test_files = sorted(test_dir.rglob('*.cln'))

    print(f"Found {len(test_files)} test files")

    # Sample 20 files from different categories for initial analysis
    sample_files = []
    categories = defaultdict(list)

    for f in test_files:
        category = f.parent.name
        categories[category].append(f)

    # Take 2-3 from each category
    for category, files in categories.items():
        sample_files.extend(files[:3])

    sample_files = sample_files[:20]  # Limit to 20 total

    print(f"\nTesting sample of {len(sample_files)} files...")

    test_results = {}
    passed = 0
    failed = 0

    for i, test_file_path in enumerate(sample_files, 1):
        print(f"[{i}/{len(sample_files)}] Testing: {test_file_path.relative_to('tests/cln')}", end=' ... ')

        success, error_info = test_file(str(test_file_path))
        test_results[str(test_file_path)] = (success, error_info)

        if success:
            print("PASS")
            passed += 1
        else:
            print(f"FAIL ({error_info['type']})")
            failed += 1

    print(f"\n\nResults: {passed} passed, {failed} failed")
    print(f"Success rate: {passed}/{len(sample_files)} ({100*passed//len(sample_files)}%)")

    # Categorize errors
    error_categories = categorize_errors(test_results)

    print("\n\n=== ERROR CATEGORIES ===\n")
    for error_type, errors in sorted(error_categories.items(), key=lambda x: len(x[1]), reverse=True):
        print(f"\n{error_type}: {len(errors)} files")
        print("-" * 60)
        for error in errors[:5]:  # Show first 5
            print(f"  File: {Path(error['file']).relative_to('tests/cln')}")
            if error['detail']:
                print(f"    {error['detail'][:100]}")
        if len(errors) > 5:
            print(f"  ... and {len(errors) - 5} more")

    return 0

if __name__ == '__main__':
    sys.exit(main())
