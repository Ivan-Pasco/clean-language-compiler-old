High‑Level Flow

  - CLI (src/main.rs) parses args with clap, sets logging, reads input .cln, resolves output
  path, and orchestrates the compile pipeline.
  - Pipeline: tokenize/parse → build AST → semantic analysis → IR → Wasm codegen → optional
  optimization → write .wasm (and optional .wat for debugging).

  Compilation Stages

  - Lexing/Parsing: pest grammar builds the AST (src/lexer/, src/parser/, src/ast/).
  - Semantic Analysis: type checking, symbol resolution, method‑style calls (e.g.,
  string.length) registered and validated (src/semantic/), stdlib wiring (src/stdlib/).
  - IR: lowering from AST to an intermediate representation and validations/transforms (src/
  ir/).
  - Codegen: IR → WebAssembly via wasm-encoder; imports for host/stdlib functions registered;
  memory/layout handled (src/codegen/, src/memory/).
  - Optimize (optional): wasm-opt if available; WAT conversion via wat for diagnostics.

  Runtime / Execution

  - Host functions registered in env module (src/runtime/host_functions.rs).
  - Feature‑gated runtimes: wasmtime/wasmer (link imports, instantiate, run). See src/bin/
  wasmtime_runner.rs.

  Key Binaries

  - clean-language-compiler / cleanc / cln: compile .cln to .wasm.
      - Example: cargo run --bin clean-language-compiler -- compile -i examples/hello.cln -o
  examples/hello.wasm
  - Utilities: wat2wasm, wasm2wat, debug_parser, debug_wasm, test_runner, coverage_report,
  performance_benchmark.

  Errors & Logging

  - Errors via thiserror/anyhow; enhanced messages in src/error/.
  - Logging/tracing via env_logger and tracing-subscriber (filter with RUST_LOG).

  Tests & Dev Loop

  - cargo test for unit/integration; parser/semantic/codegen suites wired via Makefile.
      - make all-tests runs end‑to‑end checks; parser tests use test_runner.
  - Test inputs live in tests/clean_files/*.cln; outputs in tests/output/.

  Where to start reading: src/main.rs (CLI + pipeline), then src/semantic/mod.rs, src/ir/,
  and src/codegen/mod.rs.