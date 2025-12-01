# Autonomous Bug Fix Session Workflow

## Trigger
Manual or when multiple test failures detected

## Configuration
```yaml
max_duration: 8 hours
auto_commit: true (to branch)
create_pr: false (manual)
```

## Steps

### 1. Initialize Session
```bash
# Create session branch
DATE=$(date +%Y%m%d)
git checkout -b fix/session-$DATE

# Capture initial state
mkdir -p logs/sessions/$DATE
cargo run --bin cln comprehensive-test > logs/sessions/$DATE/initial.log
```

### 2. Discovery Phase
```bash
# Identify all failures
grep -E "FAIL|Error|error" logs/sessions/$DATE/initial.log > logs/sessions/$DATE/failures.log

# Count issues
FAILURE_COUNT=$(wc -l < logs/sessions/$DATE/failures.log)
echo "Found $FAILURE_COUNT failures to investigate"
```

### 3. Execute Bug Fixer Agent
Use the error-fixer agent for each failure:
- Process failures in priority order (CRITICAL first)
- Fix → Verify → Commit cycle
- Log all actions

### 4. Verification
Use verifier agent:
- Verify all fixes
- Check for regressions using regression-guard
- Generate session report

### 5. Session Report
Create report in `system-documents/bugfix_session_YYYYMMDD.md`:

```markdown
# Bug Fix Session Report

**Date:** {date}
**Duration:** {hours}
**Branch:** fix/session-{date}

## Summary
- Failures found: X
- Failures fixed: Y
- Regressions: 0

## Fixed Issues
1. [Description] - Commit: {hash}
2. ...

## Remaining Issues
1. [Description] - Reason deferred

## Next Steps
- [ ] Review and merge branch
- [ ] Update TASKS.md
```

## Agent Integration
- **error-fixer**: Primary agent for fixes
- **compiler-debugger**: For complex issues
- **regression-guard**: Verify no regressions
- **verifier**: Final quality check

## Exit Conditions
- All failures fixed (success)
- Max duration reached (partial)
- Consecutive failures > 3 (blocked - needs human review)
