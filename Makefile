.PHONY: build test run clean all-tests benchmark coverage quality-gate

# Build the compiler
build:
	cargo build

# Run basic tests
test:
	cargo test

# Run a Clean Language program
run:
	cargo run --bin clean-language-compiler -- -i $(INPUT) -o $(OUTPUT)

# Clean build artifacts
clean:
	cargo clean

# 🚀 BULLETPROOF TESTING PIPELINE

# Run comprehensive test suite
all-tests: test-unit test-integration test-parser test-semantic test-compilation
	@echo "✅ All test suites completed"

# Unit tests with detailed output
test-unit:
	@echo "🧪 Running Unit Tests..."
	cargo test --lib --tests --verbose

# Integration tests
test-integration:
	@echo "🔗 Running Integration Tests..."
	cargo test --test "*" --verbose

# Parser-specific tests
test-parser:
	@echo "📝 Running Parser Tests..."
	cargo run --bin test_runner

# Semantic analysis tests
test-semantic:
	@echo "🔍 Running Semantic Analysis Tests..."
	cargo test semantic:: --verbose

# Full compilation pipeline tests
test-compilation:
	@echo "⚙️  Running Compilation Tests..."
	cargo test codegen:: --verbose

# Performance benchmarking
benchmark:
	@echo "🚀 Running Performance Benchmarks..."
	cargo run --bin performance_benchmark

# Code coverage analysis
coverage:
	@echo "📊 Analyzing Code Coverage..."
	cargo run --bin coverage_report

# Quality gate - runs all checks
quality-gate: all-tests benchmark coverage
	@echo "🎯 Quality Gate: All checks passed!"
	@echo "✅ Tests: PASSED"
	@echo "✅ Performance: NO REGRESSIONS"
	@echo "✅ Coverage: THRESHOLD MET"

# Property-based testing
test-property:
	@echo "🎲 Running Property-Based Tests..."
	cargo test --features "proptest"

# Fuzzing tests
test-fuzz:
	@echo "🔬 Running Fuzzing Tests..."
	cargo fuzz run parser_fuzz

# Mutation testing
test-mutation:
	@echo "🧬 Running Mutation Tests..."
	cargo mutagen

# Continuous integration pipeline
ci: quality-gate
	@echo "🚀 CI Pipeline: All stages passed!"

# Pre-commit hook (run this before committing)
pre-commit: test-unit test-parser
	@echo "✅ Pre-commit checks passed!"

# Development workflow
dev: test-unit test-parser
	@echo "🔄 Development cycle: Ready for next iteration!"

# Example usage:
# make run INPUT=examples/hello.cln OUTPUT=examples/hello.wasm
# make quality-gate
# make ci 