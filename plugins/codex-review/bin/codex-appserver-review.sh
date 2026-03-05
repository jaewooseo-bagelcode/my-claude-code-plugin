#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SESSION_NAME=""
PROJECT_PATH=""
REVIEW_CONTEXT=""

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
      else
        REVIEW_CONTEXT="${REVIEW_CONTEXT:+$REVIEW_CONTEXT }$1"
      fi
      shift
      ;;
  esac
done

if [[ -z "$SESSION_NAME" || -z "$REVIEW_CONTEXT" ]]; then
  echo "Usage: codex-appserver-review.sh [--project-path <path>] <session-name> <review-context>" >&2
  exit 2
fi

# --- Validate session name ---
if ! [[ "$SESSION_NAME" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]]; then
  echo "Invalid session name: use A-Za-z0-9._- only, max 64 chars, start with alphanumeric" >&2
  exit 2
fi

# --- Check codex installation ---
if ! command -v codex &>/dev/null; then
  echo "Error: codex CLI not found. Install with: npm install -g @openai/codex" >&2
  exit 2
fi

# --- Detect repo root ---
detect_repo_root() {
  if [[ -n "$PROJECT_PATH" ]]; then
    (cd "$PROJECT_PATH" 2>/dev/null && pwd) || echo "$PROJECT_PATH"
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

# --- Build prompt from template ---
TEMP_PROMPT=$(mktemp "${TMPDIR:-/tmp}/codex-review-XXXXXX.md")
TEMP_MEMORY=$(mktemp "${TMPDIR:-/tmp}/codex-memory-XXXXXX.txt")
trap 'rm -f "$TEMP_PROMPT" "$TEMP_MEMORY"' EXIT

TEMPLATE="$SCRIPT_DIR/review-instructions.md"
if [[ ! -f "$TEMPLATE" ]]; then
  echo "Error: review-instructions.md not found at $TEMPLATE" >&2
  exit 2
fi

echo "$PROJECT_MEMORY" > "$TEMP_MEMORY"
python3 -c "
import sys
with open(sys.argv[1], 'r') as f:
    template = f.read()
with open(sys.argv[2], 'r') as f:
    memory = f.read()
result = template.replace('{repo_root}', sys.argv[3])
result = result.replace('{session_name}', sys.argv[4])
result = result.replace('{project_memory}', memory)
with open(sys.argv[5], 'w') as f:
    f.write(result)
" "$TEMPLATE" "$TEMP_MEMORY" "$REPO_ROOT" "$SESSION_NAME" "$TEMP_PROMPT"

# Append review context
cat >> "$TEMP_PROMPT" <<EOF

---

## Review Request

$REVIEW_CONTEXT
EOF

# --- Locate App Server binary ---
MODEL="${OPENAI_MODEL:-gpt-5.3-codex}"
BINARY=""
for candidate in \
  "${SCRIPT_DIR}/codex-appserver-review" \
  "${SCRIPT_DIR}/../../target/release/codex-appserver-review"; do
  if [[ -x "$candidate" ]]; then
    BINARY="$candidate"
    break
  fi
done

if [[ -z "$BINARY" ]]; then
  echo "Error: codex-appserver-review binary not found." >&2
  echo "Searched:" >&2
  echo "  ${SCRIPT_DIR}/codex-appserver-review" >&2
  echo "  ${SCRIPT_DIR}/../../target/release/codex-appserver-review" >&2
  echo "Build it with: cd plugins && cargo build -p common --release" >&2
  exit 2
fi

"$BINARY" --project-path "$REPO_ROOT" --model "$MODEL" "$SESSION_NAME" "$TEMP_PROMPT"
