# Clean Language - Command Structure

## ✅ Simplified Command System (v0.11.0+)

We've consolidated from **3 redundant commands** down to **2 focused commands**:

---

## For Users: `cln` ⭐ PRIMARY COMMAND

**Use `cln` for all everyday development tasks.**

```bash
cln compile hello.cln          # Compile to WASM
cln run app.cln                # Compile and run
cln parse file.cln             # Check syntax
cln check file.cln             # Type checking
cln targets list               # List platforms
cln runtime detect             # Auto-detect runtime
cln version                    # Show version
cln help                       # Show help
```

**Available in v0.11.0 release:**
- macOS (Intel & Apple Silicon)
- Linux (x86_64)
- Windows (x86_64)

Download: https://github.com/Ivan-Pasco/clean-language-compiler/releases/tag/v0.11.0

---

## For Developers: `clean-language-compiler` 🔧 ADVANCED TOOLS

**Use `clean-language-compiler` for advanced development features.**

### Package Management
```bash
clean-language-compiler package init              # Initialize project
clean-language-compiler package add <name>        # Add dependency
clean-language-compiler package install           # Install dependencies
clean-language-compiler package list              # List dependencies
```

### Testing & Quality
```bash
clean-language-compiler test                      # Run test suite
clean-language-compiler lint -i <file>            # Lint code
clean-language-compiler debug -i <file>           # Enhanced debugging
```

### IDE Integration
```bash
clean-language-compiler options --export-json     # Export compile options
```

---

## What Changed?

### ❌ Removed: `cleanc` (Redundant)

The `cleanc` command was removed because it only had `compile` and `run`, which are already in `cln`.

**Before (v0.10.x):**
```bash
cln compile hello.cln                  # User-friendly
cleanc compile hello.cln hello.wasm    # Same thing (redundant!)
clean-language-compiler compile ...    # Advanced features
```

**After (v0.11.0):**
```bash
cln compile hello.cln                  # For users ✅
clean-language-compiler compile ...    # For developers ✅
```

---

## Quick Reference

| Task | Command |
|------|---------|
| **Compile program** | `cln compile app.cln` |
| **Run program** | `cln run app.cln` |
| **Check syntax** | `cln parse app.cln` |
| **Type check** | `cln check app.cln` |
| **List targets** | `cln targets list` |
| **Init package** | `clean-language-compiler package init` |
| **Run tests** | `clean-language-compiler test` |
| **Lint code** | `clean-language-compiler lint -i app.cln` |

---

## Recommendation

**For 95% of use cases, just use `cln`.**

Only use `clean-language-compiler` if you need:
- Package management
- Running the test suite
- Code linting
- Enhanced debugging
- IDE integration tools

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

### Verify Only `cln` is Needed

```bash
# This is all you need for development!
cln compile hello.cln
cln run hello.cln
```

---

**Summary:** ONE primary command (`cln`) for users, ONE advanced command for developers. No more confusion! 🎉
