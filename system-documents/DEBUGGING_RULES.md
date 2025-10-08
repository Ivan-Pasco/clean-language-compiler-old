# PERMANENT DEBUGGING RULES FOR CLEAN LANGUAGE COMPILER

## ❌ NEVER MODIFY TEST FILES (.cln) WHEN DEBUGGING

### Critical Rule: Test File Integrity
**NEVER change a .cln test file because it is failing compilation without first:**

1. **Verifying against Clean Language Specification** - Check if the syntax is valid according to `documentation/Clean_Language_Specification.md`
2. **Analyzing Intermediate Representations** - Examine AST, HIR, MIR, and WASM generation stages
3. **Implementing Missing Features** - Add support for the language construct in the compiler
4. **Only modify tests if they violate the specification** - Test files should be specification-compliant

### Proper Debugging Workflow

1. **Parse Error Analysis**
   - Check if syntax is valid per specification
   - Identify missing grammar rules in `src/parser/grammar.pest`
   - Implement missing parsing logic

2. **Semantic Error Analysis**
   - Check HIR and semantic analysis stages
   - Implement missing semantic rules
   - Add type checking for new constructs

3. **Code Generation Issues**
   - Analyze MIR and WASM generation
   - Implement missing code generation patterns
   - Ensure proper WASM output

4. **Only Modify Tests When**
   - Test syntax violates Clean Language Specification
   - Test contains typos or malformed syntax
   - Test uses deprecated syntax that was removed from specification

### Implementation Priority
- Fix compiler to support specification-compliant code
- Implement missing language features
- Extend parser, semantic analyzer, and code generator
- Maintain test file integrity as specification examples

**The goal is 100% compiler accuracy, not 100% test compliance through modification.**