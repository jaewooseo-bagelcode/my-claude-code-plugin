#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SESSION_NAME=""
PROJECT_PATH=""
ANALYSIS_MODE="describe"
ANALYSIS_PROMPT=""
FILES=()

# --- Args parsing ---
while [[ $# -gt 0 ]]; do
  case "$1" in
    --project-path)
      PROJECT_PATH="$2"
      shift 2
      ;;
    --mode)
      ANALYSIS_MODE="$2"
      shift 2
      ;;
    --file)
      FILES+=("$2")
      shift 2
      ;;
    *)
      if [[ -z "$SESSION_NAME" ]]; then
        SESSION_NAME="$1"
      else
        ANALYSIS_PROMPT="${ANALYSIS_PROMPT:+$ANALYSIS_PROMPT }$1"
      fi
      shift
      ;;
  esac
done

if [[ -z "$SESSION_NAME" || -z "$ANALYSIS_PROMPT" || ${#FILES[@]} -eq 0 ]]; then
  echo "Usage: gemini-lens.sh [--project-path <path>] [--mode <mode>] --file <path> [--file <path> ...] <session-name> <analysis-prompt>" >&2
  exit 2
fi

# --- Validate session name ---
if ! [[ "$SESSION_NAME" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]]; then
  echo "Invalid session name: use A-Za-z0-9._- only, max 64 chars, start with alphanumeric" >&2
  exit 2
fi

# --- Validate analysis mode ---
case "$ANALYSIS_MODE" in
  review|compare|describe|extract|debug) ;;
  *)
    echo "Invalid mode: $ANALYSIS_MODE (must be one of: review, compare, describe, extract, debug)" >&2
    exit 2
    ;;
esac

# --- Validate files ---
ALLOWED_EXTENSIONS="png|jpg|jpeg|gif|webp|mp4|mov|avi|webm|pdf"
for f in "${FILES[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "Error: file not found: $f" >&2
    exit 2
  fi
  ext="${f##*.}"
  ext_lower=$(echo "$ext" | tr '[:upper:]' '[:lower:]')
  if ! [[ "$ext_lower" =~ ^($ALLOWED_EXTENSIONS)$ ]]; then
    echo "Error: unsupported format '.$ext_lower' for file: $f" >&2
    echo "Supported: png, jpg, jpeg, gif, webp, mp4, mov, avi, webm, pdf" >&2
    exit 2
  fi
done

# --- Check gemini installation ---
if ! command -v gemini &>/dev/null; then
  echo "Error: gemini CLI not found. Install from: https://github.com/google-gemini/gemini-cli" >&2
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
TEMP_PROMPT=$(mktemp "${TMPDIR:-/tmp}/gemini-lens-XXXXXX.md")
TEMP_MEMORY=$(mktemp "${TMPDIR:-/tmp}/gemini-memory-XXXXXX.txt")
trap 'rm -f "$TEMP_PROMPT" "$TEMP_MEMORY"' EXIT

TEMPLATE="$SCRIPT_DIR/analysis-instructions.md"
if [[ ! -f "$TEMPLATE" ]]; then
  echo "Error: analysis-instructions.md not found at $TEMPLATE" >&2
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
result = result.replace('{analysis_mode}', sys.argv[5])
result = result.replace('{project_memory}', memory)
with open(sys.argv[6], 'w') as f:
    f.write(result)
" "$TEMPLATE" "$TEMP_MEMORY" "$REPO_ROOT" "$SESSION_NAME" "$ANALYSIS_MODE" "$TEMP_PROMPT"

# Append analysis prompt
cat >> "$TEMP_PROMPT" <<EOF

---

## Analysis Request

$ANALYSIS_PROMPT
EOF

# --- Build @file arguments ---
GEMINI_FILE_ARGS=""
for f in "${FILES[@]}"; do
  GEMINI_FILE_ARGS="$GEMINI_FILE_ARGS @$f"
done

# --- Execute gemini ---
MODEL="${GEMINI_MODEL:-gemini-3.1-pro-preview}"
GEMINI_OUTPUT=$(mktemp "${TMPDIR:-/tmp}/gemini-output-XXXXXX.txt")
STDERR_LOG="${TMPDIR:-/tmp}/gemini-lens-stderr-$SESSION_NAME.log"
trap 'rm -f "$TEMP_PROMPT" "$TEMP_MEMORY" "$GEMINI_OUTPUT"' EXIT

FILE_COUNT=${#FILES[@]}
echo "Starting gemini-lens analysis: $SESSION_NAME (mode: $ANALYSIS_MODE, files: $FILE_COUNT)" >&2
echo "Model: $MODEL" >&2

(
  cd "$REPO_ROOT" && \
  gemini \
    -p "$(cat "$TEMP_PROMPT") $GEMINI_FILE_ARGS" \
    --model "$MODEL" \
    --output-format text \
    -e none \
    -y \
    > "$GEMINI_OUTPUT" \
    2>"$STDERR_LOG"
) || {
  EXIT_CODE=$?
  echo "Error: gemini exec failed (exit $EXIT_CODE). See $STDERR_LOG" >&2
  exit 1
}

# --- Save full output to cache ---
CACHE_DIR="$REPO_ROOT/.gemini-lens-cache/analyses"
mkdir -p "$CACHE_DIR"
ANALYSIS_FILE="$CACHE_DIR/$SESSION_NAME.md"

# Write metadata header + output
{
  echo "<!-- gemini-lens session: $SESSION_NAME -->"
  echo "<!-- mode: $ANALYSIS_MODE -->"
  echo "<!-- model: $MODEL -->"
  echo "<!-- date: $(date -u +%Y-%m-%dT%H:%M:%SZ) -->"
  echo "<!-- files: ${FILES[*]} -->"
  echo ""
  cat "$GEMINI_OUTPUT"
} > "$ANALYSIS_FILE"

# --- Summary output ---
WORD_COUNT=$(wc -w < "$GEMINI_OUTPUT" | tr -d ' ')
LINES=$(wc -l < "$GEMINI_OUTPUT" | tr -d ' ')

cat <<SUMMARY

## Analysis Complete

**Session**: $SESSION_NAME
**Mode**: $ANALYSIS_MODE
**Files analyzed**: $FILE_COUNT
**Full report**: .gemini-lens-cache/analyses/$SESSION_NAME.md
**Output**: $WORD_COUNT words, $LINES lines

SUMMARY
