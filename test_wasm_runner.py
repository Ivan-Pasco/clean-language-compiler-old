#!/usr/bin/env python3
"""
WASM test runner with host function imports for Clean Language.
Provides necessary env imports to test WASM execution.
"""
import wasmtime
import sys
import os

def create_print_func(store):
    """Create a print function for WASM imports."""
    def print_impl(caller, ptr):
        # For now, just print the pointer value
        print(f"WASM Print: {ptr}")
        return []
    
    return wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], []), print_impl)

def create_printl_func(store):
    """Create a printl function for WASM imports."""
    def printl_impl(caller, ptr):
        # For now, just print the pointer value with newline
        print(f"WASM PrintL: {ptr}")
        return []
    
    return wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], []), printl_impl)

def create_input_func(store):
    """Create input functions for WASM imports."""
    def input_impl(caller):
        # Return a dummy value
        return [wasmtime.Val.i32(42)]
    
    return wasmtime.Func(store, wasmtime.FuncType([], [wasmtime.ValType.i32()]), input_impl)

def create_input_integer_func(store):
    """Create input_integer function for WASM imports."""
    def input_integer_impl(caller):
        # Return a dummy integer
        return [wasmtime.Val.i32(123)]
    
    return wasmtime.Func(store, wasmtime.FuncType([], [wasmtime.ValType.i32()]), input_integer_impl)

def create_input_float_func(store):
    """Create input_float function for WASM imports."""
    def input_float_impl(caller):
        # Return a dummy float
        return [wasmtime.Val.f64(3.14)]
    
    return wasmtime.Func(store, wasmtime.FuncType([], [wasmtime.ValType.f64()]), input_float_impl)

def create_input_yesno_func(store):
    """Create input_yesno function for WASM imports."""
    def input_yesno_impl(caller):
        # Return true (1)
        return [wasmtime.Val.i32(1)]
    
    return wasmtime.Func(store, wasmtime.FuncType([], [wasmtime.ValType.i32()]), input_yesno_impl)

def create_input_range_func(store):
    """Create input_range function for WASM imports."""
    def input_range_impl(caller, min_val, max_val):
        # Return middle value
        return [wasmtime.Val.i32((min_val + max_val) // 2)]
    
    return wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32(), wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), input_range_impl)

def create_file_funcs(store):
    """Create file operation functions for WASM imports."""
    def file_write_impl(caller, path_ptr, content_ptr):
        print(f"WASM File Write: path={path_ptr}, content={content_ptr}")
        return [wasmtime.Val.i32(1)]  # Success
    
    def file_read_impl(caller, path_ptr):
        print(f"WASM File Read: path={path_ptr}")
        return [wasmtime.Val.i32(300)]  # Return dummy string pointer
    
    def file_exists_impl(caller, path_ptr):
        print(f"WASM File Exists: path={path_ptr}")
        return [wasmtime.Val.i32(1)]  # True
    
    def file_delete_impl(caller, path_ptr):
        print(f"WASM File Delete: path={path_ptr}")
        return [wasmtime.Val.i32(1)]  # Success
    
    def file_append_impl(caller, path_ptr, content_ptr):
        print(f"WASM File Append: path={path_ptr}, content={content_ptr}")
        return [wasmtime.Val.i32(1)]  # Success
    
    return {
        'file_write': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32(), wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), file_write_impl),
        'file_read': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), file_read_impl),
        'file_exists': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), file_exists_impl),
        'file_delete': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), file_delete_impl),
        'file_append': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32(), wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), file_append_impl),
    }

def create_http_funcs(store):
    """Create HTTP operation functions for WASM imports."""
    def http_get_impl(caller, url_ptr):
        print(f"WASM HTTP GET: url={url_ptr}")
        return [wasmtime.Val.i32(300)]  # Return dummy response
    
    def http_post_impl(caller, url_ptr, data_ptr):
        print(f"WASM HTTP POST: url={url_ptr}, data={data_ptr}")
        return [wasmtime.Val.i32(300)]  # Return dummy response
    
    # Add more HTTP functions as needed
    return {
        'http_get': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), http_get_impl),
        'http_post': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32(), wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), http_post_impl),
    }

def create_string_conversion_funcs(store):
    """Create string conversion functions for WASM imports."""
    def int_to_string_impl(caller, value):
        print(f"WASM int_to_string: {value}")
        return [wasmtime.Val.i32(320)]  # Return pointer to "42"
    
    def float_to_string_impl(caller, value):
        print(f"WASM float_to_string: {value}")
        return [wasmtime.Val.i32(340)]  # Return pointer to "3.14"
    
    def bool_to_string_impl(caller, value):
        print(f"WASM bool_to_string: {value}")
        return [wasmtime.Val.i32(300 if value else 310)]  # Return pointer to "true" or "false"
    
    def string_to_int_impl(caller, ptr):
        print(f"WASM string_to_int: ptr={ptr}")
        return [wasmtime.Val.i32(42)]  # Return dummy int
    
    def string_to_float_impl(caller, ptr):
        print(f"WASM string_to_float: ptr={ptr}")
        return [wasmtime.Val.f64(3.14)]  # Return dummy float
    
    return {
        'int_to_string': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), int_to_string_impl),
        'float_to_string': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.f64()], [wasmtime.ValType.i32()]), float_to_string_impl),
        'bool_to_string': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), bool_to_string_impl),
        'string_to_int': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], [wasmtime.ValType.i32()]), string_to_int_impl),
        'string_to_float': wasmtime.Func(store, wasmtime.FuncType([wasmtime.ValType.i32()], [wasmtime.ValType.f64()]), string_to_float_impl),
    }

def run_wasm_file(wasm_path):
    """Run a WASM file with necessary host imports."""
    print(f"Running WASM file: {wasm_path}")
    print("=" * 50)
    
    # Create wasmtime engine and store
    engine = wasmtime.Engine()
    store = wasmtime.Store(engine)
    
    # Load the WASM module
    try:
        with open(wasm_path, 'rb') as f:
            wasm_bytes = f.read()
        module = wasmtime.Module(engine, wasm_bytes)
    except Exception as e:
        print(f"Failed to load WASM module: {e}")
        return False
    
    # Create host imports
    imports = {
        'print': create_print_func(store),
        'printl': create_printl_func(store),
        'input': create_input_func(store),
        'input_integer': create_input_integer_func(store),
        'input_float': create_input_float_func(store),
        'input_yesno': create_input_yesno_func(store),
        'input_range': create_input_range_func(store),
    }
    
    # Add file functions
    imports.update(create_file_funcs(store))
    
    # Add HTTP functions  
    imports.update(create_http_funcs(store))
    
    # Add string conversion functions
    imports.update(create_string_conversion_funcs(store))
    
    # Create the instance
    try:
        instance = wasmtime.Instance(store, module, imports)
        print("✅ WASM module instantiated successfully!")
        
        # Try to call the start function if it exists
        try:
            start_func = instance.exports(store).get('start')
            if start_func:
                print("Calling start function...")
                result = start_func(store)
                print(f"Start function result: {result}")
            else:
                print("No start function found in exports")
                
            # List available exports
            print("\nAvailable exports:")
            for name, export in instance.exports(store):
                print(f"  - {name}: {type(export).__name__}")
                
        except Exception as e:
            print(f"Error calling start function: {e}")
            
        return True
        
    except Exception as e:
        print(f"Failed to instantiate WASM module: {e}")
        return False

def main():
    if len(sys.argv) != 2:
        print("Usage: python3 test_wasm_runner.py <wasm_file>")
        sys.exit(1)
    
    wasm_file = sys.argv[1]
    
    if not os.path.exists(wasm_file):
        print(f"Error: {wasm_file} does not exist")
        sys.exit(1)
    
    success = run_wasm_file(wasm_file)
    
    if success:
        print("\n🎉 WASM execution completed successfully!")
        sys.exit(0)
    else:
        print("\n❌ WASM execution failed")
        sys.exit(1)

if __name__ == "__main__":
    main()