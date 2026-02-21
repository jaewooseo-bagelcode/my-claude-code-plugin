#!/bin/bash
set -euo pipefail

# braintrust-codex.sh — Codex CLI wrapper for braintrust participant
# Args: --project-path <path> --session-dir <dir> --round <n> <prompt-file>
# Output: full analysis → session_dir/round_N/codex-output.md
#         summary 1 line → stdout

PROJECT_PATH=""
SESSION_DIR=""
ROUND_NUM=""
PROMPT_FILE=""

# --- Args parsing ---
while [[ $# -gt 0 ]]; do
  case "$1" in
    --project-path|--session-dir|--round)
      if [[ $# -lt 2 ]]; then
        echo "Error: $1 requires a value" >&2
        exit 2
      fi
      case "$1" in
        --project-path) PROJECT_PATH="$2" ;;
        --session-dir)  SESSION_DIR="$2" ;;
        --round)        ROUND_NUM="$2" ;;
      esac
      shift 2
      ;;
    *)
      if [[ -z "$PROMPT_FILE" ]]; then
        PROMPT_FILE="$1"
      fi
      shift
      ;;
  esac
done

if [[ -z "$PROJECT_PATH" || -z "$SESSION_DIR" || -z "$ROUND_NUM" || -z "$PROMPT_FILE" ]]; then
  echo "Usage: braintrust-codex.sh --project-path <path> --session-dir <dir> --round <n> <prompt-file>" >&2
  exit 2
fi

# Validate round number is a non-negative integer
if ! [[ "$ROUND_NUM" =~ ^[0-9]+$ ]]; then
  echo "Error: --round must be a non-negative integer, got: $ROUND_NUM" >&2
  exit 2
fi

if [[ ! -f "$PROMPT_FILE" ]]; then
  echo "Error: prompt file not found: $PROMPT_FILE" >&2
  exit 2
fi

# --- Check codex installation ---
if ! command -v codex &>/dev/null; then
  echo "Error: codex CLI not found. Install with: npm install -g @openai/codex" >&2
  exit 2
fi

# --- Setup ---
ROUND_DIR="$SESSION_DIR/round_${ROUND_NUM}"
mkdir -p "$ROUND_DIR"
OUTPUT_FILE="$ROUND_DIR/codex-output.md"
MODEL="${OPENAI_MODEL:-gpt-5.3-codex}"

# --- Execute codex ---
codex exec \
  --model "$MODEL" \
  -C "$PROJECT_PATH" \
  --sandbox read-only \
  --ephemeral \
  -o "$OUTPUT_FILE" \
  - < "$PROMPT_FILE" \
  2>"$ROUND_DIR/codex-stderr.log" || {
    EXIT_CODE=$?
    echo "Error: codex exec failed (exit $EXIT_CODE). See $ROUND_DIR/codex-stderr.log" >&2
    # Write error marker for the agent to detect
    echo "[Codex failed with exit code $EXIT_CODE]" > "$OUTPUT_FILE"
    exit 0  # Don't fail the whole meeting
  }

# --- Summary ---
WORD_COUNT=$(wc -w < "$OUTPUT_FILE" | tr -d ' ')
echo "Codex analysis saved: $OUTPUT_FILE ($WORD_COUNT words)"
