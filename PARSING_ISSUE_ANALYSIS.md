# Critical Parsing Issue Analysis

## Problem
The Clean Language parser incorrectly extracts class methods from their class context, resulting in:
- Classes with functions: blocks being parsed as 0 classes + N standalone functions
- Loss of class member variable scope in methods
- 12 failing tests with "Variable 'name' not found" errors

## Root Cause
Grammar rule precedence in `program_item`: functions_block matches before class_decl can complete parsing, causing class methods to be extracted as standalone functions.

## Evidence
- Simple class without functions: "0 functions, 1 classes" ✓
- Class with functions block: "4 functions, 0 classes" ✗
- Expected for 14_classes_basic.cln: "0 functions, 1 class with 4 methods"

## Affected Files
All tests with classes containing functions: blocks (14-16, 33-36)

## Temporary Fix
Implemented semantic analyzer workaround to infer class context and inject member variables into scope.

## Long-term Solution
Fix grammar parser to properly handle nested functions: blocks within class declarations.