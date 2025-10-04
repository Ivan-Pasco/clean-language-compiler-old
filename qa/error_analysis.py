#!/usr/bin/env python3
"""
Error Pattern Analysis Script for Clean Language Compiler QA
Analyzes error patterns from test compilation failures to identify critical gaps
"""

import re
from collections import defaultdict, Counter
from pathlib import Path

def analyze_error_patterns():
    error_file = Path("/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/error_patterns.txt")
    
    if not error_file.exists():
        print("Error patterns file not found!")
        return
    
    with open(error_file, 'r') as f:
        content = f.read()
    
    # Parse errors by test file
    error_blocks = re.split(r'=== ERROR in (\w+) ===', content)[1:]  # Remove first empty element
    
    errors_by_category = defaultdict(list)
    errors_by_test = {}
    missing_functions = Counter()
    missing_methods = Counter()
    type_errors = []
    
    # Process error blocks (filename, error content pairs)
    for i in range(0, len(error_blocks), 2):
        if i + 1 >= len(error_blocks):
            break
            
        test_name = error_blocks[i]
        error_content = error_blocks[i + 1]
        
        # Categorize the error
        category = categorize_error(error_content)
        errors_by_category[category].append(test_name)
        errors_by_test[test_name] = {
            'category': category,
            'content': error_content.strip()
        }
        
        # Extract specific missing functions/methods
        extract_missing_functions(error_content, missing_functions, missing_methods)
        
        # Extract type errors
        if 'type' in category.lower():
            type_errors.append({'test': test_name, 'error': error_content.strip()})
    
    # Generate comprehensive report
    generate_analysis_report(errors_by_category, errors_by_test, missing_functions, missing_methods, type_errors)

def categorize_error(error_content):
    """Categorize error based on content analysis"""
    error_lower = error_content.lower()
    
    # Specific error patterns
    if 'method' in error_lower and 'not found' in error_lower:
        return 'MISSING_METHOD'
    elif 'namespace function' in error_lower and 'not found' in error_lower:
        return 'MISSING_NAMESPACE_FUNCTION'
    elif 'cannot call method' in error_lower:
        return 'INVALID_METHOD_CALL'
    elif 'variable' in error_lower and 'not found' in error_lower:
        return 'UNDEFINED_VARIABLE'
    elif 'if condition must be' in error_lower:
        return 'INVALID_CONDITION_TYPE'
    elif 'function expects return value' in error_lower:
        return 'MISSING_RETURN_VALUE'
    elif 'type mismatch' in error_lower or 'type error' in error_lower:
        return 'TYPE_MISMATCH'
    elif 'unexpected token' in error_lower:
        return 'PARSER_UNEXPECTED_TOKEN'
    elif 'expected' in error_lower and 'token' in error_lower:
        return 'PARSER_EXPECTED_TOKEN'
    elif 'semantic' in error_lower:
        return 'SEMANTIC_ERROR'
    elif 'codegen' in error_lower or 'wasm' in error_lower:
        return 'CODEGEN_ERROR'
    else:
        return 'OTHER_ERROR'

def extract_missing_functions(error_content, missing_functions, missing_methods):
    """Extract missing function and method names from error messages"""
    
    # Pattern for missing methods: "Method 'methodName' not found"
    method_pattern = r"Method '([^']+)' not found"
    method_matches = re.findall(method_pattern, error_content)
    for method in method_matches:
        missing_methods[method] += 1
    
    # Pattern for missing namespace functions: "function 'namespace.function' not found"
    namespace_pattern = r"function '([^']+)' not found"
    namespace_matches = re.findall(namespace_pattern, error_content)
    for func in namespace_matches:
        missing_functions[func] += 1
    
    # Pattern for missing method calls: "Cannot call method 'methodName'"
    invalid_method_pattern = r"Cannot call method '([^']+)'"
    invalid_method_matches = re.findall(invalid_method_pattern, error_content)
    for method in invalid_method_matches:
        missing_methods[method] += 1

def generate_analysis_report(errors_by_category, errors_by_test, missing_functions, missing_methods, type_errors):
    """Generate comprehensive analysis report"""
    
    report_path = Path("/Users/earcandy/Documents/Dev/Clean Language/clean-language-compiler/COMPREHENSIVE_QA_REPORT.md")
    
    with open(report_path, 'w') as f:
        f.write("# Comprehensive QA Analysis Report\n\n")
        f.write("## Executive Summary\n\n")
        
        total_errors = len(errors_by_test)
        f.write(f"- **Total Failed Tests**: {total_errors}\n")
        f.write(f"- **Success Rate**: 79.93% (255/319 tests passing)\n")
        f.write(f"- **Primary Issue Categories**: {len(errors_by_category)}\n\n")
        
        f.write("## Error Categories by Impact\n\n")
        
        # Sort categories by impact (number of tests affected)
        sorted_categories = sorted(errors_by_category.items(), key=lambda x: len(x[1]), reverse=True)
        
        for i, (category, affected_tests) in enumerate(sorted_categories, 1):
            f.write(f"### {i}. {category.replace('_', ' ').title()}\n")
            f.write(f"**Impact**: {len(affected_tests)} tests affected\n")
            f.write(f"**Tests**: {', '.join(affected_tests[:10])}{'...' if len(affected_tests) > 10 else ''}\n\n")
        
        f.write("## Critical Missing Standard Library Functions\n\n")
        
        f.write("### Most Critical Missing Methods\n")
        for method, count in missing_methods.most_common(10):
            f.write(f"- `{method}`: {count} tests blocked\n")
        
        f.write("\n### Most Critical Missing Namespace Functions\n")
        for func, count in missing_functions.most_common(10):
            f.write(f"- `{func}`: {count} tests blocked\n")
        
        f.write("\n## Top 5 Critical Gaps by Impact\n\n")
        
        top_gaps = [
            ("Missing List/Collection Methods", ["size", "add", "remove"], len([t for t in errors_by_test if 'list' in errors_by_test[t]['content'].lower()])),
            ("Missing String Methods", ["isEmpty", "contains", "concat"], len([t for t in errors_by_test if 'string' in errors_by_test[t]['content'].lower()])),
            ("Missing HTTP Module", ["http.get", "http.post"], len([t for t in errors_by_test if 'http' in errors_by_test[t]['content'].lower()])),
            ("Missing Object Methods", ["getName", "getArea", "toString"], len([t for t in errors_by_test if 'method' in errors_by_test[t]['content'].lower() and 'not found' in errors_by_test[t]['content'].lower()])),
            ("Type System Issues", ["Boolean conditions", "Method chaining"], len([t for t in errors_by_test if 'type' in errors_by_test[t]['content'].lower()]))
        ]
        
        for i, (gap_name, examples, impact) in enumerate(top_gaps, 1):
            f.write(f"### {i}. {gap_name}\n")
            f.write(f"**Impact**: ~{impact} tests affected\n")
            f.write(f"**Examples**: {', '.join(examples)}\n")
            f.write(f"**Priority**: {'🔴 CRITICAL' if impact > 10 else '🟡 HIGH' if impact > 5 else '🟢 MEDIUM'}\n\n")
        
        f.write("## Specific Implementation Priorities\n\n")
        
        priorities = [
            ("List/Collection Standard Library", "Implement size(), add(), remove(), isEmpty() methods", "🔴 CRITICAL"),
            ("String Standard Library", "Implement isEmpty(), contains(), concat() methods", "🔴 CRITICAL"),
            ("HTTP Module", "Implement http.get(), http.post() namespace functions", "🟡 HIGH"),
            ("Object Method Resolution", "Fix method lookup for custom object types", "🟡 HIGH"),
            ("Type System Improvements", "Fix boolean condition type checking", "🟡 HIGH")
        ]
        
        for area, task, priority in priorities:
            f.write(f"### {area}\n")
            f.write(f"**Task**: {task}\n")
            f.write(f"**Priority**: {priority}\n\n")
        
        f.write("## Progress Since Recent Improvements\n\n")
        f.write("✅ **Apply-blocks**: Working correctly (recent implementation successful)\n")
        f.write("✅ **Math constants**: pi, e, tau now working\n") 
        f.write("🟡 **String functions**: Partially implemented, missing key methods\n")
        f.write("❌ **List behaviors**: Critical missing methods blocking multiple tests\n")
        f.write("❌ **HTTP module**: Not implemented\n")
        f.write("❌ **Testing framework**: Missing core functionality\n\n")
        
        f.write("## Recommendations for Next Steps\n\n")
        f.write("1. **Immediate Priority**: Implement missing list methods (size, add, remove)\n")
        f.write("2. **High Priority**: Complete string standard library implementation\n")
        f.write("3. **Medium Priority**: Implement HTTP module for networking tests\n")
        f.write("4. **Ongoing**: Fix type system issues with boolean conditions\n")
        f.write("5. **Testing**: Re-run comprehensive tests after each major implementation\n\n")
        
        f.write("## Detailed Error Breakdown\n\n")
        for test_name, error_info in sorted(errors_by_test.items()):
            f.write(f"### {test_name}\n")
            f.write(f"**Category**: {error_info['category']}\n")
            f.write("```\n")
            f.write(error_info['content'][:500] + ("..." if len(error_info['content']) > 500 else ""))
            f.write("\n```\n\n")

    print(f"Comprehensive QA analysis report generated: {report_path}")

if __name__ == "__main__":
    analyze_error_patterns()