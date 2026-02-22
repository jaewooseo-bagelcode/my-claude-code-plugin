#!/bin/bash
set -euo pipefail

# update-dashboard.sh — Rebuild dashboard.html from session data
# Args: <session-dir>
# Reads: metadata.json, events.jsonl, round_N/*.md, synthesis.md
# Writes: dashboard.html (atomic via os.replace)

SESSION_DIR="${1:-}"
if [[ -z "$SESSION_DIR" ]]; then
  echo "Usage: update-dashboard.sh <session-dir>" >&2
  exit 2
fi

PLUGIN_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEMPLATE="$PLUGIN_ROOT/bin/dashboard-template.html"

if [[ ! -f "$TEMPLATE" ]]; then
  echo "Error: template not found: $TEMPLATE" >&2
  exit 1
fi

python3 -c '
import json, os, sys, glob as globmod, re

session_dir = sys.argv[1]
template_path = sys.argv[2]

# --- Read metadata ---
meta = {}
meta_path = os.path.join(session_dir, "metadata.json")
if os.path.isfile(meta_path):
    with open(meta_path) as f:
        meta = json.load(f)

# --- Read events ---
events = []
events_path = os.path.join(session_dir, "events.jsonl")
if os.path.isfile(events_path):
    with open(events_path) as f:
        for line in f:
            line = line.strip()
            if line:
                try:
                    events.append(json.loads(line))
                except json.JSONDecodeError:
                    pass

# --- Determine rounds from events + filesystem ---
round_dirs = sorted(globmod.glob(os.path.join(session_dir, "round_*")))
rounds = []
for rd in round_dirs:
    m = re.search(r"round_(\d+)$", rd)
    if not m:
        continue
    n = int(m.group(1))
    r = {"round": n, "prompt": "", "participants": {}, "chair": {}}

    # Read prompt
    prompt_path = os.path.join(rd, "prompt.md")
    if os.path.isfile(prompt_path):
        with open(prompt_path) as f:
            r["prompt"] = f.read()

    rounds.append(r)

# --- Build participant status from events (authoritative source) ---
participant_models = {}
participant_status = {}  # (round, name) -> status
for ev in events:
    d = ev.get("data", {})
    if ev.get("event") == "participant_start":
        rn = d.get("round", 0)
        pname = d.get("participant", "")
        if d.get("model"):
            participant_models[pname] = d["model"]
        if pname:
            participant_status[(rn, pname)] = "analyzing"
            # Ensure round exists
            while len(rounds) <= rn:
                rounds.append({"round": len(rounds), "prompt":"", "participants":{}, "chair":{}})
    elif ev.get("event") == "participant_done":
        rn = d.get("round", 0)
        pname = d.get("participant", "")
        if pname:
            participant_status[(rn, pname)] = "done"
    elif ev.get("event") == "participant_error":
        rn = d.get("round", 0)
        pname = d.get("participant", "")
        if pname:
            participant_status[(rn, pname)] = "error"
    elif ev.get("event") == "chair_decision" and d.get("round") is not None:
        rn = d["round"]
        if rn < len(rounds):
            rounds[rn]["chair"] = {"decision": d.get("decision",""), "question": d.get("question","")}

# --- Read file content and merge with event-based status ---
for r in rounds:
    rd = os.path.join(session_dir, "round_%d" % r["round"])
    for name, filename in [("codex","codex-output.md"),("gemini","gemini-output.md"),("claude","claude-output.md")]:
        fpath = os.path.join(rd, filename)
        status = participant_status.get((r["round"], name))
        content = ""
        words = 0
        if os.path.isfile(fpath):
            with open(fpath) as f:
                content = f.read().strip()
            words = len(content.split()) if content else 0
            is_error = content.startswith("[Codex failed") or content.startswith("[Gemini failed")
            if is_error:
                status = "error"
        # Only add participant if we have event or file data
        if status:
            r["participants"][name] = {"status": status, "content": content, "words": words}

# Apply models to participants
for r in rounds:
    for name, pdata in r["participants"].items():
        if name in participant_models:
            pdata["model"] = participant_models[name]

# --- Read synthesis ---
synthesis = None
synth_path = os.path.join(session_dir, "synthesis.md")
if os.path.isfile(synth_path):
    with open(synth_path) as f:
        synthesis = f.read().strip()
    if not synthesis:
        synthesis = None

# --- Determine overall status ---
status = "setup"
for ev in events:
    e = ev["event"]
    d = ev.get("data", {})
    rnd = d.get("round", 0)
    if e == "meeting_start":
        status = "setup"
    elif e == "round_start":
        status = "round_%d_participants" % rnd
    elif e == "participant_start":
        status = "round_%d_participants" % rnd
    elif e == "chair_start":
        status = "round_%d_chair" % rnd
    elif e == "synthesis_start":
        status = "synthesis"
    elif e == "synthesis_done" or e == "meeting_end":
        status = "done"

# --- Build STATE ---
state = {
    "meta": meta,
    "events": events,
    "rounds": [{"round": r["round"], "prompt": r["prompt"], "participants": r["participants"], "chair": r["chair"]} for r in rounds],
    "synthesis": synthesis,
    "status": status
}
state_json = json.dumps(state, ensure_ascii=False, indent=None)
# Escape </script> to prevent breaking out of <script> context
state_json = state_json.replace("</", "<\\/")

# --- Read template and substitute ---
with open(template_path) as f:
    tmpl = f.read()

# Replace placeholder: /*__STATE__*/{...}/*__END_STATE__*/
import re as re2
replacement = "/*__STATE__*/" + state_json + "/*__END_STATE__*/"
tmpl = re2.sub(
    r"/\*__STATE__\*/.*?/\*__END_STATE__\*/",
    lambda m: replacement,
    tmpl,
    flags=re2.DOTALL
)

# --- Atomic write (PID in tmp name to avoid race condition) ---
out_path = os.path.join(session_dir, "dashboard.html")
tmp_path = out_path + ".tmp.%d" % os.getpid()
with open(tmp_path, "w") as f:
    f.write(tmpl)
os.replace(tmp_path, out_path)
' "$SESSION_DIR" "$TEMPLATE"
