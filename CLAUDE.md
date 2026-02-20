# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a **Claude Code plugin marketplace** repository. It contains multiple plugins that extend Claude Code's capabilities via skills (commands), hooks, agents, and MCP servers. The plugins are distributed through a self-hosted marketplace.

## Repository Structure

```
my-claude-code-plugin/
├── .claude-plugin/
│   └── marketplace.json          # Marketplace manifest listing all plugins
├── plugins/
│   └── <plugin-name>/            # Each plugin is a standalone directory
│       ├── .claude-plugin/
│       │   └── plugin.json       # Plugin manifest (name, version, description)
│       ├── skills/               # Skills with SKILL.md (preferred)
│       │   └── <skill-name>/
│       │       └── SKILL.md
│       ├── commands/             # Simple command .md files (legacy)
│       ├── agents/               # Subagent definitions
│       ├── hooks/
│       │   └── hooks.json        # Hook event handlers
│       ├── .mcp.json             # MCP server definitions (optional)
│       └── README.md
└── CLAUDE.md
```

## Plugin Development

### Creating a new plugin

1. Create directory: `plugins/<plugin-name>/`
2. Add manifest: `plugins/<plugin-name>/.claude-plugin/plugin.json`
   ```json
   {
     "name": "plugin-name",
     "description": "Brief description",
     "version": "1.0.0",
     "author": { "name": "jaewooseo" }
   }
   ```
3. Add skills in `skills/<skill-name>/SKILL.md` or commands in `commands/<name>.md`
4. Register the plugin in `.claude-plugin/marketplace.json`

### Skill file format (SKILL.md)

```markdown
---
description: What this skill does (shown in skill list)
disable-model-invocation: false
---

Instructions for Claude when this skill is invoked...
```

### Command file format (commands/*.md)

```markdown
---
allowed-tools: Bash(git status:*), Bash(git diff:*)
description: What this command does
disable-model-invocation: false
---

Instructions for Claude...
```

- `allowed-tools`: Auto-approve specific tool patterns without user confirmation
- `disable-model-invocation: true`: Only user can invoke via `/plugin:command`, Claude won't auto-invoke
- Use `!`backtick` ` syntax to inject dynamic shell output into the prompt context
- Use `$ARGUMENTS` placeholder for user input passed to the command

### Hook event types

Available events: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `PreCompact`, `Notification`, `SubagentStart`, `SubagentStop`, `Stop`, `SessionEnd`, `TaskCompleted`, `TeammateIdle`

Hook types: `command` (shell script), `prompt` (LLM evaluation), `agent` (agentic verifier)

Use `${CLAUDE_PLUGIN_ROOT}` env var to reference plugin directory in hooks and MCP configs.

### Marketplace manifest format (.claude-plugin/marketplace.json)

```json
{
  "name": "my-claude-code-plugin",
  "owner": { "name": "jaewooseo" },
  "plugins": [
    {
      "name": "plugin-name",
      "source": "./plugins/plugin-name",
      "description": "Brief description",
      "version": "1.0.0"
    }
  ]
}
```

## Testing Plugins Locally

```bash
# Test a single plugin in development
claude --plugin-dir ./plugins/<plugin-name>

# Test multiple plugins
claude --plugin-dir ./plugins/plugin-a --plugin-dir ./plugins/plugin-b
```

## Installing from This Marketplace

```bash
# Add this marketplace (from local path)
/plugin marketplace add /Users/jaewooseo/git/my-claude-code-plugin

# Or from GitHub after pushing
/plugin marketplace add <github-user>/my-claude-code-plugin

# Install a specific plugin
/plugin install <plugin-name>@my-claude-code-plugin
```

## Rust Workspace Architecture

```
plugins/
├── Cargo.toml                    # Workspace root (resolver = "2")
├── aiproxy-common/               # Shared core library
│   ├── src/{config,session,sse/*,tools/*}.rs
│   └── tests/e2e_{tools,sse,session,config,streaming}.rs
└── braintrust/scripts/           # Multi-AI consensus (uses aiproxy-common)
```

### aiproxy-common E2E 테스트 (반드시 유지)

**의도**: `aiproxy-common`은 braintrust의 공통 의존성이다. 이 코어 레이어에 버그가 있으면 하위 플러그인에 전파된다. 레이어를 분리한 가치는 코어를 독립적으로 검증할 수 있다는 데 있다.

**실행**: `cargo test -p aiproxy-common` (139 tests)

**커버리지 (100% 도구)**:
- `tools/glob` — `**` recursive, max_results, dual naming, skip .git
- `tools/grep` — files_with_matches/content/count, regex, glob filter, case_insensitive, context, head_limit
- `tools/read` — offset/limit/end_line, path traversal rejection, deny list, directory rejection
- `tools/git_diff` — default/base branch/file path, invalid branch rejection, fallback chain
- `sse/` — SseParser + OpenAI Responses/Chat + Anthropic Messages + Gemini streamGenerateContent
- `session` — JsonlLogger write/nil-safe/append, ParticipantSession lifecycle/serde, summarize_args
- `config` — aiproxy/direct mode URL routing, auth headers, project memory loading
- `streaming` — StreamingResult accumulation, OpenAI/Anthropic/Gemini stream parsing

**규칙**: aiproxy-common 코드를 변경할 때 반드시 `cargo test -p aiproxy-common`을 돌리고, 실패하면 머지하지 않는다.

## Conventions

- Plugin names: kebab-case (e.g., `my-tool`, `code-formatter`)
- Skill names: kebab-case, matching directory name under `skills/`
- Versioning: semver (MAJOR.MINOR.PATCH)
- Prefer `skills/` (SKILL.md pattern) over `commands/` for new plugins
- Components live at the plugin root, NOT inside `.claude-plugin/` — only `plugin.json` goes there
