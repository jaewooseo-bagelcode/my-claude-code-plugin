---
name: braintrust
description: Multi-AI consensus meeting - GPT-5.3, Gemini 3.1 Pro, Claude Opus 4.6 analyze the codebase in parallel, then chair AI (Sonnet 4.6 1M) drives multi-round discussion to produce a consensus report. Use actively for important technical decisions.
---

# Braintrust Meeting

3 AIs (GPT-5.3 Codex, Gemini 3.1 Pro, Claude Opus 4.6) analyze the codebase in parallel, then the chair (Sonnet 4.6 1M) drives multi-round discussion to produce a consensus report.

## Plan Mode

Braintrust cannot run in plan mode (read-only). Plan mode propagates read-only restrictions to subagents, blocking shell script execution.

**If plan mode is active, do NOT invoke the agent. Instead, tell the user to exit plan mode (Shift+Tab) and invoke /braintrust again.**

## Execution

Invoke the `braintrust` agent to run a meeting.

**$ARGUMENTS parsing rules:**

1. `--context "..."` or `--context ...` → extract as `context`, remove from $ARGUMENTS
2. `--max-rounds N` → extract as `max_rounds`, remove from $ARGUMENTS (default: 3)
3. Remaining text → `agenda`

Example: `$ARGUMENTS` = `code review strategy --context "security focus" --max-rounds 2`
→ agenda: `code review strategy`, context: `security focus`, max_rounds: `2`

**Input to pass to the agent:**

```
agenda: [parsed agenda]
project_path: !`git rev-parse --show-toplevel`
context: [parsed context, omit if none]
max_rounds: [parsed max_rounds, or 3]
```

## Live Dashboard

A real-time HTML dashboard is auto-generated to track meeting progress.
- Path: `.braintrust-sessions/{meeting_id}/dashboard.html`
- VS Code: Open with Simple Browser or Live Preview extension for auto-reload on file change
- Browser: Open the file directly for 3-second auto-refresh (stops on completion)
- Displays status of 3 AI participants, analysis content, chair decisions, and event timeline

## Displaying Results

Show the summary returned by the agent to the user.
For full details, Read `.braintrust-sessions/{meeting_id}/synthesis.md`.

## Prerequisites

- `codex` CLI: `npm install -g @openai/codex` → `codex login`
- `gemini` CLI: https://github.com/google-gemini/gemini-cli → `gemini auth login`
- `claude` CLI (Claude Code — for Claude Opus participant)
