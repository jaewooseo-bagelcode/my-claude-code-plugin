---
name: braintrust
description: Multi-AI consensus meeting - GPT-5.3, Gemini 3.1 Pro, Claude Opus 4.6이 코드베이스를 병렬 분석하고, 의장 AI가 멀티라운드 토론을 거쳐 합의문을 도출합니다. 중요한 기술 결정에 적극적으로 사용하세요.
---

# Braintrust Meeting

3개 AI(GPT-5.3 Codex, Gemini 3.1 Pro, Claude Opus 4.6)가 코드베이스를 병렬 분석하고, 의장이 멀티라운드 토론을 통해 합의문을 도출하는 회의 시스템입니다.

## 실행

`braintrust` 에이전트를 호출하여 회의를 실행합니다.

**$ARGUMENTS 파싱 규칙:**

1. `--context "..."` 또는 `--context ...` → 해당 값을 `context`로 추출, $ARGUMENTS에서 제거
2. `--max-rounds N` → 해당 숫자를 `max_rounds`로 추출, $ARGUMENTS에서 제거 (기본: 3)
3. 나머지 텍스트 → `agenda`

예시: `$ARGUMENTS` = `코드 리뷰 전략 --context "보안 중심" --max-rounds 2`
→ agenda: `코드 리뷰 전략`, context: `보안 중심`, max_rounds: `2`

**에이전트에 전달할 입력:**

```
agenda: [파싱된 agenda]
project_path: !`git rev-parse --show-toplevel`
context: [파싱된 context, 없으면 생략]
max_rounds: [파싱된 max_rounds, 없으면 3]
```

## 실시간 대시보드

회의 진행 상황을 실시간으로 확인할 수 있는 HTML 대시보드가 자동 생성됩니다.
- 경로: `.braintrust-sessions/{meeting_id}/dashboard.html`
- VS Code: Simple Browser 또는 Live Preview 확장으로 열면 파일 변경 시 자동 리로드
- 브라우저: 파일을 직접 열면 3초마다 자동 새로고침 (완료 시 중단)
- 3개 AI 참여자의 상태, 분석 내용, 의장 판정, 이벤트 타임라인을 실시간 표시

## 결과 표시

에이전트가 반환한 요약을 사용자에게 표시합니다.
상세 내용은 `.braintrust-sessions/{meeting_id}/synthesis.md` 파일을 Read하여 확인할 수 있습니다.

## 필수 조건

- `codex` CLI: `npm install -g @openai/codex` → `codex login`
- `gemini` CLI: https://github.com/google-gemini/gemini-cli → `gemini auth login`
