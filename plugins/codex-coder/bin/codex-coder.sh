#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SESSION_NAME=""
PROJECT_PATH=""
PLAN_FILE=""

# --- Args parsing ---
while [[ $# -gt 0 ]]; do
  case "$1" in
    --project-path)
      PROJECT_PATH="$2"
      shift 2
      ;;
    *)
      if [[ -z "$SESSION_NAME" ]]; then
        SESSION_NAME="$1"
      elif [[ -z "$PLAN_FILE" ]]; then
        PLAN_FILE="$1"
      fi
      shift
      ;;
  esac
done

if [[ -z "$SESSION_NAME" || -z "$PLAN_FILE" ]]; then
  echo "Usage: codex-coder.sh [--project-path <path>] <session-name> <plan-file>" >&2
  exit 2
fi

# --- Validate session name ---
if ! [[ "$SESSION_NAME" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]]; then
  echo "Invalid session name: use A-Za-z0-9._- only, max 64 chars, start with alphanumeric" >&2
  exit 2
fi

# --- Validate plan file ---
if [[ ! -f "$PLAN_FILE" ]]; then
  echo "Error: plan file not found: $PLAN_FILE" >&2
  exit 2
fi

# --- Check codex installation ---
if ! command -v codex &>/dev/null; then
  echo "Error: codex CLI not found. Install with: npm install -g @openai/codex" >&2
  exit 2
fi

# --- Validate and canonicalize project path early ---
if [[ -n "$PROJECT_PATH" ]]; then
  PROJECT_PATH="$(cd "$PROJECT_PATH" 2>/dev/null && pwd -P)" || {
    echo "Error: --project-path is not an existing directory: $PROJECT_PATH" >&2
    exit 2
  }
fi

# --- Detect repo root ---
detect_repo_root() {
  if [[ -n "$PROJECT_PATH" ]]; then
    echo "$PROJECT_PATH"
    return
  fi

  local toplevel
  if toplevel=$(git rev-parse --show-toplevel 2>/dev/null) && [[ -n "$toplevel" ]]; then
    echo "$toplevel"
    return
  fi

  if [[ -n "${REPO_ROOT:-}" ]]; then
    (cd "$REPO_ROOT" 2>/dev/null && pwd) || echo "$REPO_ROOT"
    return
  fi

  local dir
  dir=$(pwd)
  while [[ "$dir" != "/" ]]; do
    if [[ -d "$dir/.git" ]]; then
      echo "$dir"
      return
    fi
    dir=$(dirname "$dir")
  done

  pwd
}

REPO_ROOT=$(detect_repo_root)

# --- Load project memory ---
load_project_memory() {
  local sections=()
  local home_dir="${HOME:-}"

  if [[ -n "$home_dir" && -f "$home_dir/.claude/CLAUDE.md" ]]; then
    sections+=("### $home_dir/.claude/CLAUDE.md (user memory)"$'\n\n'"$(cat "$home_dir/.claude/CLAUDE.md")")
  fi

  if [[ -n "$home_dir" && -d "$home_dir/.claude/rules" ]]; then
    local f
    for f in "$home_dir/.claude/rules/"*.md; do
      [[ -f "$f" ]] || continue
      sections+=("### $(basename "$f") (user rules)"$'\n\n'"$(cat "$f")")
    done
  fi

  if [[ -f "$REPO_ROOT/.claude/CLAUDE.md" ]]; then
    sections+=("### .claude/CLAUDE.md (project memory)"$'\n\n'"$(cat "$REPO_ROOT/.claude/CLAUDE.md")")
  elif [[ -f "$REPO_ROOT/CLAUDE.md" ]]; then
    sections+=("### CLAUDE.md (project memory)"$'\n\n'"$(cat "$REPO_ROOT/CLAUDE.md")")
  fi

  if [[ -d "$REPO_ROOT/.claude/rules" ]]; then
    local f
    for f in "$REPO_ROOT/.claude/rules/"*.md; do
      [[ -f "$f" ]] || continue
      sections+=("### $(basename "$f") (project rules)"$'\n\n'"$(cat "$f")")
    done
  fi

  local result=""
  for i in "${!sections[@]}"; do
    if [[ $i -gt 0 ]]; then
      result+=$'\n\n---\n\n'
    fi
    result+="${sections[$i]}"
  done
  echo "$result"
}

PROJECT_MEMORY=$(load_project_memory)

# --- Copy plan file to cache ---
PLAN_CACHE_DIR="$REPO_ROOT/.codex-coder-cache/plans"
mkdir -p "$PLAN_CACHE_DIR"
PLAN_CACHE_DEST="$PLAN_CACHE_DIR/${SESSION_NAME}.md"
PLAN_FILE_REAL=$(cd "$(dirname "$PLAN_FILE")" && pwd -P)/$(basename "$PLAN_FILE")
PLAN_CACHE_REAL=$(cd "$(dirname "$PLAN_CACHE_DEST")" && pwd -P)/$(basename "$PLAN_CACHE_DEST")
if [[ "$PLAN_FILE_REAL" != "$PLAN_CACHE_REAL" ]]; then
  cp "$PLAN_FILE" "$PLAN_CACHE_DEST"
fi

# --- Read plan content ---
PLAN_CONTENT=$(cat "$PLAN_FILE")

# --- Build prompt from template ---
TEMP_PROMPT=$(mktemp "${TMPDIR:-/tmp}/codex-coder-XXXXXX")
TEMP_MEMORY=$(mktemp "${TMPDIR:-/tmp}/codex-memory-XXXXXX")
TEMP_PLAN=$(mktemp "${TMPDIR:-/tmp}/codex-plan-XXXXXX")
trap 'rm -f "$TEMP_PROMPT" "$TEMP_MEMORY" "$TEMP_PLAN"' EXIT

TEMPLATE="$SCRIPT_DIR/coder-instructions.md"
if [[ ! -f "$TEMPLATE" ]]; then
  echo "Error: coder-instructions.md not found at $TEMPLATE" >&2
  exit 2
fi

echo "$PROJECT_MEMORY" > "$TEMP_MEMORY"
echo "$PLAN_CONTENT" > "$TEMP_PLAN"
python3 -c "
import sys
with open(sys.argv[1], 'r') as f:
    template = f.read()
with open(sys.argv[2], 'r') as f:
    memory = f.read()
with open(sys.argv[3], 'r') as f:
    plan = f.read()
result = template.replace('{repo_root}', sys.argv[4])
result = result.replace('{session_name}', sys.argv[5])
result = result.replace('{project_memory}', memory)
result = result.replace('{plan_content}', plan)
with open(sys.argv[6], 'w') as f:
    f.write(result)
" "$TEMPLATE" "$TEMP_MEMORY" "$TEMP_PLAN" "$REPO_ROOT" "$SESSION_NAME" "$TEMP_PROMPT"

# --- Locate App Server binary ---
MODEL="${OPENAI_MODEL:-gpt-5.4}"
BINARY=""
for candidate in \
  "${SCRIPT_DIR}/codex-appserver-coder" \
  "${SCRIPT_DIR}/../../target/release/codex-appserver-coder"; do
  if [[ -x "$candidate" ]]; then
    BINARY="$candidate"
    break
  fi
done

if [[ -z "$BINARY" ]]; then
  echo "Error: codex-appserver-coder binary not found." >&2
  echo "Searched:" >&2
  echo "  ${SCRIPT_DIR}/codex-appserver-coder" >&2
  echo "  ${SCRIPT_DIR}/../../target/release/codex-appserver-coder" >&2
  echo "Build it with: cd plugins && cargo build -p codex-appserver --release" >&2
  exit 2
fi

"$BINARY" --project-path "$REPO_ROOT" --model "$MODEL" "$SESSION_NAME" "$TEMP_PROMPT"
