# Clean Language Compiler - Agent System

## Overview

This directory contains configurations for autonomous testing and development agents that maintain code quality for the Clean Language compiler.

## Agent Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           AGENT ECOSYSTEM                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  TESTING & QA AGENTS                                                        │
│  ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐               │
│  │  QA Engineer    │ │  Regression     │ │    Verifier     │               │
│  │  (Comprehensive)│ │    Guard        │ │ (Quality Gate)  │               │
│  └────────┬────────┘ └────────┬────────┘ └────────┬────────┘               │
│           │                   │                   │                         │
│           └───────────────────┴───────────────────┘                         │
│                               │                                             │
│                               ▼                                             │
│  FIXING & DEBUGGING AGENTS                                                  │
│  ┌─────────────────┐ ┌─────────────────┐                                   │
│  │  Compiler       │ │   Error         │                                   │
│  │  Debugger       │ │   Fixer         │                                   │
│  └────────┬────────┘ └────────┬────────┘                                   │
│           │                   │                                             │
│           └───────────────────┘                                             │
│                   │                                                         │
│                   ▼                                                         │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     Code Architect                                   │   │
│  │                  (Design & Best Practices)                           │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Agent Files

| Agent | File | Purpose | Color |
|-------|------|---------|-------|
| QA Engineer | `agents/clean-language-qa-engineer.md` | Comprehensive quality assurance | Green |
| Compiler Debugger | `agents/compiler-debugger.md` | Systematic debugging | Cyan |
| Error Fixer | `agents/error-fixer.md` | Error resolution | Red |
| Code Architect | `agents/code-architect.md` | Design & best practices | Blue |
| Regression Guard | `agents/regression_guard.md` | Prevent regressions | Orange |
| Verifier | `agents/verifier.md` | Final quality gate | Purple |
| **Spec Coverage** | `agents/spec_coverage.md` | Specification coverage analysis | Green |
| **Test Auditor** | `agents/test_auditor.md` | Test compliance auditing | Orange |

## Workflow Files

| Workflow | File | Purpose |
|----------|------|---------|
| Nightly Tests | `workflows/nightly.md` | Daily comprehensive testing |
| Pre-Commit | `workflows/pre_commit.md` | Quick checks before commits |
| Bug Fix Session | `workflows/bugfix_session.md` | Autonomous fix sessions |
| Release | `workflows/release.md` | Release verification |
| **Spec Compliance** | `workflows/spec_compliance.md` | Specification compliance workflow |

## Usage

### Run Specific Agent
Reference the agent in your request:
```
"Use the regression-guard agent to check for regressions"
"Launch the verifier agent for release verification"
```

### Execute Workflow
Reference the workflow:
```
"Execute the nightly test workflow"
"Run the pre-commit checks"
```

## Integration with Project

The agents work with:
- **Test files**: `tests/cln/` (179+ .cln files)
- **Test output**: `tests/output/` (compiled .wasm)
- **Scripts**: `scripts/` (comprehensive_test_runner.sh, etc.)
- **QA automation**: `tests/qa/scripts/`

## Quality Standards

Per CLAUDE.md mandate:
- 100% compilation rate required
- 100% execution rate required
- No placeholder implementations
- No todo!() macros
- Production-grade code only

## Adding New Agents

1. Create new `.md` file in `agents/`
2. Use frontmatter format:
   ```yaml
   ---
   name: agent-name
   description: Use this agent when...
   model: sonnet
   color: colorname
   ---
   ```
3. Define clear responsibilities
4. Specify success criteria
5. Update this README
