# Clean Language - Command Structure

## ✅ Unified Command System (v0.11.0+)

We've consolidated from **3 redundant commands** down to **ONE unified command**: `cln`

Following the pattern of modern tools like `cargo`, `go`, and `npm`, Clean Language now has a single, unified command-line interface for all tasks.

---

## The `cln` Command ⭐ ALL-IN-ONE

**Use `cln` for everything - from compilation to package management.**

### Basic Operations
```bash
cln compile hello.cln          # Compile to WASM
cln run app.cln                # Compile and run
cln parse file.cln             # Check syntax
cln check file.cln             # Type checking
```

### Package Management
```bash
cln package init               # Initialize project
cln package add <name>         # Add dependency
cln package install            # Install dependencies
cln package list               # List dependencies
```

### Testing & Quality
```bash
cln test                       # Run test suite
cln lint -i file.cln           # Lint code
cln debug -i file.cln          # Enhanced debugging
```

### Platform & Runtime
```bash
cln targets list               # List platforms
cln runtime detect             # Auto-detect runtime
```

### IDE Integration
```bash
cln options --export-json      # Export compile options
```

### Help & Version
```bash
cln version                    # Show version
cln help                       # Show help
```

**Available in v0.11.0 release:**
- macOS (Intel & Apple Silicon)
- Linux (x86_64)
- Windows (x86_64)

Download: https://github.com/Ivan-Pasco/clean-language-compiler/releases/tag/v0.11.0

---

## What Changed?

### ❌ Removed: `cleanc` and `clean-language-compiler` (Redundant)

All functionality has been consolidated into the single `cln` command for better user experience.

**Before (v0.10.x):**
```bash
cln compile hello.cln                     # User-friendly command
cleanc compile hello.cln hello.wasm       # Same thing (redundant!)
clean-language-compiler package init      # Advanced features
clean-language-compiler test              # Testing
```

**After (v0.11.0):**
```bash
cln compile hello.cln                     # Compilation ✅
cln package init                          # Package management ✅
cln test                                  # Testing ✅
cln lint -i file.cln                      # Linting ✅
```

---

## Quick Reference

| Task | Command |
|------|---------|
| **Compile program** | `cln compile app.cln` |
| **Run program** | `cln run app.cln` |
| **Check syntax** | `cln parse app.cln` |
| **Type check** | `cln check app.cln` |
| **Init package** | `cln package init` |
| **Add dependency** | `cln package add <name>` |
| **Install deps** | `cln package install` |
| **Run tests** | `cln test` |
| **Lint code** | `cln lint -i app.cln` |
| **Debug code** | `cln debug -i app.cln` |
| **List targets** | `cln targets list` |
| **Detect runtime** | `cln runtime detect` |
| **IDE integration** | `cln options --export-json` |

---

## Installation

### Quick Install (Unix/macOS)

```bash
# Download for your platform
curl -LO https://github.com/Ivan-Pasco/clean-language-compiler/releases/download/v0.11.0/cln-macos-aarch64

# Make executable
chmod +x cln-macos-aarch64

# Move to PATH
sudo mv cln-macos-aarch64 /usr/local/bin/cln

# Verify
cln version
```

### All You Need

```bash
# One command for everything!
cln compile hello.cln
cln run hello.cln
cln test
cln package init
```

---

**Summary:** ONE command (`cln`) for everything. Simple, unified, powerful! 🎉
