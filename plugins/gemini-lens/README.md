# gemini-lens

Multimodal visual analysis plugin for Claude Code, powered by Gemini 3.1 Pro.

## What it does

gemini-lens wraps the Gemini CLI to analyze images, videos, screenshots, diagrams, and documents directly from Claude Code conversations. It's the visual analysis counterpart to codex-review's code analysis.

## Analysis Modes

| Mode | Use Case | Output |
|------|----------|--------|
| `describe` | General visual description (default) | Elements, layout, text, style |
| `review` | UI/UX design review, accessibility | Visual hierarchy, WCAG compliance, typography |
| `compare` | Before/after, A/B comparison | Differences, improvements, regressions |
| `extract` | OCR, data extraction | Structured text, tables, numbers |
| `debug` | Error screenshots, broken layouts | Issue identification, root cause, fix suggestions |

## Prerequisites

- [Gemini CLI](https://github.com/google-gemini/gemini-cli) installed and configured
- Google AI API key set up for gemini CLI

## Usage

The plugin is invoked automatically by Claude Code when you ask about visual content:

```
"Analyze this screenshot"
"Review this UI design for accessibility"
"Compare these two mockups"
"Extract text from this image"
"What's wrong with this layout?"
```

Or invoke directly:

```bash
bash bin/gemini-lens.sh \
  --project-path /path/to/repo \
  --mode review \
  --file /path/to/screenshot.png \
  "session-name" "analysis prompt"
```

## Supported Formats

- **Images**: png, jpg, jpeg, gif, webp
- **Video**: mp4, mov, avi, webm
- **Documents**: pdf

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `GEMINI_MODEL` | `gemini-3.1-pro-preview` | Gemini model to use |

## Cache

Analysis results are saved to `{project}/.gemini-lens-cache/analyses/{session}.md` for reference. Add `.gemini-lens-cache/` to your `.gitignore`.

## Installation

```bash
# From the marketplace
/plugin install gemini-lens@my-claude-code-plugin

# Or test locally
claude --plugin-dir ./plugins/gemini-lens
```
