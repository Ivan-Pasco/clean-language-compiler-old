#!/usr/bin/env python3
"""
Syntax Compliance Validator for Clean Language Compiler QA
Validates test files against the Language Specification to ensure correct syntax
"""

import re
import sys
import json
from pathlib import Path
from typing import Dict, List, Tuple, Optional
from dataclasses import dataclass

@dataclass
class SyntaxRule:
    name: str
    pattern: str
    description: str
    examples: List[str]
    violations: List[str]

# Define syntax rules based on Clean Language specification
SYNTAX_RULES = [
    # Function definitions
    SyntaxRule(
        name="function_definition",
        pattern=r"^\s*\w+\s+\w+\([^)]*\)\s*$",
        description="Function definitions must have return type, name, and parameters",
        examples=["integer add(integer a, integer b)", "string getName()", "void process()"],
        violations=["add(a, b)", "integer add", "add(integer a, integer b) ->"]
    ),
    
    # Variable declarations
    SyntaxRule(
        name="variable_declaration",
        pattern=r"^\s*\w+\s+\w+\s*=\s*.+$",
        description="Variable declarations must have type, name, and initialization",
        examples=["integer x = 5", "string name = \"test\"", "boolean flag = true"],
        violations=["x = 5", "integer x", "var x = 5"]
    ),
    
    # Control flow statements
    SyntaxRule(
        name="if_statement",
        pattern=r"^\s*if\s+.+$",
        description="If statements must have condition and proper indentation",
        examples=["if x > 0", "if name == \"test\"", "if flag && other"],
        violations=["if (x > 0)", "if x > 0 {", "if x > 0:"]
    ),
    
    # Function calls
    SyntaxRule(
        name="function_call",
        pattern=r"^\s*\w+\([^)]*\)\s*$",
        description="Function calls must have name and parentheses",
        examples=["print(\"hello\")", "calculate(a, b)", "process()"],
        violations=["print \"hello\"", "calculate a b", "process"]
    ),
    
    # Method calls
    SyntaxRule(
        name="method_call",
        pattern=r"^\s*\w+\.\w+\([^)]*\)\s*$",
        description="Method calls must use dot notation with parentheses",
        examples=["string.length()", "array.push(item)", "object.method()"],
        violations=["string::length()", "array push item", "object.method"]
    ),
    
    # Class definitions
    SyntaxRule(
        name="class_definition",
        pattern=r"^\s*class\s+\w+\s*(\([\w\s,]*\))?\s*$",
        description="Class definitions must have 'class' keyword and name",
        examples=["class Person", "class Car(Vehicle)", "class Calculator"],
        violations=["Class Person", "class person {", "class Person extends Vehicle"]
    ),
    
    # Import statements
    SyntaxRule(
        name="import_statement", 
        pattern=r"^\s*import\s+[\w.]+(\s+as\s+\w+)?\s*$",
        description="Import statements must use 'import' keyword",
        examples=["import math", "import http.client", "import utils as u"],
        violations=["from math import *", "include math", "#import math"]
    ),
    
    # Block structure (indentation)
    SyntaxRule(
        name="block_indentation",
        pattern=r"^\t+\w+.*$",
        description="Code blocks must use tab indentation",
        examples=["\treturn x + y", "\tprint \"hello\"", "\tif condition"],
        violations=["    return x + y", "  print \"hello\"", " if condition"]
    ),
    
    # Print statements
    SyntaxRule(
        name="print_statement",
        pattern=r"^\s*print\s+.*(\s+ln)?\s*$",
        description="Print statements must use 'print' keyword, optional 'ln' suffix",
        examples=["print \"hello\"", "print value ln", "print result"],
        violations=["println(\"hello\")", "console.log(value)", "printf(\"hello\")"]
    ),
    
    # String literals
    SyntaxRule(
        name="string_literal",
        pattern=r'"[^"]*"',
        description="String literals must use double quotes",
        examples=["\"hello\"", "\"test string\"", "\"\""],
        violations=["'hello'", "`hello`", "hello"]
    )
]

class SyntaxValidator:
    def __init__(self, spec_file: Optional[str] = None):
        self.violations = []
        self.file_stats = {}
        self.spec_file = spec_file
        
    def validate_file(self, file_path: Path) -> Dict:
        """Validate a single .cln file against syntax rules"""
        violations = []
        line_count = 0
        
        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                lines = f.readlines()
                
            for line_num, line in enumerate(lines, 1):
                line_count += 1
                stripped = line.strip()
                
                # Skip empty lines and comments
                if not stripped or stripped.startswith('//'):
                    continue
                    
                # Check each syntax rule
                violations.extend(self._check_line_syntax(line, line_num, file_path))
                
        except Exception as e:
            violations.append({
                'rule': 'file_access',
                'line': 0,
                'issue': f"Cannot read file: {e}",
                'suggestion': "Ensure file exists and is readable"
            })
            
        return {
            'file': str(file_path),
            'lines': line_count,
            'violations': violations,
            'violation_count': len(violations)
        }
    
    def _check_line_syntax(self, line: str, line_num: int, file_path: Path) -> List[Dict]:
        """Check a single line against all syntax rules"""
        violations = []
        stripped = line.strip()
        
        # Check for common syntax issues
        if stripped:
            # Check for incorrect indentation (spaces instead of tabs)
            if line.startswith(' ') and not line.startswith('\t'):
                violations.append({
                    'rule': 'indentation',
                    'line': line_num,
                    'issue': 'Using spaces for indentation instead of tabs',
                    'suggestion': 'Use tab characters for indentation',
                    'content': line.rstrip()
                })
            
            # Check for unsupported syntax patterns
            unsupported_patterns = [
                (r'^\s*var\s+\w+', 'Use typed variable declarations instead of "var"'),
                (r'^\s*let\s+\w+', 'Use typed variable declarations instead of "let"'),
                (r'^\s*const\s+\w+', 'Use typed variable declarations instead of "const"'),
                (r'function\s+\w+', 'Use return-type-first function syntax'),
                (r'def\s+\w+', 'Use return-type-first function syntax'),
                (r'printf?\s*\(', 'Use "print" statement instead of printf'),
                (r'console\.log', 'Use "print" statement instead of console.log'),
                (r'System\.out', 'Use "print" statement instead of System.out'),
                (r'std::cout', 'Use "print" statement instead of std::cout'),
                (r'\'[^\']*\'', 'Use double quotes for strings instead of single quotes'),
                (r'\{[^}]*\}', 'Use indentation-based blocks instead of braces'),
            ]
            
            for pattern, suggestion in unsupported_patterns:
                if re.search(pattern, stripped):
                    violations.append({
                        'rule': 'unsupported_syntax',
                        'line': line_num,
                        'issue': f'Unsupported syntax pattern: {pattern}',
                        'suggestion': suggestion,
                        'content': line.rstrip()
                    })
        
        return violations
    
    def validate_directory(self, directory: Path) -> Dict:
        """Validate all .cln files in a directory"""
        cln_files = list(directory.glob('**/*.cln'))
        
        if not cln_files:
            return {
                'directory': str(directory),
                'files_found': 0,
                'total_violations': 0,
                'files': []
            }
        
        all_results = []
        total_violations = 0
        
        for cln_file in sorted(cln_files):
            result = self.validate_file(cln_file)
            all_results.append(result)
            total_violations += result['violation_count']
            
        return {
            'directory': str(directory),
            'files_found': len(cln_files),
            'total_violations': total_violations,
            'files': all_results
        }

def generate_compliance_report(validation_results: Dict, output_file: str = None) -> str:
    """Generate markdown compliance report"""
    
    report = []
    report.append("# Syntax Compliance Validation Report")
    report.append(f"*Generated by: {Path(__file__).name}*\n")
    
    # Summary
    files_count = validation_results['files_found']
    violations_count = validation_results['total_violations']
    clean_files = sum(1 for f in validation_results['files'] if f['violation_count'] == 0)
    
    report.append("## Summary")
    report.append(f"- **Files Validated**: {files_count}")
    report.append(f"- **Total Violations**: {violations_count}")
    report.append(f"- **Clean Files**: {clean_files}/{files_count} ({(clean_files/files_count*100):.1f}%)")
    report.append(f"- **Files with Issues**: {files_count - clean_files}")
    report.append("")
    
    # Files with violations
    files_with_issues = [f for f in validation_results['files'] if f['violation_count'] > 0]
    
    if files_with_issues:
        report.append("## Files with Syntax Issues")
        
        # Sort by violation count (most issues first)
        files_with_issues.sort(key=lambda x: x['violation_count'], reverse=True)
        
        for file_result in files_with_issues[:20]:  # Show top 20 files with issues
            file_name = Path(file_result['file']).name
            violation_count = file_result['violation_count']
            
            report.append(f"### {file_name}")
            report.append(f"**Violations**: {violation_count}")
            report.append("")
            
            # Group violations by rule
            violations_by_rule = {}
            for violation in file_result['violations']:
                rule = violation['rule']
                if rule not in violations_by_rule:
                    violations_by_rule[rule] = []
                violations_by_rule[rule].append(violation)
            
            for rule, rule_violations in violations_by_rule.items():
                report.append(f"#### {rule.title().replace('_', ' ')}")
                for violation in rule_violations[:5]:  # Show first 5 violations per rule
                    report.append(f"- **Line {violation['line']}**: {violation['issue']}")
                    if 'suggestion' in violation:
                        report.append(f"  - *Suggestion*: {violation['suggestion']}")
                    if 'content' in violation:
                        report.append(f"  - *Code*: `{violation['content']}`")
                
                if len(rule_violations) > 5:
                    report.append(f"  - *...and {len(rule_violations) - 5} more {rule} violations*")
                report.append("")
    
    # Most common violations
    all_violations = []
    for file_result in validation_results['files']:
        all_violations.extend(file_result['violations'])
    
    if all_violations:
        violation_counts = {}
        for violation in all_violations:
            rule = violation['rule']
            violation_counts[rule] = violation_counts.get(rule, 0) + 1
        
        report.append("## Most Common Syntax Issues")
        sorted_violations = sorted(violation_counts.items(), key=lambda x: x[1], reverse=True)
        
        for rule, count in sorted_violations[:10]:
            report.append(f"- **{rule.replace('_', ' ').title()}**: {count} occurrences")
        report.append("")
    
    # Recommendations
    report.append("## Recommendations")
    
    if violations_count == 0:
        report.append("🎉 **Excellent!** All test files comply with Clean Language syntax rules.")
    elif violations_count < 10:
        report.append("✅ **Good compliance** with minor syntax issues to address:")
        report.append("1. Fix the remaining syntax violations")
        report.append("2. Consider adding syntax linting to prevent future issues")
    else:
        report.append("⚠️ **Multiple syntax issues** found. Recommended actions:")
        report.append("1. **Fix indentation issues** - use tabs instead of spaces")
        report.append("2. **Update variable declarations** - use typed declarations")
        report.append("3. **Convert function syntax** - use Clean Language function format")
        report.append("4. **Fix string literals** - use double quotes consistently")
        report.append("5. **Remove unsupported syntax** - follow Clean Language specification")
    
    report.append("")
    report.append("## Next Steps")
    report.append("1. **Fix Priority Issues**: Address files with most violations first")
    report.append("2. **Update Test Files**: Ensure compliance with Language-Specification.md")
    report.append("3. **Verify Fixes**: Re-run this validator after making changes")
    report.append("4. **Run Compiler Tests**: Test corrected files with the compiler")
    
    report_text = '\n'.join(report)
    
    if output_file:
        with open(output_file, 'w') as f:
            f.write(report_text)
        print(f"Compliance report saved to: {output_file}")
    
    return report_text

def main():
    """Main entry point for syntax validation"""
    if len(sys.argv) < 2:
        print("Usage: python3 validate_syntax_compliance.py <test_directory> [output_report.md]")
        print("Example: python3 validate_syntax_compliance.py tests/clean_files/")
        sys.exit(1)
    
    test_dir = Path(sys.argv[1])
    if not test_dir.exists():
        print(f"Error: Directory '{test_dir}' does not exist")
        sys.exit(1)
    
    print(f"🔍 Validating syntax compliance in: {test_dir}")
    
    # Run validation
    validator = SyntaxValidator()
    results = validator.validate_directory(test_dir)
    
    # Generate report
    output_file = sys.argv[2] if len(sys.argv) > 2 else None
    report = generate_compliance_report(results, output_file)
    
    # Print summary
    print("\n" + "="*60)
    print("SYNTAX COMPLIANCE SUMMARY")
    print("="*60)
    print(f"Files Validated: {results['files_found']}")
    print(f"Total Violations: {results['total_violations']}")
    
    clean_files = sum(1 for f in results['files'] if f['violation_count'] == 0)
    compliance_rate = (clean_files / results['files_found'] * 100) if results['files_found'] > 0 else 0
    
    if compliance_rate == 100:
        print(f"🎉 Compliance Rate: {compliance_rate:.1f}% - All files compliant!")
    elif compliance_rate >= 90:
        print(f"✅ Compliance Rate: {compliance_rate:.1f}% - Excellent")
    elif compliance_rate >= 75:
        print(f"🟡 Compliance Rate: {compliance_rate:.1f}% - Good")
    else:
        print(f"🔴 Compliance Rate: {compliance_rate:.1f}% - Needs improvement")
    
    if not output_file and results['total_violations'] < 50:
        print("\nDetailed report:")
        print(report)
    elif not output_file:
        print("\nToo many violations to display. Use output file option for full report.")

if __name__ == "__main__":
    main()