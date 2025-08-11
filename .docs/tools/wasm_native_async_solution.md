# WASM-Native Async Solution for Clean Language

## Overview

This document outlines a **fully self-contained WASM async implementation** that requires no host dependencies and maintains complete portability.

## Core Principles

1. **Pure WASM Implementation** - All async logic compiled into WASM
2. **State Machine Pattern** - Use WASM-native coroutines and state machines
3. **Cooperative Multitasking** - Manual yielding within WASM execution
4. **Memory-Based Task Queue** - WASM linear memory for task management
5. **Polling-Based Futures** - WASM-compatible future resolution

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    WASM Module                          │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │   Parser    │  │   Codegen   │  │   Runtime   │     │
│  │             │  │             │  │             │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
├─────────────────────────────────────────────────────────┤
│              WASM Async Runtime Layer                   │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │Task Queue   │  │State Machine│  │Future Store │     │
│  │(Memory)     │  │(Functions)  │  │(Memory)     │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
├─────────────────────────────────────────────────────────┤
│                 WASM Linear Memory                      │
├─────────────────────────────────────────────────────────┤
│  Task Queue | Future Store | Execution Stack | Heap    │
└─────────────────────────────────────────────────────────┘
```

## Implementation Strategy

### 1. State Machine Code Generation

Transform async constructs into state machines:

```clean
// Clean Language Code:
background print("Hello")
later result = start computeValue()
integer value = result  // implicit await

// Generated WASM State Machine:
function async_state_machine() {
    switch (current_state) {
        case 0: // background print
            call $print("Hello")
            enqueue_task(1)
            current_state = 1
            return
        case 1: // start future
            future_id = create_future()
            enqueue_computation(future_id, computeValue)
            current_state = 2
            return
        case 2: // await result
            if (is_future_ready(future_id)) {
                value = get_future_value(future_id)
                current_state = 3
            }
            return
        case 3: // complete
            return
    }
}
```

### 2. Memory Layout

```
WASM Linear Memory Layout:
┌─────────────────┬─────────────────┬─────────────────┬─────────────────┐
│   Task Queue    │  Future Store   │ Execution Stack │   Heap Space    │
│   (4KB)         │   (4KB)         │   (8KB)         │   (Remaining)   │
└─────────────────┴─────────────────┴─────────────────┴─────────────────┘
0x0000           0x1000           0x2000           0x4000

Task Queue Entry (32 bytes):
- task_id: u32
- state: u32 (pending/running/complete)
- function_ptr: u32
- next_ptr: u32
- data[16]: u8

Future Entry (32 bytes):
- future_id: u32
- state: u32 (pending/resolved)
- value: u32
- type: u32
- data[16]: u8
```

### 3. Core WASM Functions

Generate these functions directly in WASM:

```wasm
;; Task Management
(func $enqueue_task (param $task_id i32) (param $func_ptr i32))
(func $dequeue_task (result i32))
(func $execute_next_task (result i32))

;; Future Management  
(func $create_future (result i32))
(func $resolve_future (param $future_id i32) (param $value i32))
(func $is_future_ready (param $future_id i32) (result i32))
(func $get_future_value (param $future_id i32) (result i32))

;; Scheduler
(func $run_scheduler)
(func $yield_execution)
(func $resume_execution)
```

### 4. Async Pattern Transformations

#### Background Statements
```clean
background print("test")
```
↓
```wasm
(call $enqueue_task (i32.const 1) (i32.const $background_print_1))
(call $yield_execution)
```

#### Later Assignments
```clean
later result = start computation()
```
↓
```wasm
(call $create_future) ;; returns future_id
(local.set $future_id)
(call $enqueue_computation (local.get $future_id) (i32.const $computation_func))
```

#### Implicit Await
```clean
integer value = result  // where result is a future
```
↓
```wasm
(block $await_loop
  (loop $wait
    (call $is_future_ready (local.get $future_id))
    (br_if $await_loop)
    (call $yield_execution)
    (br $wait)
  )
)
(call $get_future_value (local.get $future_id))
(local.set $value)
```

## Benefits

1. **✅ Fully Self-Contained** - No host dependencies
2. **✅ Complete Portability** - Runs in any WASM environment
3. **✅ Predictable Performance** - Deterministic execution
4. **✅ Small Binary Size** - Minimal overhead
5. **✅ Standards Compliant** - Pure WASM without extensions

## Deployment Scenarios

### Web Browsers
- Runs in any modern browser
- Uses standard WASM APIs only
- No special permissions required

### Server Environments
- Node.js, Deno, Bun
- Serverless functions (AWS Lambda, Cloudflare Workers)
- Container environments

### Edge Computing
- CDN edge functions
- IoT devices with WASM support
- Embedded systems

### Desktop Applications
- Electron apps
- Native desktop with WASM runtime
- Cross-platform compatibility

## Limitations & Trade-offs

### Limitations
- **Cooperative multitasking only** - No preemptive scheduling
- **Single-threaded execution** - No true parallelism
- **Manual yield points** - Requires careful placement

### Trade-offs
- **Simplicity vs Performance** - Simpler than host-dependent async
- **Portability vs Features** - More portable, fewer advanced features
- **Determinism vs Flexibility** - Predictable but less dynamic

## Migration Path

1. **Phase 1**: Implement basic state machine generation
2. **Phase 2**: Add memory-based task queue
3. **Phase 3**: Implement future store and resolution
4. **Phase 4**: Add cooperative scheduler
5. **Phase 5**: Optimize and test across environments

This approach provides **true async functionality** while maintaining **complete WASM portability** and **zero host dependencies**. 