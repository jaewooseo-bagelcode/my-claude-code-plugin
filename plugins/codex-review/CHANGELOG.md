# Changelog

## [3.2.1] — 2026-02-18

- **SKILL.md**: Explicit "Do NOT Read cache files" instruction in Presenting Results to prevent main agent from proactively reading full reports into context

## [3.2.0] — 2026-02-17

Context optimization via file-based handoff. ~90% context reduction.

- **codex-review.sh**: Full output saved to `.codex-review-cache/reviews/`, stdout returns severity summary + file path only
- **verify-review agent**: Reads review from cache file, saves verification report to `.codex-review-cache/verifications/`, returns summary only
- **SKILL.md**: Cross-model verification now passes file paths instead of full content

Before: ~6-10K tokens per review in main context.
After: ~700 tokens (summary + file paths).

## [3.1.0] — 2026-02-17

Cross-model verification pipeline.

- Added `verify-review` agent (Claude Sonnet) for adversarial verification of Codex findings
- Each finding gets a verdict: Confirmed / False Positive / Needs Context
- Zero-Trust Code Tracing protocol — never trust review snippets, always read actual files
- Tiered verification: Deep (Critical/High), Spot-Check (Medium), Existence (Low)

## [3.0.1] — 2026-02-17

- Adopted Zero-Trust Code Tracing Protocol in review instructions
- Codex no longer infers behavior from names, comments, or type hints

## [3.0.0] — 2026-02-17

Replaced Rust binary with Codex CLI wrapper.

- Shell script `codex-review.sh` replaces Rust `scripts/` directory
- Execution via `codex exec --model gpt-5.2-codex --sandbox read-only`
- Session resume via `codex exec resume <session-id>`
- Project memory loading mirrors Claude Code's CLAUDE.md resolution order
- Rust source archived in git history (v2.2.0)

## [2.2.0]

- Added `--project-path` flag for git worktree support
- Auto-detection via `git rev-parse --show-toplevel`

## [2.0.0]

- Rewritten in Rust with aiproxy-common shared library

## [1.0.0]

- Initial release with braintrust plugin
