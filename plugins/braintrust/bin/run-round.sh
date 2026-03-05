#!/bin/bash
set -euo pipefail

# run-round.sh — Run all 3 braintrust participants in parallel
# Prevents participant omission when agent context is under pressure.
# Args: --project-path <path> --session-dir <dir> --round <n> <prompt-file>

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
  echo "Usage: run-round.sh --project-path <path> --session-dir <dir> --round <n> <prompt-file>" >&2
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

# --- Resolve script directory ---
BIN_DIR="$(cd "$(dirname "$0")" && pwd)"

COMMON_ARGS=(--project-path "$PROJECT_PATH" --session-dir "$SESSION_DIR" --round "$ROUND_NUM" "$PROMPT_FILE")

# --- Launch all 3 participants in parallel ---
echo "=== Round $ROUND_NUM: launching 3 participants ==="

"$BIN_DIR/braintrust-codex.sh"  "${COMMON_ARGS[@]}" &
PID_CODEX=$!

"$BIN_DIR/braintrust-gemini.sh" "${COMMON_ARGS[@]}" &
PID_GEMINI=$!

"$BIN_DIR/braintrust-claude.sh" "${COMMON_ARGS[@]}" &
PID_CLAUDE=$!

# --- Wait for all and tally results ---
SUCCESS=0
FAIL=0

wait $PID_CODEX  && ((SUCCESS++)) || ((FAIL++))
wait $PID_GEMINI && ((SUCCESS++)) || ((FAIL++))
wait $PID_CLAUDE && ((SUCCESS++)) || ((FAIL++))

echo "=== Round $ROUND_NUM complete: $SUCCESS succeeded, $FAIL failed ==="

# 1+ success → meeting continues; all failed → abort
if [[ $SUCCESS -eq 0 ]]; then
  echo "Error: all 3 participants failed in round $ROUND_NUM" >&2
  exit 1
fi

exit 0
