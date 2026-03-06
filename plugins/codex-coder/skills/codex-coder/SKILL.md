---
name: codex-coder
description: Implements code using GPT-5.4 in workspace-write sandbox. Writes features, fixes bugs, creates tests based on precise task plans. Invoked when the user says "implement this", "write code for", "build this feature", "add this functionality", "create this module", "codex로 구현해줘", or needs multi-file code generation that benefits from GPT-5.4's coding ability. Does NOT do review — codex-review handles that.
---

# Instructions

Execute GPT-5.4 code implementation via Codex App Server with workspace-write sandbox.

## When to Use codex-coder vs Direct Implementation

**Use codex-coder when:**
- Multi-file creation/modification (3+ files)
- New module or feature from scratch
- Test suite generation
- Boilerplate-heavy implementation
- User explicitly requests Codex

**Do NOT use (implement directly) when:**
- Single-file simple edit
- One-line fix or rename
- Configuration change
- The task is faster to do directly than to write a plan

## Workflow

### Step 1: Analyze & Write Plan

Before invoking codex-coder, YOU (Claude Code) must:

1. **Read relevant files** — understand current codebase structure
2. **Identify scope** — which files to create, modify, reference
3. **Write plan file** — save to `{repo}/.codex-coder-cache/plans/{session-name}.md`

**Session Name**: Descriptive prefix + random hex:
```
<prefix>-!`openssl rand -hex 4`
```
Examples: `rate-limit-a3f7b2c1`, `auth-middleware-7d4e9f3a`

### Step 2: Plan File Format (CRITICAL)

The plan file is the CONTRACT between you and Codex. Quality here determines output quality.

```markdown
# Task: {concise title}

## Objective
{one sentence — WHAT to achieve, not HOW}

## Scope
### Create
- {path} — {brief purpose}

### Modify
- {path} — {what to change and why}

### Reference (read only, do not modify)
- {path} — {why Codex should read this}

### DO NOT Touch
- {path or area}

## Requirements
1. {specific, testable requirement}
2. {specific, testable requirement}

## Constraints
- {rule to follow}
- {pattern to match}
```

**Plan rules:**
- **Task**: Clear objective, not implementation details (Codex decides HOW)
- **Range**: Explicit file paths in Scope — Codex knows where to look and where to write
- **DO NOT embed file contents** — Codex reads files itself in workspace-write sandbox
- **DO NOT prescribe implementation** — state WHAT, not HOW (unless specific algorithm is required)
- **Reference files** tell Codex which patterns to follow

### Step 3: Invoke

```bash
bash ${CLAUDE_PLUGIN_ROOT}/bin/codex-coder.sh \
  --project-path "!`git rev-parse --show-toplevel`" \
  "<session-name>" "<plan-file-path>"
```

**Note**: The plan file path should be absolute. Use the repo root path.

### Step 4: Handle Results

The script returns a structured summary:
- **Status**: completed / partial / blocked
- **Files changed**: table of created/modified/deleted files
- **Git diff stats**: actual change metrics
- **Summary**: what was implemented

### Step 5: Verify (Conditional)

Trigger verify-impl agent when:
- **Always**: 5+ files changed, or new module created
- **Skip**: Simple additions (1-2 files), user says "skip verification"

```
Use the verify-impl agent to validate the implementation.
Plan file: {plan_file_path}
Project root: {project_root}
```

### Step 6: Report to User

Present unified summary (do NOT read cache files unless user asks):
1. Implementation summary (from script stdout)
2. Verification verdict (from agent return)
3. List of changed files
4. Paths for on-demand access

## Context Preparation

### Use Conversation Context
If the user has already discussed files, issues, or requirements in conversation, extract and use that information to write the plan. Don't re-ask what's already known.

### When Context is Missing
Ask clarifying questions:
1. **What** to implement (feature, fix, refactor)
2. **Where** in the codebase (files, modules)
3. **Constraints** (dependencies, patterns, tests needed)

## Environment

**Prerequisites**:
- `codex` CLI installed (`npm install -g @openai/codex`)
- `codex login` completed
- Rust binary built: `cd plugins && cargo build -p codex-appserver --release`

**Optional**:
- `OPENAI_MODEL` — override model (default: `gpt-5.4`)

**Cache**: `{project}/.codex-coder-cache/plans/` (plans), `implementations/` (results), `verifications/` (verify-impl output)
