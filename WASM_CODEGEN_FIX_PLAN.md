# WASM Codegen Fix Plan - Root Cause Analysis

## Critical Finding: String Representation Mismatch

### The Problem

**Root Cause:** Strings are represented as single pointers `Ptr(U8)` in MIR, but WebAssembly operations require (pointer, length) pairs.

### Evidence

1. **Print function signature** (correctly implemented):
   ```rust
   // From register_print_imports():
   let print_type = self.add_function_type(&[WasmType::I32, WasmType::I32], None)?;
   // Signature: (i32, i32) -> void
   ```

2. **User-defined string functions** (incorrectly implemented):
   ```clean
   string fetchData(string url)
       return "Data from " + url
   ```

   Should generate: `(param i32 i32) (result i32 i32)`
   Actually generates: `(param i32) (result i32)` ← **WRONG**

3. **WASM Validation Errors:**
   ```
   error: type mismatch in call, expected [i32, i32] but got [i32]
   error: type mismatch in local.set, expected [i32] but got []
   error: type mismatch in return, expected [i32] but got []
   ```

### Why 88% of Tests Fail

**All tests using string functions fail because:**
- String parameters don't pass length
- String returns don't return length
- String operations (concatenation) don't produce/consume pairs
- Function calls to user-defined functions have wrong signatures

### The Fix Strategy

Based on **WebAssembly best practices** and **context7 documentation**:

#### Phase 1: Fix MIR String Representation
**Target:** Make MIR properly represent strings as (ptr, len) tuples

1. **Create String Tuple Type in MIR:**
   ```rust
   // In mir_types.rs
   MirType::StringTuple { ptr_type: Box<MirType>, len_type: MirType }
   ```

2. **Update from_concrete_type:**
   ```rust
   ConcreteType::String => MirType::StringTuple {
       ptr_type: Box::new(MirType::Ptr(Box::new(MirType::U8))),
       len_type: MirType::I32
   }
   ```

#### Phase 2: Fix Function Signatures
**Target:** Generate correct WASM signatures for string functions

1. **Update convert_function_signature in mir_codegen.rs:**
   ```rust
   fn convert_function_signature(&self, function: &MirFunction) -> Result<(Vec<ValType>, Vec<ValType>), CompilerError> {
       let mut param_types = Vec::new();
       for param in &function.parameters {
           match &param.param_type {
               MirType::StringTuple { .. } => {
                   // String parameters expand to (ptr, len)
                   param_types.push(ValType::I32);
                   param_types.push(ValType::I32);
               }
               _ => param_types.push(self.mir_type_to_wasm_type(&param.param_type)?),
           }
       }

       let mut result_types = Vec::new();
       match &function.return_type {
           MirType::StringTuple { .. } => {
               // String returns use multi-value
               result_types.push(ValType::I32);
               result_types.push(ValType::I32);
           }
           MirType::Void => {}, // No return
           _ => result_types.push(self.mir_type_to_wasm_type(&function.return_type)?),
       }

       Ok((param_types, result_types))
   }
   ```

2. **Example Generated WASM:**
   ```wat
   (func $fetchData (param $url_ptr i32) (param $url_len i32) (result i32 i32)
     ;; Function body
   )
   ```

#### Phase 3: Fix Function Calls
**Target:** Make function calls push/pop correct number of values

1. **Update generate_call in mir_codegen.rs:**
   ```rust
   MirOperation::Call { function, arguments } => {
       // Push arguments
       for arg in arguments {
           match self.get_value_type(arg) {
               MirType::StringTuple { .. } => {
                   // Load both ptr and len
                   self.load_string_tuple(arg)?;
               }
               _ => self.load_operand(arg)?,
           }
       }

       // Call function
       self.current_instructions.push(Instruction::Call(fn_index));

       // Handle return value
       match return_type {
           MirType::StringTuple { .. } => {
               // Multi-value return: stack has [ptr, len]
               // Store both values if needed
           }
           _ => {
               // Regular return handling
           }
       }
   }
   ```

#### Phase 4: Fix String Operations
**Target:** String concatenation and other operations

1. **String concatenation** should call runtime function:
   ```wat
   (import "env" "string_concat" (func $string_concat
       (param i32 i32 i32 i32)  ;; str1_ptr, str1_len, str2_ptr, str2_len
       (result i32 i32)))        ;; result_ptr, result_len
   ```

2. **Update binary operations in MIR builder** to detect string ops

#### Phase 5: Fix Local Variables
**Target:** String local variables store both ptr and len

1. **Allocate TWO locals for each string variable:**
   ```rust
   // For: string data = fetchData(...)
   // Allocate: local_0 (ptr), local_1 (len)
   ```

2. **Update local.set/get operations** to handle pairs

### Implementation Order (Prevents Regression)

1. ✅ **Fix Phase 1:** MIR String Type (foundational)
2. ✅ **Fix Phase 2:** Function Signatures
3. ✅ **Fix Phase 3:** Function Calls
4. ✅ **Fix Phase 4:** String Operations
5. ✅ **Fix Phase 5:** Local Variables
6. ✅ **Run comprehensive test after EACH phase**

### Expected Improvements

- **After Phase 1+2:** Function signature errors should drop from 75% to ~0%
- **After Phase 3:** Function call errors should drop significantly
- **After Phase 4:** String operation errors resolved
- **After Phase 5:** local.set errors drop from 85% to ~0%

### Target Metrics

**Current:**
- 33/285 tests pass (11%)
- 252/285 tests fail (88%)

**After All Fixes:**
- 280+/285 tests pass (98%+)
- Remaining failures will be edge cases only

## References

- WebAssembly Multi-Value Proposal: Supported in all modern runtimes
- Context7 wasmtime examples: Multi-value returns are standard practice
- Print function implementation: Already uses (ptr, len) correctly

## Next Steps

1. Start with Phase 1 (MIR String Type)
2. Test incrementally with known-failing tests
3. Iterate until 100% pass rate achieved
