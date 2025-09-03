#!/usr/bin/env python3
"""
Error Categorization Script for Clean Language Compiler QA
Automatically analyzes test output and categorizes errors by impact and frequency
"""

import re
import sys
import json
from collections import defaultdict
from dataclasses import dataclass
from typing import Dict, List, Set
from pathlib import Path

@dataclass
class ErrorPattern:
    pattern: str
    category: str
    priority: str
    description: str
    suggested_fix: str

# Define error patterns with their classifications
ERROR_PATTERNS = [
    # CRITICAL - Blocks compilation
    ErrorPattern(
        pattern=r"non-exhaustive patterns",
        category="COMPILATION",
        priority="CRITICAL",
        description="Missing pattern matches in AST handling",
        suggested_fix="Add all missing Type/Expression/Statement variants to match statements"
    ),
    ErrorPattern(
        pattern=r"cannot find function `.*` in this scope",
        category="COMPILATION", 
        priority="CRITICAL",
        description="Missing function implementations in compiler",
        suggested_fix="Implement missing functions in appropriate modules"
    ),
    
    # HIGH - Affects many tests
    ErrorPattern(
        pattern=r"Undefined variable: (\w+)",
        category="SEMANTIC",
        priority="HIGH",
        description="Missing namespace or variable definitions",
        suggested_fix="Add namespace support in semantic analyzer"
    ),
    ErrorPattern(
        pattern=r"(\w+) literals? not yet implemented",
        category="CODEGEN",
        priority="HIGH", 
        description="Missing code generation for language features",
        suggested_fix="Implement full codegen for the missing feature type"
    ),
    ErrorPattern(
        pattern=r"Method calls? not yet implemented",
        category="CODEGEN",
        priority="HIGH",
        description="Missing method call code generation",
        suggested_fix="Implement method call expression handling in codegen"
    ),
    ErrorPattern(
        pattern=r"Unsupported (\w+) type: (\w+)",
        category="CODEGEN",
        priority="HIGH",
        description="Missing handlers for AST node types",
        suggested_fix="Add case for the unsupported type in appropriate generator"
    ),
    
    # MEDIUM - Affects specific features
    ErrorPattern(
        pattern=r"String interpolation not supported",
        category="CODEGEN",
        priority="MEDIUM",
        description="Missing string interpolation implementation",
        suggested_fix="Implement string interpolation expression generation"
    ),
    ErrorPattern(
        pattern=r"Function not found: (\w+)",
        category="SEMANTIC",
        priority="MEDIUM", 
        description="Missing function definitions or resolution issues",
        suggested_fix="Add function to symbol table or fix resolution logic"
    ),
    ErrorPattern(
        pattern=r"Type mismatch",
        category="SEMANTIC",
        priority="MEDIUM",
        description="Type checking and inference issues",
        suggested_fix="Review type inference logic and add missing type conversions"
    )
]

class ErrorAnalyzer:
    def __init__(self):
        self.error_counts = defaultdict(int)
        self.error_details = defaultdict(list)
        self.test_failures = defaultdict(set)
        
    def analyze_output(self, test_output: str) -> Dict:
        """Analyze test output and categorize all errors found"""
        lines = test_output.split('\n')
        current_test = None
        
        for line in lines:
            # Track current test being processed
            test_match = re.search(r'Testing: (.*\.cln)', line)
            if test_match:
                current_test = test_match.group(1)
                continue
                
            # Analyze each line for error patterns
            for pattern in ERROR_PATTERNS:
                match = re.search(pattern.pattern, line, re.IGNORECASE)
                if match:
                    error_key = f"{pattern.category}_{pattern.priority}"
                    self.error_counts[error_key] += 1
                    
                    error_detail = {
                        'pattern': pattern.pattern,
                        'category': pattern.category,
                        'priority': pattern.priority,
                        'description': pattern.description,
                        'suggested_fix': pattern.suggested_fix,
                        'match': match.group(0),
                        'line': line.strip(),
                        'test_file': current_test
                    }
                    
                    self.error_details[error_key].append(error_detail)
                    
                    if current_test:
                        self.test_failures[pattern.priority].add(current_test)
        
        return self._generate_analysis_report()
    
    def _generate_analysis_report(self) -> Dict:
        """Generate comprehensive analysis report"""
        
        # Calculate impact metrics
        critical_tests = len(self.test_failures['CRITICAL'])
        high_tests = len(self.test_failures['HIGH'])  
        medium_tests = len(self.test_failures['MEDIUM'])
        
        # Group errors by priority
        priority_summary = {
            'CRITICAL': {
                'count': sum(count for key, count in self.error_counts.items() if 'CRITICAL' in key),
                'tests_affected': critical_tests,
                'errors': [details for key, details in self.error_details.items() if 'CRITICAL' in key]
            },
            'HIGH': {
                'count': sum(count for key, count in self.error_counts.items() if 'HIGH' in key),
                'tests_affected': high_tests,
                'errors': [details for key, details in self.error_details.items() if 'HIGH' in key]
            },
            'MEDIUM': {
                'count': sum(count for key, count in self.error_counts.items() if 'MEDIUM' in key),
                'tests_affected': medium_tests,
                'errors': [details for key, details in self.error_details.items() if 'MEDIUM' in key]
            }
        }
        
        # Find most frequent error types
        frequent_errors = sorted(self.error_counts.items(), key=lambda x: x[1], reverse=True)[:10]
        
        return {
            'summary': {
                'total_errors': sum(self.error_counts.values()),
                'unique_patterns': len(self.error_counts),
                'critical_count': priority_summary['CRITICAL']['count'],
                'high_count': priority_summary['HIGH']['count'],
                'medium_count': priority_summary['MEDIUM']['count'],
                'tests_with_critical': critical_tests,
                'tests_with_high': high_tests,
                'tests_with_medium': medium_tests
            },
            'priority_breakdown': priority_summary,
            'frequent_errors': frequent_errors,
            'all_errors': dict(self.error_details)
        }

def generate_markdown_report(analysis: Dict, output_file: str = None) -> str:
    """Generate a markdown report from error analysis"""
    
    report = []
    report.append("# Error Analysis Report")
    report.append(f"*Generated: {Path(__file__).name}*\n")
    
    # Summary section
    summary = analysis['summary']
    report.append("## Summary")
    report.append(f"- **Total Errors Found**: {summary['total_errors']}")
    report.append(f"- **Unique Error Patterns**: {summary['unique_patterns']}")
    report.append(f"- **Critical Issues**: {summary['critical_count']} (affecting {summary['tests_with_critical']} tests)")
    report.append(f"- **High Priority Issues**: {summary['high_count']} (affecting {summary['tests_with_high']} tests)")
    report.append(f"- **Medium Priority Issues**: {summary['medium_count']} (affecting {summary['tests_with_medium']} tests)")
    report.append("")
    
    # Priority breakdown
    for priority in ['CRITICAL', 'HIGH', 'MEDIUM']:
        priority_data = analysis['priority_breakdown'][priority]
        icon = '🔴' if priority == 'CRITICAL' else '🟡' if priority == 'HIGH' else '🟢'
        
        report.append(f"## {icon} {priority} Priority Issues")
        report.append(f"**Count**: {priority_data['count']} errors affecting {priority_data['tests_affected']} tests\n")
        
        # Show error details for this priority
        for error_group in priority_data['errors']:
            if error_group:  # Skip empty groups
                for error in error_group[:3]:  # Show top 3 examples
                    report.append(f"### {error['description']}")
                    report.append(f"- **Pattern**: `{error['match']}`")
                    report.append(f"- **Category**: {error['category']}")
                    report.append(f"- **Suggested Fix**: {error['suggested_fix']}")
                    if error['test_file']:
                        report.append(f"- **Example Test**: {error['test_file']}")
                    report.append("")
    
    # Most frequent errors
    report.append("## Most Frequent Error Types")
    for error_type, count in analysis['frequent_errors']:
        report.append(f"- **{error_type}**: {count} occurrences")
    report.append("")
    
    # Recommendations
    report.append("## Recommended Fix Order")
    report.append("1. **Fix CRITICAL issues first** - these block compilation and prevent all testing")
    report.append("2. **Address HIGH priority by test count** - focus on errors affecting most tests")  
    report.append("3. **Implement MEDIUM priority features** - complete remaining functionality")
    report.append("4. **Validate fixes** - re-run analysis after each batch of fixes")
    
    report_text = '\n'.join(report)
    
    if output_file:
        with open(output_file, 'w') as f:
            f.write(report_text)
        print(f"Report saved to: {output_file}")
    
    return report_text

def main():
    """Main entry point for error analysis"""
    if len(sys.argv) < 2:
        print("Usage: python3 categorize_errors.py <test_output_file> [output_report.md]")
        print("   or: python3 categorize_errors.py - (read from stdin)")
        sys.exit(1)
    
    # Read input
    if sys.argv[1] == '-':
        test_output = sys.stdin.read()
    else:
        try:
            with open(sys.argv[1], 'r') as f:
                test_output = f.read()
        except FileNotFoundError:
            print(f"Error: File '{sys.argv[1]}' not found")
            sys.exit(1)
    
    # Analyze errors
    analyzer = ErrorAnalyzer()
    analysis = analyzer.analyze_output(test_output)
    
    # Generate report
    output_file = sys.argv[2] if len(sys.argv) > 2 else None
    report = generate_markdown_report(analysis, output_file)
    
    # Print summary to console
    print("\n" + "="*50)
    print("ERROR ANALYSIS SUMMARY") 
    print("="*50)
    summary = analysis['summary']
    print(f"Total Errors: {summary['total_errors']}")
    print(f"🔴 Critical: {summary['critical_count']} (blocks {summary['tests_with_critical']} tests)")
    print(f"🟡 High: {summary['high_count']} (affects {summary['tests_with_high']} tests)")
    print(f"🟢 Medium: {summary['medium_count']} (affects {summary['tests_with_medium']} tests)")
    
    if not output_file:
        print("\nFull report:")
        print(report)

if __name__ == "__main__":
    main()