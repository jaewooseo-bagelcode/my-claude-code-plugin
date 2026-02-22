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
EVENTS_FILE="$SESSION_DIR/events.jsonl"
PLUGIN_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# --- Start event ---
printf '{"ts":%d,"event":"participant_start","data":{"round":%s,"participant":"gemini","model":"%s"}}\n' \
  "$(date +%s)000" "$ROUND_NUM" "$MODEL" >> "$EVENTS_FILE"
"$PLUGIN_ROOT/bin/update-dashboard.sh" "$SESSION_DIR" 2>/dev/null &

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
  # Error event
  printf '{"ts":%d,"event":"participant_error","data":{"round":%s,"participant":"gemini","error":"exit code %d"}}\n' \
    "$(date +%s)000" "$ROUND_NUM" "$EXIT_CODE" >> "$EVENTS_FILE"
  "$PLUGIN_ROOT/bin/update-dashboard.sh" "$SESSION_DIR" 2>/dev/null &
  exit 0  # Don't fail the whole meeting
}

# --- Done event + Summary ---
WORD_COUNT=$(wc -w < "$OUTPUT_FILE" | tr -d ' ')
printf '{"ts":%d,"event":"participant_done","data":{"round":%s,"participant":"gemini","words":%s}}\n' \
  "$(date +%s)000" "$ROUND_NUM" "$WORD_COUNT" >> "$EVENTS_FILE"
"$PLUGIN_ROOT/bin/update-dashboard.sh" "$SESSION_DIR" 2>/dev/null &
echo "Gemini analysis saved: $OUTPUT_FILE ($WORD_COUNT words)"
