---
name: codex-review
description: Runs GPT-5.4 code review via a Bash script — READ-ONLY, never modifies code. Analyzes bugs, security vulnerabilities, performance issues, and code quality. Produces detailed reports with actionable suggestions. Invoked when the user says "review this code", "analyze this", "find bugs in", "what's wrong with", "check security of", "audit this code", "is this code safe", or "identify issues". Does NOT implement fixes — codex-coder handles that.
---

# Instructions

Execute GPT-5.4 code review via Codex App Server in read-only sandbox.

**READ-ONLY analysis only.** For implementing fixes, use `codex-coder`.

## Invocation

```bash
bash ${CLAUDE_PLUGIN_ROOT}/bin/codex-appserver-review.sh \
  --project-path "!`git rev-parse --show-toplevel`" \
  "<session-name>" "<review-context>"
```

**Session Name**: Descriptive prefix + random hex:
```
<prefix>-!`openssl rand -hex 4`
```
Examples: `security-a3f7b2c1`, `auth-review-7d4e9f3a`, `perf-1bc8d4ef`

## Context Preparation

Codex reads files itself in the sandbox. You only need to tell it **what** to focus on, not **how**.

### Review Context Format

Pass a concise review-context string:

```
FILES: src/auth/login.ts, src/middleware/auth.ts
FOCUS: Security
CONTEXT: SQL injection suspected at login handler line 45
```

That's it. Codex reads the files, applies its review methodology, and uses latest best practices. Do NOT embed file contents — Codex reads them directly.

### Use Conversation Context

If the user has already discussed files, issues, or concerns, extract that information and invoke immediately. Don't re-ask what's already known.

### When Context is Missing

If no file or focus is identifiable from conversation, ask:
1. **Which file(s)** to review
2. **Focus area** — Security, Bugs, Performance, Code Quality, or Comprehensive
3. **Any specific concerns**

Default to comprehensive review if focus is unclear but files are known.

## Workflow

### Step 1: Build Review Context

From conversation or by asking:
- **FILES** (required): file paths to review
- **FOCUS** (required): Security / Bugs / Performance / CodeQuality / Comprehensive
- **CONTEXT** (optional): specific concerns, git diff, error logs

### Step 2: Invoke

```bash
bash ${CLAUDE_PLUGIN_ROOT}/bin/codex-appserver-review.sh \
  --project-path "!`git rev-parse --show-toplevel`" \
  "<prefix>-!`openssl rand -hex 4`" "<review-context>"
```

### Step 3: Verify (Conditional)

Trigger verify-review agent when Critical or High count > 0:

```
Use the verify-review agent to validate the code review findings.
Review file: {review_file_path}
Project root: {project_root}
```

Skip verification when:
- All counts are 0 (score 9-10)
- User says "skip verification" or "raw review"

### Step 4: Report to User

**Do NOT Read cache files** — summaries contain all needed information. Only read full reports when user explicitly asks.

Present:
1. Codex review summary (score, severity table)
2. Verification verdict (if triggered)
3. Confirmed Critical/High action items
4. Cache file paths for on-demand access

**Example**:
```
## Code Review Results

### GPT-5.4 Review
Session: security-a3f7b2c1 | Score: 6/10
Critical: 2 | High: 1 | Medium: 3 | Low: 1

### Cross-Model Verification (Claude)
Confirmed: 5 | False Positive: 1 | Needs Context: 1
Confidence Rate: 71%

### Confirmed Critical/High
- Path Traversal Write (decode_masks.py:54)
- Malformed JSON Handling (actions.ts:291)

Full reports:
- .codex-review-cache/reviews/security-a3f7b2c1.md
- .codex-review-cache/verifications/security-a3f7b2c1.md
```

## Environment

**Prerequisites**:
- `codex` CLI installed (`npm install -g @openai/codex`)
- `codex login` completed

**Optional**:
- `OPENAI_MODEL` — override model (default: `gpt-5.4`)

**Cache**: `{project}/.codex-review-cache/reviews/` (results), `verifications/` (verify-review output)
