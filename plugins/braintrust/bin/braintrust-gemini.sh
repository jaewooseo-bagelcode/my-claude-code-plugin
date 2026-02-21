#!/bin/bash
set -euo pipefail

# braintrust-gemini.sh — Gemini CLI wrapper for braintrust participant
# Args: --project-path <path> --session-dir <dir> --round <n> <prompt-file>
# Output: full analysis → session_dir/round_N/gemini-output.md
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
  echo "Usage: braintrust-gemini.sh --project-path <path> --session-dir <dir> --round <n> <prompt-file>" >&2
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

# --- Check gemini installation ---
if ! command -v gemini &>/dev/null; then
  echo "Error: gemini CLI not found. Install from: https://github.com/google-gemini/gemini-cli" >&2
  exit 2
fi

# --- Setup ---
ROUND_DIR="$SESSION_DIR/round_${ROUND_NUM}"
mkdir -p "$ROUND_DIR"
OUTPUT_FILE="$ROUND_DIR/gemini-output.md"
MODEL="${GEMINI_MODEL:-gemini-3.1-pro}"

# --- Execute gemini ---
(
  cd "$PROJECT_PATH" && \
  gemini \
    -p "$(cat "$PROMPT_FILE")" \
    --model "$MODEL" \
    --output-format text \
    -e none \
    -y \
    > "$OUTPUT_FILE" \
    2>"$ROUND_DIR/gemini-stderr.log"
) || {
  EXIT_CODE=$?
  echo "Error: gemini exec failed (exit $EXIT_CODE). See $ROUND_DIR/gemini-stderr.log" >&2
  echo "[Gemini failed with exit code $EXIT_CODE]" > "$OUTPUT_FILE"
  exit 0  # Don't fail the whole meeting
}

# --- Summary ---
WORD_COUNT=$(wc -w < "$OUTPUT_FILE" | tr -d ' ')
echo "Gemini analysis saved: $OUTPUT_FILE ($WORD_COUNT words)"
