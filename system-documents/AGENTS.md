# Repository Guidelines

## Project Structure & Module Organization
- `src/` — Rust sources grouped by stage: `lexer/`, `parser/`, `semantic/`, `ir/`, `codegen/`, `runtime/`, plus entrypoints in `main.rs` and `bin/` tools.
- `tests/` — Rust integration tests, fixtures in `tests/clean_files/`, and generated outputs in `tests/output/`.
- `examples/` — Sample Clean Language programs (`.cln`).
- `documentation/` and `.docs/` — Design notes and reference docs.
- `scripts/` — Helper scripts (testing, cleanup, CI helpers).
- `language-server/`, `modules/`, `stdlib/` — Supporting tools and standard library modules.

## Build, Test, and Development Commands
- `cargo build` — Compile all binaries and the library.
- `cargo run --bin clean-language-compiler -- -i <in.cln> -o <out.wasm>` — Compile a Clean file.
- `make build | make test | make run INPUT=examples/hello.cln OUTPUT=examples/hello.wasm` — Common tasks.
- Test suites: `make all-tests` (unit, integration, parser, semantic, codegen).
- Targeted tests: `cargo test semantic::` or `cargo test codegen::`.
- Quality gates: `make benchmark`, `make coverage`, `make quality-gate`.

## Coding Style & Naming Conventions
- Language: Rust 2021, 4‑space indentation, standard `rustfmt` defaults.
- Names: modules/files `snake_case`, types/traits `PascalCase`, functions/vars `snake_case`.
- Lints: Clippy is allowed (see `clippy.toml`); prefer fixing warnings when practical.
- Keep binaries small and focused; place utilities in `src/bin/`.

## Testing Guidelines
- Primary: `cargo test` and `make all-tests`.
- Test data lives in `tests/clean_files/*.cln`; avoid adding binaries to git.
- Name new integration tests `*_tests.rs` and keep per‑area focus (e.g., parser/semantic/codegen).
- For performance and coverage checks, use `make benchmark` and `make coverage`.

## Commit & Pull Request Guidelines
- Before committing: run `./scripts/pre-commit-cleanup.sh` (removes build artfacts and stray `*.cln`/`*.wasm` in repo root). Details: `GIT_COMMIT_GUIDELINES.md`.
- Commit format: `<type>: <description>` (e.g., `feat: improve string.length lowering`). Types: feat, fix, docs, refactor, test, ci.
- PRs must include: concise description, rationale, links to issues, and test coverage or reproduction steps. Add CLI examples (input `.cln`, expected `.wasm`) when relevant.
- Do not commit `target/`, logs, or root‑level test artifacts. Update docs when changing behavior.

## Security & Configuration Tips
- Prefer feature‑gated runtimes when adding deps; avoid introducing networked code paths into core stages.
- Use `package.clean.toml` for package metadata; keep paths relative.
