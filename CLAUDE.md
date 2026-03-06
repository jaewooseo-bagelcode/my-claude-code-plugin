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
├── claude-usage-app/             # macOS menu bar app (NOT a plugin)
│   ├── Package.swift             # SPM package, macOS 14+
│   ├── Sources/ClaudeUsage/      # SwiftUI app source
│   ├── Resources/                # Info.plist, AppIcon.icns
│   └── scripts/build-app.sh     # Release build + Developer ID signing
└── CLAUDE.md
```

### claude-usage-app (standalone, NOT in marketplace)

macOS menu bar app that monitors Claude AI usage limits via Safari + AppleScript.
Uses cookie-authenticated claude.ai web API. Multi-account with active/inactive model.
Build: `cd claude-usage-app && bash scripts/build-app.sh` → `cp -R ClaudeUsage.app /Applications/`

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
└── codex-appserver/              # Codex App Server JSON-RPC client + binaries
    ├── src/appserver/{client,protocol}.rs
    ├── src/bin/codex_appserver_review.rs
    ├── src/bin/codex_appserver_coder.rs
    └── tests/e2e_appserver.rs
```

### codex-appserver E2E 테스트

**실행**: `cargo test -p codex-appserver` (85 tests)

**커버리지**:
- `appserver/protocol` — JSON-RPC 직렬화, ServerMessage 파싱, ReviewOutput/CoderOutput 스키마, deny_unknown_fields
- `appserver/client` — 텍스트 축적, UTF-8 truncation, ShutdownStatus
- `bin/codex_appserver_review` — multi-object JSON 파싱, 에러 케이스
- `bin/codex_appserver_coder` — multi-object JSON 파싱, session name validation, 에러 케이스

**규칙**: codex-appserver 코드를 변경할 때 반드시 `cargo test -p codex-appserver`를 돌리고, 실패하면 머지하지 않는다.

## Plugin Release Workflow

**버전 올리기는 반드시 유저 검수 후에 한다.** 절대로 테스트 전에 버전을 올리지 않는다.

1. 코드 변경
2. 로컬에서 충분히 테스트 (`bash -n`, E2E 등)
3. 유저에게 테스트 결과 보고 및 검수 요청
4. 유저 승인 후 `plugin.json` 버전 업 + 커밋

`plugin.json`의 version을 올려야 `/plugin` 싱크 시 반영된다. 유저 승인 없이 version을 올리지 마라.

## Conventions

- Plugin names: kebab-case (e.g., `my-tool`, `code-formatter`)
- Skill names: kebab-case, matching directory name under `skills/`
- Versioning: semver (MAJOR.MINOR.PATCH)
- Prefer `skills/` (SKILL.md pattern) over `commands/` for new plugins
- Components live at the plugin root, NOT inside `.claude-plugin/` — only `plugin.json` goes there
