# codex-review

Professional code review using GPT-5.3-Codex with cross-model verification by Claude.

**READ-ONLY** — identifies issues and provides suggestions but never modifies code.

## How It Works

```
codex-review.sh ──► GPT-5.3-Codex (read-only sandbox)
                         │
                    full review saved to cache
                         │
                    summary returned to main agent
                         │
               ┌─────────┴─────────┐
               │ Critical/High > 0? │
               └─────────┬─────────┘
                    yes   │
                         ▼
                verify-review agent (Claude)
                         │
                    reads review from cache file
                    verifies against actual source
                    saves full report to cache
                    returns summary only
                         │
                         ▼
                unified summary to user
```

## Quick Start

### Prerequisites

```bash
npm install -g @openai/codex
codex login
```

### Install

```bash
# From marketplace
/plugin install codex-review@my-claude-code-plugin

# Or test locally
claude --plugin-dir ./plugins/codex-review
```

### Usage

Invoke via natural language — Claude auto-detects review requests:

- "review this code"
- "find bugs in auth.ts"
- "check security of this module"
- "audit this code"

Or invoke directly: `/codex-review`

## Architecture

```
plugins/codex-review/
├── .claude-plugin/plugin.json       # Plugin manifest (v3.2.0)
├── agents/
│   └── verify-review.md             # Cross-model verification agent
├── bin/
│   ├── codex-review.sh              # Main entrypoint
│   └── review-instructions.md       # Prompt template for Codex
└── skills/codex-review/
    └── SKILL.md                     # Skill definition + orchestration
```

### Data Flow

| Step | Tool | Output | Context Cost |
|------|------|--------|-------------|
| 1. Review | codex-review.sh | Full review → `.codex-review-cache/reviews/{session}.md` | ~200 tok (summary only) |
| 2. Verify | verify-review agent | Full report → `.codex-review-cache/verifications/{session}.md` | ~200 tok (summary only) |
| 3. Present | Main agent | Unified summary to user | ~300 tok |
| **Total** | | | **~700 tok** |

### Cache Structure

```
{repo}/.codex-review-cache/
├── reviews/
│   └── {session-name}.md         # Codex full review output
└── verifications/
    └── {session-name}.md         # Claude verification report
```

Sessions are stored at `{repo}/.codex-sessions/`.

## Analysis Dimensions

| Dimension | Severity | Examples |
|-----------|----------|---------|
| Bugs & Debugging | Critical | Logic errors, null references, type mismatches |
| Security | High | SQL injection, XSS, auth bypass, data exposure |
| Performance | Medium | N+1 queries, algorithm efficiency, memory leaks |
| Code Quality | Low | Naming, duplication, SOLID principles |
| Refactoring | — | Design patterns, structural improvements |

## Cross-Model Verification

GPT-5.3-Codex identifies issues, then Claude independently verifies each finding by reading the actual source code. This adversarial approach catches:

- Hallucinated file paths or line numbers
- Misread code logic
- Missed mitigating code upstream
- Outdated findings (already fixed)

Each finding gets a verdict: **Confirmed**, **False Positive**, or **Needs Context**.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_MODEL` | `gpt-5.3-codex` | Model for Codex CLI |

## License

MIT
