# Multi-Runtime and Target-Aware Compilation

This document describes the Clean Language compiler's multi-runtime support and target-aware compilation system, which allows for optimal WebAssembly output targeting different deployment environments.

## Overview

The Clean Language compiler now supports multiple WebAssembly runtimes and intelligent target selection for optimized compilation:

- **Multiple Runtime Support**: Wasmtime and Wasmer integration with automatic selection
- **Target-Aware Compilation**: Optimized builds for web, Node.js, native, embedded, and WASI environments
- **Intelligent Optimization**: Automatic optimization profiles based on deployment targets
- **Runtime Abstraction**: Unified interface for different WebAssembly runtimes
- **CLI Integration**: Enhanced command-line interface with target and runtime selection

## Supported Runtimes

### Wasmtime
- **Best for**: Native applications, Node.js integration, WASI support
- **Features**: High performance, comprehensive WebAssembly support, excellent WASI integration
- **Use cases**: Server applications, desktop software, development tools

### Wasmer
- **Best for**: Embedded systems, minimal footprint applications
- **Features**: Smaller runtime footprint, flexible embedding options
- **Use cases**: IoT devices, microservices, portable applications

### Auto-Selection
The compiler can automatically select the best runtime based on:
- Target platform requirements
- Available system resources
- Performance benchmarks
- Feature compatibility

## Compilation Targets

### Web Target
**Optimized for browser deployment**

```bash
cln compile app.cln --target web
```

- **Runtime Preference**: Auto (browser-compatible)
- **Optimizations**: Size optimization for fast downloads
- **Features**: Async support, SIMD, SharedArrayBuffer
- **Limitations**: 2GB memory limit, no direct file system access
- **Host Functions**: Web APIs, Canvas, WebGL, Web Audio

### Node.js Target
**Optimized for server-side JavaScript runtime**

```bash
cln compile app.cln --target nodejs
```

- **Runtime Preference**: Wasmtime (better Node.js integration)
- **Optimizations**: Speed optimization over size
- **Features**: Full async support, threading, WASI integration
- **Capabilities**: File system access, network operations, system APIs
- **Host Functions**: Node.js APIs, file I/O, HTTP/HTTPS

### Native Target
**Optimized for desktop and server applications**

```bash
cln compile app.cln --target native
```

- **Runtime Preference**: Wasmtime (maximum performance)
- **Optimizations**: Balanced speed and size optimization
- **Features**: All WebAssembly features enabled
- **Capabilities**: Full system access, unlimited memory
- **Host Functions**: Complete system integration, graphics, audio

### Embedded Target
**Optimized for resource-constrained environments**

```bash
cln compile app.cln --target embedded
```

- **Runtime Preference**: Wasmer (minimal footprint)
- **Optimizations**: Aggressive size optimization
- **Features**: Minimal feature set, no async/threading
- **Limitations**: 1MB memory limit, no I/O operations
- **Host Functions**: Core functions only

### WASI Target
**WebAssembly System Interface for portable system integration**

```bash
cln compile app.cln --target wasi
```

- **Runtime Preference**: Wasmtime (best WASI support)
- **Optimizations**: Portable optimization settings
- **Features**: WASI system calls, file I/O, component model
- **Capabilities**: Portable system access, cross-platform
- **Host Functions**: WASI imports, standardized system calls

## Runtime Configuration

### Basic Configuration

```bash
# Use specific runtime
cln compile app.cln --runtime wasmtime
cln compile app.cln --runtime wasmer

# Auto-select runtime
cln compile app.cln --runtime auto
```

### Advanced Configuration

```bash
# Combine target and runtime
cln compile app.cln --target web --runtime auto --optimization size --debug

# Verbose compilation
cln run app.cln --target nodejs --runtime wasmtime --verbose
```

## Optimization Profiles

### Development Profile
- **Focus**: Fast compilation, debugging support
- **Optimization Level**: None
- **Debug Info**: Enabled
- **Use Case**: Development and testing

```bash
cln compile app.cln --optimization development --debug
```

### Production Profile
- **Focus**: Balanced performance
- **Optimization Level**: Speed
- **Debug Info**: Disabled
- **Use Case**: Production deployment

```bash
cln compile app.cln --optimization production
```

### Size Profile
- **Focus**: Minimal code size
- **Optimization Level**: Speed and Size
- **Features**: Dead code elimination, aggressive optimization
- **Use Case**: Web deployment, embedded systems

```bash
cln compile app.cln --optimization size
```

### Speed Profile
- **Focus**: Maximum execution speed
- **Optimization Level**: Speed
- **Features**: Function inlining, vectorization
- **Use Case**: Performance-critical applications

```bash
cln compile app.cln --optimization speed
```

## CLI Commands

### Target Management

```bash
# List all available targets
cln targets list

# Show detailed target information
cln targets info web
cln targets info nodejs
cln targets info native
```

### Runtime Management

```bash
# List available runtimes
cln runtime list

# Auto-detect best runtime
cln runtime detect

# Benchmark runtimes
cln runtime benchmark test.cln
```

### Compilation Examples

```bash
# Basic compilation with auto-detection
cln compile hello.cln

# Web-optimized compilation
cln compile app.cln --target web --optimization size --debug

# Server application compilation
cln compile server.cln --target nodejs --optimization production

# Native application with specific runtime
cln compile desktop.cln --target native --runtime wasmtime --verbose

# Embedded system compilation
cln compile firmware.cln --target embedded --optimization size

# WASI portable application
cln compile portable.cln --target wasi --optimization production
```

## Architecture Overview

### Runtime Abstraction Layer

```rust
// Unified runtime trait
pub trait WebAssemblyRuntime {
    type Engine;
    type Store;
    type Module;
    type Instance;
    
    fn create_engine(config: &RuntimeConfig) -> Result<Self::Engine>;
    fn create_store(engine: &Self::Engine) -> Result<Self::Store>;
    // ... other methods
}
```

### Target Configuration System

```rust
pub struct Target {
    pub name: String,
    pub target_type: TargetType,
    pub runtime_preference: RuntimeType,
    pub capabilities: TargetCapabilities,
    pub optimizations: TargetOptimizations,
    // ... other fields
}
```

### Optimization Pipeline

1. **Target Selection**: Choose deployment target (auto or explicit)
2. **Runtime Selection**: Select WebAssembly runtime based on target
3. **Configuration Generation**: Create optimized runtime configuration
4. **Validation**: Ensure target/runtime compatibility
5. **Compilation**: Generate optimized WebAssembly output

## Feature Compatibility Matrix

| Feature | Web | Node.js | Native | Embedded | WASI |
|---------|-----|---------|---------|----------|------|
| Async Support | ✅ | ✅ | ✅ | ❌ | ❌ |
| Threading | ✅ | ✅ | ✅ | ❌ | ❌ |
| SIMD | ✅ | ✅ | ✅ | ❌ | ✅ |
| File System | ❌ | ✅ | ✅ | ❌ | ✅ |
| Network Access | ✅ | ✅ | ✅ | ❌ | ❌ |
| Graphics | ✅ | ❌ | ✅ | ❌ | ❌ |
| System Calls | ❌ | ✅ | ✅ | ❌ | ✅ |

## Performance Considerations

### Target-Specific Optimizations

- **Web**: Size optimization reduces download time and memory usage
- **Node.js**: Speed optimization improves server response times
- **Native**: Balanced optimization provides best overall performance
- **Embedded**: Size optimization critical for resource constraints
- **WASI**: Portable optimization ensures cross-platform compatibility

### Runtime Selection Impact

- **Wasmtime**: Better for performance-critical applications
- **Wasmer**: Better for memory-constrained environments
- **Auto**: Selects optimal runtime based on benchmarks and target requirements

## Development Workflow

### 1. Choose Target
Identify your deployment environment:
```bash
cln targets list
cln targets info <target>
```

### 2. Select Runtime (Optional)
Let auto-selection choose, or specify explicitly:
```bash
cln runtime detect
cln runtime list
```

### 3. Compile with Configuration
```bash
cln compile app.cln --target <target> --optimization <profile>
```

### 4. Test and Benchmark
```bash
cln run app.cln --target <target> --verbose
cln runtime benchmark app.cln
```

## Troubleshooting

### Common Issues

**Target/Runtime Incompatibility**
```
Error: Target 'embedded' does not support threading
```
Solution: Disable threading or choose compatible target

**Runtime Not Available**
```
Error: Runtime 'wasmer' not available on this system
```
Solution: Install runtime or use auto-selection

**Memory Limit Exceeded**
```
Error: Memory configuration exceeds target limit
```
Solution: Reduce memory usage or choose different target

### Debugging

```bash
# Verbose compilation output
cln compile app.cln --target web --verbose

# Runtime detection details
RUST_LOG=debug cln runtime detect

# Target validation
cln targets info <target>
```

## Configuration Files

### Project Configuration

Create `.clean-project.toml` for project-specific settings:

```toml
[compilation]
default_target = "nodejs"
default_runtime = "wasmtime"
default_optimization = "production"

[targets.nodejs]
runtime = "wasmtime"
optimization = "speed"
debug = false

[targets.web]
runtime = "auto"
optimization = "size"
debug = false
```

### Runtime Configuration

Advanced runtime configuration in `runtime.toml`:

```toml
[wasmtime]
async_support = true
threads_support = true
simd_support = true
optimization_level = "speed"

[wasmer]
async_support = false
threads_support = false
optimization_level = "size"
```

## Extension Points

### Custom Targets

Add custom targets by extending the target system:

```rust
impl Target {
    pub fn custom() -> Self {
        Self {
            name: "custom".to_string(),
            description: "Custom deployment target".to_string(),
            target_type: TargetType::Custom,
            // ... configure capabilities and optimizations
        }
    }
}
```

### Runtime Plugins

Extend runtime support through the plugin system:

```rust
pub struct CustomRuntime;

impl WebAssemblyRuntime for CustomRuntime {
    // Implement runtime interface
}
```

## Future Enhancements

- **GPU Acceleration**: WebGPU integration for compute-intensive workloads
- **Edge Computing**: Specialized targets for edge deployment
- **Serverless**: Optimizations for Function-as-a-Service platforms
- **Mobile**: Android and iOS WebAssembly targets
- **Custom Optimizations**: User-defined optimization passes

---

This multi-runtime and target-aware compilation system provides the foundation for deploying Clean Language applications across diverse environments with optimal performance and resource utilization.