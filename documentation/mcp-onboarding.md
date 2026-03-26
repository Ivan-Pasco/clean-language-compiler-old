# Developing with Clean Language + AI

Clean Language includes a built-in MCP (Model Context Protocol) server that gives your AI assistant real-time access to the language specification, syntax validation, and compilation. This is the recommended way to develop with Clean Language.

## Why Use the MCP Server?

Without it, your AI assistant guesses syntax from training data — which is often outdated or wrong. With it, the assistant can:

- Look up correct syntax before writing code
- Type-check your code without compiling
- Read the full language specification on demand
- Get architecture guidance (execution layers, host bridge, memory model)
- See all available built-in functions, types, and plugins
- Compile files and explain errors

## Quick Setup

### 1. Install Clean Language

```bash
# Install the version manager
curl -fsSL https://cleanlanguage.dev/install.sh | bash

# Install the latest compiler
cleen install latest
```

### 2. Generate the config for your AI tool

```bash
cln mcp-config --format claude-code    # Claude Code (CLI/VS Code/Desktop)
cln mcp-config --format vscode         # Cursor / VS Code extensions
cln mcp-config --format claude-desktop # Claude Desktop app
cln mcp-config --format generic        # Any other MCP-compatible tool
```

### 3. Follow the instructions for your tool below

---

## Claude Code (CLI, VS Code Extension, Desktop App)

Create a `.mcp.json` file in your project root:

```json
{
  "mcpServers": {
    "clean-language": {
      "command": "cln",
      "args": ["mcp-server"]
    }
  }
}
```

That's it. Claude Code reads `.mcp.json` automatically on session start.

**Verify it works:** Start a new session and ask "What tools do you have from the clean-language MCP?" — it should list `get_quick_reference`, `compile`, `check`, etc.

---

## Cursor

Add to your Cursor MCP settings (`.cursor/mcp.json` in your project or global settings):

```json
{
  "mcpServers": {
    "clean-language": {
      "command": "cln",
      "args": ["mcp-server"]
    }
  }
}
```

Then restart Cursor. The MCP tools will appear in the AI panel.

---

## Claude Desktop

Add to your Claude Desktop config (`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):

```json
{
  "mcpServers": {
    "clean-language": {
      "command": "/path/to/cln",
      "args": ["mcp-server"]
    }
  }
}
```

Replace `/path/to/cln` with the actual path. Find it with:

```bash
which cln
# or
cleen which
```

Restart Claude Desktop after saving.

---

## Windsurf / Continue / Other MCP Tools

Most MCP-compatible tools use the same format. Create a config file in the tool's expected location:

```json
{
  "mcpServers": {
    "clean-language": {
      "command": "cln",
      "args": ["mcp-server"]
    }
  }
}
```

Check your tool's documentation for where to place this file.

---

## Available MCP Tools

Once connected, your AI assistant has access to these tools:

| Tool | What It Does |
|------|-------------|
| `get_quick_reference` | Full syntax cheat sheet — types, blocks, operators, patterns |
| `get_specification` | Read the complete language specification |
| `get_architecture` | Execution layers, host bridge, memory model |
| `check` | Type-check a file without compiling |
| `compile` | Compile a `.cln` file to WebAssembly |
| `parse` | Parse a file and show the AST |
| `diagnostics` | Get detailed diagnostics for a file |
| `explain_error` | Explain a compiler error code |
| `list_functions` | List all built-in functions |
| `list_types` | List all types in the type system |
| `list_builtins` | List built-in modules (Math, String, etc.) |
| `list_plugins` | Show installed plugins with DSL syntax |

## Best Practice for AI Assistants

Add this to your project's `CLAUDE.md` (or equivalent instructions file):

```
Before writing ANY Clean Language code, call `get_quick_reference` from
the clean-language MCP server. Do NOT write Clean code from memory —
always verify syntax against the MCP tools.
```

This ensures the assistant always checks the spec before generating code.

## Troubleshooting

**MCP server not found:**
```bash
# Check cln is installed and on PATH
cln --version

# If not on PATH, use the full path
cleen which  # Shows installed versions and paths
```

**Tools not appearing:**
- Make sure `cln mcp-server` runs without errors: `echo '{}' | cln mcp-server`
- Restart your AI tool after adding the config
- Check the config file is in the right location for your tool

**Wrong syntax suggestions:**
- The AI might still use cached knowledge. Explicitly ask it to "call get_quick_reference from the MCP server" to force a fresh lookup.
