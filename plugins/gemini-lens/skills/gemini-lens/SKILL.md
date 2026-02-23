---
name: gemini-lens
description: >
  Multimodal visual analysis using Gemini 3.1 Pro. Analyzes images, videos,
  screenshots, diagrams, and documents for UI/UX review, comparison, OCR,
  debugging, and general visual Q&A. Triggers on "analyze this image",
  "review this UI", "compare these screenshots", "OCR this", "describe this video",
  "what's in this screenshot", "extract text from". NOT for code review — use
  codex-review. NOT for code generation from images.
---

# Instructions

Execute Gemini-powered multimodal visual analysis with complete context preparation.

**IMPORTANT: Plan Mode Guard** — If you are currently in plan mode, do NOT execute this skill. Inform the user to exit plan mode first (Shift+Tab), then invoke.

## Invocation

```bash
bash ${CLAUDE_PLUGIN_ROOT}/bin/gemini-lens.sh \
  --project-path "!`git rev-parse --show-toplevel`" \
  --mode "<mode>" \
  --file "<absolute-path-1>" \
  --file "<absolute-path-2>" \
  "<session-name>" "<analysis-prompt>"
```

**Session Name**: Must be globally unique. Generate by combining a descriptive prefix with a random suffix:
```
<prefix>-!`openssl rand -hex 4`
```
- Prefix: short descriptor (e.g., "ui-review", "compare", "ocr", "debug")
- Examples: `ui-review-a3f7b2c1`, `compare-7d4e9f3a`, `ocr-1bc8d4ef`

**Files**: Each file requires a separate `--file` flag with an **absolute path**. Resolve relative paths before invocation.

**Analysis Prompt**: Structured context for Gemini analysis (see Context Preparation below).

## Mode Detection

Automatically select the analysis mode based on user intent:

| Condition | Mode |
|-----------|------|
| 2+ files + "compare/difference/before/after/vs" | `compare` |
| "extract/OCR/text/read/transcribe/pull out" | `extract` |
| "error/bug/broken/wrong/fix/debug/crash/issue" | `debug` |
| "review/UI/UX/design/accessibility/layout/WCAG" | `review` |
| Default (none of the above) | `describe` |

When ambiguous, prefer `describe` — it provides the most general coverage.

## Supported File Formats

**Images**: png, jpg, jpeg, gif, webp
**Video**: mp4, mov, avi, webm
**Documents**: pdf

## Context Preparation

### Step 1: Check Conversation History

Extract from previous messages:
- **Files**: Any file paths mentioned, screenshots shared, images discussed
- **Intent**: What the user wants to know about the visual content
- **Context**: UI framework, design system, specific concerns mentioned

### Step 2: Determine if Context is Sufficient

**Sufficient context = Invoke immediately:**
- File path identified (or user provided a screenshot path)
- General intent clear
- Mode can be auto-detected

**Insufficient context = Ask first:**
- No file path identifiable
- Unclear what analysis is needed

### Step 3: Build Prompt and Invoke

**If context is sufficient** — build analysis prompt from conversation context:

```python
analysis_prompt = """
Visual Analysis Request:

TARGET: [what to analyze — e.g., "landing page design", "error dialog"]
FOCUS: [specific aspects — e.g., "accessibility", "color contrast", "layout issues"]
CONTEXT: [relevant background — e.g., "React app", "mobile-first design", "dark mode"]
"""
```

**If context is insufficient** — ask the user:
```
"I can analyze this with Gemini. What should I focus on?
1. UI/UX design review (accessibility, visual hierarchy, layout)
2. Compare with another version
3. Extract text/data (OCR)
4. Debug visual issues (broken layout, errors)
5. General description"
```

## Context7 Integration

**For `review` mode**, automatically query Context7 for relevant guidelines:

- **WCAG 2.2**: When checking accessibility compliance
- **Material Design / Apple HIG**: When reviewing platform-specific UI
- **Framework design systems**: When user mentions a specific framework (Tailwind, Chakra UI, etc.)

Include fetched guidelines in the analysis prompt as `REFERENCE GUIDELINES:` section.

## Session Management

**Session naming**: Descriptive prefix + random hex suffix.

**Stateless execution**: Gemini CLI is stateless — each invocation is independent.

**Follow-up policy**: For follow-up questions on the same content:
1. Reuse the same session name
2. Read the previous cache file to get context
3. Build a new prompt that references prior findings
4. Re-invoke with the updated prompt

## Environment

**Prerequisites**:
- `gemini` CLI installed (https://github.com/google-gemini/gemini-cli)
- Google AI API key configured for gemini CLI

**Optional**:
- `GEMINI_MODEL` — override model (default: `gemini-3.1-pro-preview`)

**Cache**: `{project}/.gemini-lens-cache/analyses/` (project-isolated)

## Workflow Examples

### Example 1: Rich Conversation Context

```
[Earlier in conversation]
User: "I just redesigned the login page, here's a screenshot"
User: [provides /tmp/login-redesign.png]
User: "Does this follow accessibility guidelines?"

You (Claude Code):
[Extract context from conversation]
- File: /tmp/login-redesign.png
- Mode: review (accessibility focus)
- Context: Login page redesign

[Query Context7 for WCAG 2.2 guidelines]

[Build prompt and invoke]
bash ${CLAUDE_PLUGIN_ROOT}/bin/gemini-lens.sh \
  --project-path "$(git rev-parse --show-toplevel)" \
  --mode "review" \
  --file "/tmp/login-redesign.png" \
  "login-a11y-{hex}" "Visual Analysis Request:
TARGET: Login page redesign
FOCUS: WCAG 2.2 accessibility compliance — contrast ratios, touch targets, focus indicators, text alternatives
CONTEXT: Login page screenshot for accessibility audit
REFERENCE GUIDELINES: [WCAG 2.2 from Context7]"
```

### Example 2: Minimal Context

```
User: "What's in this screenshot? /tmp/error.png"

You (Claude Code):
[Minimal but sufficient context]
- File: /tmp/error.png
- Mode: describe (default)

bash ${CLAUDE_PLUGIN_ROOT}/bin/gemini-lens.sh \
  --project-path "$(git rev-parse --show-toplevel)" \
  --mode "describe" \
  --file "/tmp/error.png" \
  "describe-{hex}" "Describe what is visible in this screenshot — all UI elements, text, layout, and any notable visual details."
```

### Example 3: Multi-file Comparison

```
User: "Compare these two designs"
User: [provides /tmp/v1.png and /tmp/v2.png]

You (Claude Code):
- Files: /tmp/v1.png, /tmp/v2.png
- Mode: compare (2 files + comparison intent)

bash ${CLAUDE_PLUGIN_ROOT}/bin/gemini-lens.sh \
  --project-path "$(git rev-parse --show-toplevel)" \
  --mode "compare" \
  --file "/tmp/v1.png" \
  --file "/tmp/v2.png" \
  "compare-{hex}" "Compare these two design versions. Identify visual differences, improvements, regressions, and recommend which version is stronger."
```

## Best Practices

1. **Use conversation context** — if file paths, intent, or concerns are already discussed, don't ask again. Invoke immediately.
2. **Do NOT Read cache files** — present only the summary returned by the script. Only Read the full report when the user explicitly asks for details.
3. **Verify file existence** — use `ls` to confirm files exist before invoking.
4. **Auto-detect mode** — use the decision table above. Default to `describe` when unclear.
5. **Batch related files** — use multiple `--file` flags in one invocation rather than separate calls.
6. **Context7 for review mode** — automatically fetch WCAG/design system docs when doing UI/UX review.
7. **Absolute paths only** — resolve all file paths to absolute paths before passing to `--file`.
8. **Summary-only return** — the script returns word count + cache path. Present this to the user, not the full analysis content.
