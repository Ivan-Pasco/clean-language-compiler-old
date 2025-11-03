# Session Continued: October 31, 2025 - Base() Call Investigation

## Problem Investigation

Investigating why constructor calls with inheritance fail WASM validation with error:
```
error: type mismatch in call, expected [i32, i32, i32, i32] but got [i32, i32, i32]
```

## Root Cause Found

After extensive investigation through the compilation pipeline:

### Evidence Trail:
1. ✅ TastExpressionKind::BaseCall EXISTS in TAST (line 244 of tast.rs)
2. ✅ Type inference HANDLES BaseCall (line 2313-2334 of type_inference.rs)  
3. ✅ Resolver HANDLES BaseCall (line 1732-1776 of resolver_impl.rs)
4. ✅ MIR builder HANDLES BaseCall (line 2194-2280 of mir_builder.rs)
5. ✅ HIR has BaseCall variant (line 342 of hir/mod.rs)
6. ❌ HIR builder DOES NOT handle base() calls - **NO CODE EXISTS**

### The Bug:
**The HIR builder (hir_builder.rs) has NO handling for base() function calls.**

When parsing `base(xParam, yParam)`:
- Parser correctly identifies it as a function call
- HIR builder treats it as a regular function call (not BaseCall)
- The function "base" doesn't exist as a regular function
- Result: base() call is probably converted to a null/void literal or error

### Why This Explains Everything:
- Debug output shows base() as "Discriminant(0)" = Literal
- No "DEBUG MIR BASECALL" output appears (BaseCall handler never runs)
- ColoredPoint shows "Class has 1 fields" (only its own `color`, not inherited `x`,`y`)
- No actual base constructor call is generated in MIR

## Next Steps

Need to add base() call handling to HIR builder:
1. Detect when function call is specifically "base(...)"
2. Convert to HirExpression::BaseCall instead of regular FunctionCall
3. Pass empty parent_class_symbol_id (resolver will fill it in)

## Status
Investigation complete - bug identified. Ready to implement fix.
