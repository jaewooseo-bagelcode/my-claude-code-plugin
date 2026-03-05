# Braintrust 실시간 대시보드 (v2.1.0)

## Context

braintrust 에이전트가 3개 AI 토론을 진행할 때 유저는 스피너만 보고 기다려야 함. 토론 내용(각 AI 의견, 의장 판정, 라운드 핑퐁)을 실시간으로 보고 싶음. VS Code integrated terminal + Ghostty가 주 환경이라 tmux 불가 → **HTML 파일 + meta refresh** 방식.

## 구조

```
update-dashboard.sh가 events.jsonl + output 파일 → dashboard.html 재생성
  ↓
VS Code: Simple Browser / Live Preview → file watch 즉시 리로드
Ghostty: 브라우저에서 open → meta refresh 3초 폴링
```

## 파일 변경

| 순서 | 파일 | 작업 |
|------|------|------|
| 1 | `bin/dashboard-template.html` | **NEW** — HTML/CSS/JS 템플릿, `/*__STATE__*/` placeholder |
| 2 | `bin/update-dashboard.sh` | **NEW** — python3로 세션 파일 읽어 STATE JSON 빌드 → HTML 생성 |
| 3 | `bin/braintrust-codex.sh` | **MODIFY** — events.jsonl에 participant_start/done/error append + update-dashboard.sh 호출 |
| 4 | `bin/braintrust-gemini.sh` | **MODIFY** — 동일 |
| 5 | `agents/braintrust.md` | **MODIFY** — 각 스텝에서 이벤트 emit + update-dashboard.sh 호출 지시 추가 |
| 6 | `skills/braintrust/SKILL.md` | **MODIFY** — 대시보드 안내 추가 |
| 7 | `.claude-plugin/plugin.json` | **MODIFY** — 2.0.0 → 2.1.0 |

## 이벤트 스키마 (events.jsonl)

```jsonl
{"ts":1770782187655,"event":"meeting_start","data":{"meeting_id":"20260222-143025","agenda":"...","max_rounds":3}}
{"ts":...,"event":"round_start","data":{"round":0}}
{"ts":...,"event":"participant_start","data":{"round":0,"participant":"codex","model":"gpt-5.3-codex"}}
{"ts":...,"event":"participant_done","data":{"round":0,"participant":"codex","words":987}}
{"ts":...,"event":"participant_error","data":{"round":0,"participant":"gemini","error":"exit code 1"}}
{"ts":...,"event":"chair_start","data":{"round":0}}
{"ts":...,"event":"chair_decision","data":{"round":0,"decision":"CONTINUE","question":"보안 관점에서..."}}
{"ts":...,"event":"synthesis_start","data":{}}
{"ts":...,"event":"synthesis_done","data":{"words":2500}}
{"ts":...,"event":"meeting_end","data":{"total_rounds":2}}
```

## update-dashboard.sh

- Args: `<session-dir>`
- python3 인라인 스크립트 (codex-review.sh와 동일 패턴)
- 읽기: metadata.json, events.jsonl, round_N/*.md, synthesis.md
- STATE JSON 빌드 → `/*__STATE__*/` 치환 → dashboard.html에 atomic write (`os.replace`)
- 셸스크립트에서 `2>/dev/null &`로 백그라운드 호출 (참여자 블로킹 방지)

## STATE 구조

```javascript
const STATE = {
  meta: { meeting_id, agenda, context, created_at, max_rounds },
  events: [ { ts, event, data } ... ],
  rounds: [{
    round: 0,
    prompt: "...",
    participants: {
      codex:  { status: "done|analyzing|waiting|error", content: "...", words: 987, model: "..." },
      gemini: { status: "...", ... },
      claude: { status: "...", ... }
    },
    chair: { decision: "CONTINUE|DONE", question: "..." }
  }],
  synthesis: "...(full md or null)...",
  status: "setup|round_N_participants|round_N_chair|synthesis|done"
};
```

## 대시보드 UI 디자인

- 다크 테마 (GitHub Dark 계열), 한국어 네이티브
- `<meta http-equiv="refresh" content="3">` — done 시 JS로 제거
- 3 participant 카드 (CSS Grid, 컬러코딩: GPT=green, Gemini=purple, Claude=blue)
- 상태 애니메이션: waiting=pulse, analyzing=shimmer, done=solid, error=red
- `<details open>` 로 분석 내용 collapsible
- 의장 판정 영역 (CONTINUE=yellow, DONE=green)
- 이벤트 타임라인 (하단, collapsible)
- 인라인 마크다운 렌더러 (헤더, 볼드, 코드블록, 테이블, 리스트)

## 셸스크립트 수정 (codex.sh / gemini.sh)

각 스크립트에 ~10줄 추가:
```bash
EVENTS_FILE="$SESSION_DIR/events.jsonl"
PLUGIN_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# 시작 이벤트
printf '{"ts":%d,"event":"participant_start","data":{"round":%s,"participant":"codex","model":"%s"}}\n' \
  "$(date +%s)000" "$ROUND_NUM" "$MODEL" >> "$EVENTS_FILE"
"$PLUGIN_ROOT/bin/update-dashboard.sh" "$SESSION_DIR" 2>/dev/null &

# ... 기존 codex exec ...

# 완료 이벤트
printf '{"ts":%d,"event":"participant_done","data":{"round":%s,"participant":"codex","words":%s}}\n' \
  "$(date +%s)000" "$ROUND_NUM" "$WORD_COUNT" >> "$EVENTS_FILE"
"$PLUGIN_ROOT/bin/update-dashboard.sh" "$SESSION_DIR" 2>/dev/null &
```

에러 핸들러에도 participant_error 이벤트 추가.

## 에이전트 수정 (braintrust.md)

"Dashboard Updates" 섹션 추가. 각 스텝 전환 시:
1. python3 -c로 이벤트 JSON 생성 (한국어 안전한 json.dumps)
2. events.jsonl에 append
3. update-dashboard.sh 호출

Step 1 완료 후 유저에게 대시보드 경로 안내:
```
Dashboard: .braintrust-sessions/{meeting_id}/dashboard.html
```

## 검증

1. `bash -n` — update-dashboard.sh, codex.sh, gemini.sh 문법 체크
2. 모의 세션 생성 → update-dashboard.sh 실행 → dashboard.html 생성 확인
3. 브라우저에서 dashboard.html 열어 UI 렌더링 확인
4. events.jsonl 수정 → 3초 내 대시보드 갱신 확인
5. 빈 세션(events.jsonl 없음) → 에러 없이 "setup" 상태 표시 확인
