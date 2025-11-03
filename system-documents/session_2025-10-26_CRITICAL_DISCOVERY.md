# Session 2025-10-26: CRITICAL DISCOVERY - Method Bodies Not Using Standard Variable Processing

## Date
2025-10-26 (Continuation - Critical Discovery)

## CRITICAL FINDING

**The variables `name` and `age` in method bodies are NEVER processed through the standard Variable expression handler!**

### Debug Output Shows:

```
DEBUG MIR VAR: Processing variable 'animal'   (in start function)
DEBUG MIR VAR: Processing variable 'this'     (multiple times in class context)
```

**MISSING**: No processing of `name` or `age` variables!

### What This Means

1. **The methods `getName()` and `getAge()` are NOT being built through standard expression processing**
2. **Method bodies with `return name` and `return age` are being handled differently**
3. **The fix I attempted (adding field access to Variable expression) is irrelevant**
4. **The real issue is in HOW class methods are generated in the first place**

## Source Code Context

**Test File**: `tests/cln/language/classes/07_class_definitions.cln`

```clean
class Animal
	string name
	integer age

	constructor(string name, integer age)
		name = name
		age = age

	functions:
		string getName()
			return name  // <-- This is NOT being processed as a Variable!

		integer getAge()
			return age   // <-- This is NOT being processed as a Variable!
```

## The Real Bug

**Hypothesis**: Class methods are likely being **pre-generated** or **synthesized** somewhere in the compiler pipeline BEFORE reaching the MIR builder.

**Possibilities**:
1. Methods are generated as **stub/template code** during semantic analysis
2. Methods are **synthesized during AST/TAST generation**
3. There's **special handling for simple getter methods**
4. Method bodies are **pre-compiled** with field references already resolved to incorrect ValueIds

## Where To Look Next

### Search for Method Generation Code
```bash
# Search for getter/accessor generation
grep -r "getName\|getAge\|getter" src/

# Search for method synthesis
grep -r "method.*generat\|synthesize.*method" src/

# Search for class method compilation
grep -r "class.*method\|method.*class" src/mir/
```

### Key Files to Investigate
1. **`src/semantic/`** - Semantic analysis might be pre-processing methods
2. **`src/hir/hir_builder.rs`** - HIR might have method generation logic
3. **`src/typechecker/`** - Type checker might be synthesizing method bodies
4. **`src/parser/class_parser.rs`** (if exists) - Parser might handle methods specially

## Error Analysis

**Original Error**:
```
ValueId(1) not found in local variable map during load_operand
```

**What This Really Means**:
- The MIR for getName/getAge methods contains a reference to ValueId(1)
- This ValueId was created WITHOUT generating any MIR instructions
- The codegen phase can't find ValueId(1) in the local variable map
- **This ValueId(1) was likely created during method pre-processing/synthesis**

## Next Investigation Steps

1. **Dump the TAST** for the class methods to see what expressions they contain
2. **Add debug logging in TAST→MIR conversion** for method bodies
3. **Search for where method bodies are being created/modified**
4. **Find where the invalid ValueId(1) is being assigned**

## Expected Root Cause

**The compiler is likely generating method bodies automatically for simple getters**, and this automatic generation is:
1. Creating a ValueId for the field access
2. NOT generating the necessary MIR instructions to load the field
3. Leaving a dangling ValueId that codegen can't resolve

## Files Already Modified (Can Be Reverted)

- `src/mir/mir_builder.rs:1083-1087` - Added debug logging (helpful)
- `src/mir/mir_builder.rs:1113-1171` - Added field access handling (NOT USED!)
- `src/codegen/mir_codegen.rs:198-227` - Added debug logging (helpful)

## Success Criteria for Next Session

1. Find where method bodies are being processed/generated
2. Identify where ValueId(1) is created for field references
3. Fix the root cause (either stop auto-generation or make it generate proper MIR)
4. Verify all 4 functions (constructor, getName, getAge, start) generate successfully
5. WASM validates
6. 19 class-related test files pass validation (73% → 79%)

## Key Insight

**The bug is NOT in expression handling - it's in method generation/synthesis!**

This explains why:
- My field access fix didn't help
- No "DEBUG MIR VAR" output for `name` and `age`
- The error occurs during codegen, not MIR building

The method bodies are already broken when they reach the MIR builder.
