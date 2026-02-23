#!/bin/bash
set -euo pipefail

# braintrust-claude.sh — Claude CLI wrapper for braintrust Claude participant
# Args: --project-path <path> --session-dir <dir> --round <n> <prompt-file>
# Output: full analysis → session_dir/round_N/claude-output.md
#         summary 1 line → stdout
# Uses: claude CLI (claude -p --model opus) — no API key needed

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
  echo "Usage: braintrust-claude.sh --project-path <path> --session-dir <dir> --round <n> <prompt-file>" >&2
  exit 2
fi

if ! [[ "$ROUND_NUM" =~ ^[0-9]+$ ]]; then
  echo "Error: --round must be a non-negative integer, got: $ROUND_NUM" >&2
  exit 2
fi

if [[ ! -f "$PROMPT_FILE" ]]; then
  echo "Error: prompt file not found: $PROMPT_FILE" >&2
  exit 2
fi

# --- Check claude CLI ---
if ! command -v claude &>/dev/null; then
  echo "Error: claude CLI not found." >&2
  exit 2
fi

# --- Setup ---
ROUND_DIR="$SESSION_DIR/round_${ROUND_NUM}"
mkdir -p "$ROUND_DIR"
OUTPUT_FILE="$ROUND_DIR/claude-output.md"
MODEL="${CLAUDE_MODEL:-opus}"
EVENTS_FILE="$SESSION_DIR/events.jsonl"
PLUGIN_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# --- Start event ---
printf '{"ts":%d,"event":"participant_start","data":{"round":%s,"participant":"claude","model":"claude-opus-4-6"}}\n' \
  "$(date +%s)000" "$ROUND_NUM" >> "$EVENTS_FILE"
"$PLUGIN_ROOT/bin/update-dashboard.sh" "$SESSION_DIR" 2>/dev/null &

# --- Execute via claude CLI (print mode, bypass nested session check) ---
# cd /tmp: avoid loading CWD's .claude/ project context (prevents contamination)
# --append-system-prompt-file: reliable file-based prompt delivery (avoids stdin large-input bug)
# --add-dir: grants tool access to target project without loading its CLAUDE.md
unset CLAUDECODE 2>/dev/null || true
(cd /tmp && claude -p \
  --model "$MODEL" \
  --output-format text \
  --no-session-persistence \
  --dangerously-skip-permissions \
  --add-dir "$PROJECT_PATH" \
  --append-system-prompt-file "$PROMPT_FILE" \
  "Analyze the codebase according to the system prompt instructions.") \
  > "$OUTPUT_FILE" \
  2>"$ROUND_DIR/claude-stderr.log" || {
    EXIT_CODE=$?
    echo "Error: claude exec failed (exit $EXIT_CODE). See $ROUND_DIR/claude-stderr.log" >&2
    echo "[Claude failed with exit code $EXIT_CODE]" > "$OUTPUT_FILE"
    printf '{"ts":%d,"event":"participant_error","data":{"round":%s,"participant":"claude","error":"exit code %d"}}\n' \
      "$(date +%s)000" "$ROUND_NUM" "$EXIT_CODE" >> "$EVENTS_FILE"
    "$PLUGIN_ROOT/bin/update-dashboard.sh" "$SESSION_DIR" 2>/dev/null &
    exit 0  # Don't fail the whole meeting
  }

# --- Done event + Summary ---
WORD_COUNT=$(wc -w < "$OUTPUT_FILE" | tr -d ' ')
printf '{"ts":%d,"event":"participant_done","data":{"round":%s,"participant":"claude","words":%s}}\n' \
  "$(date +%s)000" "$ROUND_NUM" "$WORD_COUNT" >> "$EVENTS_FILE"
"$PLUGIN_ROOT/bin/update-dashboard.sh" "$SESSION_DIR" 2>/dev/null &
echo "Claude analysis saved: $OUTPUT_FILE ($WORD_COUNT words)"
