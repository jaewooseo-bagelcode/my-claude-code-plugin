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
  echo "Usage: codex-review.sh [--project-path <path>] <session-name> <review-context>" >&2
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
  # 1. --project-path (highest priority)
  if [[ -n "$PROJECT_PATH" ]]; then
    (cd "$PROJECT_PATH" 2>/dev/null && pwd) || echo "$PROJECT_PATH"
    return
  fi

  # 2. git rev-parse --show-toplevel (works with worktrees)
  local toplevel
  if toplevel=$(git rev-parse --show-toplevel 2>/dev/null) && [[ -n "$toplevel" ]]; then
    echo "$toplevel"
    return
  fi

  # 3. REPO_ROOT env var
  if [[ -n "${REPO_ROOT:-}" ]]; then
    (cd "$REPO_ROOT" 2>/dev/null && pwd) || echo "$REPO_ROOT"
    return
  fi

  # 4. CWD walk-up fallback
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

# --- Session directory ---
SESSIONS_DIR="${STATE_DIR:-$REPO_ROOT/.codex-sessions}"
mkdir -p "$SESSIONS_DIR"

# --- Load project memory ---
# Mirrors Claude Code's memory loading order:
#   1. ~/.claude/CLAUDE.md
#   2. ~/.claude/rules/*.md (sorted)
#   3. {repo}/.claude/CLAUDE.md or {repo}/CLAUDE.md
#   4. {repo}/.claude/rules/*.md (sorted)
load_project_memory() {
  local sections=()
  local home_dir="${HOME:-}"

  # 1. User memory: ~/.claude/CLAUDE.md
  if [[ -n "$home_dir" && -f "$home_dir/.claude/CLAUDE.md" ]]; then
    sections+=("### $home_dir/.claude/CLAUDE.md (user memory)"$'\n\n'"$(cat "$home_dir/.claude/CLAUDE.md")")
  fi

  # 2. User rules: ~/.claude/rules/*.md (sorted)
  if [[ -n "$home_dir" && -d "$home_dir/.claude/rules" ]]; then
    local f
    for f in "$home_dir/.claude/rules/"*.md; do
      [[ -f "$f" ]] || continue
      sections+=("### $(basename "$f") (user rules)"$'\n\n'"$(cat "$f")")
    done
  fi

  # 3. Project memory: .claude/CLAUDE.md or CLAUDE.md (first found)
  if [[ -f "$REPO_ROOT/.claude/CLAUDE.md" ]]; then
    sections+=("### .claude/CLAUDE.md (project memory)"$'\n\n'"$(cat "$REPO_ROOT/.claude/CLAUDE.md")")
  elif [[ -f "$REPO_ROOT/CLAUDE.md" ]]; then
    sections+=("### CLAUDE.md (project memory)"$'\n\n'"$(cat "$REPO_ROOT/CLAUDE.md")")
  fi

  # 4. Project rules: .claude/rules/*.md (sorted)
  if [[ -d "$REPO_ROOT/.claude/rules" ]]; then
    local f
    for f in "$REPO_ROOT/.claude/rules/"*.md; do
      [[ -f "$f" ]] || continue
      sections+=("### $(basename "$f") (project rules)"$'\n\n'"$(cat "$f")")
    done
  fi

  # Join with separator
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

# Substitute template variables using python3 (handles multiline cleanly)
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

# --- Execute codex ---
MODEL="${OPENAI_MODEL:-gpt-5.3-codex}"
SESSION_ID_FILE="$SESSIONS_DIR/$SESSION_NAME.id"

# Capture output to file (not streamed to stdout)
CODEX_OUTPUT=$(mktemp "${TMPDIR:-/tmp}/codex-output-XXXXXX.txt")
trap 'rm -f "$TEMP_PROMPT" "$TEMP_MEMORY" "$CODEX_OUTPUT"' EXIT

if [[ -f "$SESSION_ID_FILE" ]]; then
  SESSION_ID=$(cat "$SESSION_ID_FILE")
  echo "Resuming session: $SESSION_NAME (ID: ${SESSION_ID:0:16}...)" >&2
  codex exec resume "$SESSION_ID" \
    --model "$MODEL" \
    - < "$TEMP_PROMPT" \
    > "$CODEX_OUTPUT" 2>&1
else
  echo "Starting new session: $SESSION_NAME" >&2
  echo "Running codex review..." >&2
  codex exec \
    --model "$MODEL" \
    -C "$REPO_ROOT" \
    --sandbox read-only \
    - < "$TEMP_PROMPT" \
    > "$CODEX_OUTPUT" 2>&1
fi

# Extract and save session ID for future resume
CAPTURED_ID=$(grep -m1 '^session id:' "$CODEX_OUTPUT" | sed 's/^session id: //')
if [[ -n "$CAPTURED_ID" ]]; then
  echo "$CAPTURED_ID" > "$SESSION_ID_FILE"
  echo "Session saved: $SESSION_NAME → $CAPTURED_ID" >&2
fi

# --- Save full output to cache ---
CACHE_DIR="$REPO_ROOT/.codex-review-cache/reviews"
mkdir -p "$CACHE_DIR"
REVIEW_FILE="$CACHE_DIR/$SESSION_NAME.md"
cp "$CODEX_OUTPUT" "$REVIEW_FILE"

# --- Extract and print summary only ---
CRITICAL=$(grep -c '\[CRITICAL\]\|Critical:' "$CODEX_OUTPUT" 2>/dev/null || echo "0")
HIGH=$(grep -c '\[HIGH\]\|High:' "$CODEX_OUTPUT" 2>/dev/null || echo "0")
MEDIUM=$(grep -c '\[MEDIUM\]\|Medium:' "$CODEX_OUTPUT" 2>/dev/null || echo "0")
LOW=$(grep -c '\[LOW\]\|Low:' "$CODEX_OUTPUT" 2>/dev/null || echo "0")
SCORE=$(grep -oE 'score: [0-9]+/10|Overall score: [0-9]+' "$CODEX_OUTPUT" | head -1 || echo "")

cat <<SUMMARY

## Review Complete

**Session**: $SESSION_NAME
**Full report**: $REVIEW_FILE

| Severity | Count |
|----------|-------|
| Critical | $CRITICAL |
| High     | $HIGH |
| Medium   | $MEDIUM |
| Low      | $LOW |

${SCORE:+**Score**: $SCORE}

SUMMARY
