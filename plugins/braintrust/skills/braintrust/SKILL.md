---
name: braintrust
description: Multi-AI consensus meeting - GPT-5.3, Gemini 3.1 Pro, Claude Opus 4.6이 코드베이스를 병렬 분석하고, 의장 AI가 멀티라운드 토론을 거쳐 합의문을 도출합니다. 중요한 기술 결정에 적극적으로 사용하세요.
---

# Braintrust Meeting

3개 AI가 코드베이스를 병렬 분석하고, 의장이 멀티라운드 토론을 통해 합의문을 도출하는 회의 시스템입니다.

## 사용법

### 새 회의

```bash
${CLAUDE_PLUGIN_ROOT}/bin/braintrust-darwin-arm64 \
  --agenda "$ARGUMENTS" \
  --project-path "!`git rev-parse --show-toplevel`" \
  --max-iterations 3
```

### 이전 회의 이어하기

```bash
${CLAUDE_PLUGIN_ROOT}/bin/braintrust-darwin-arm64 \
  --resume "<meeting_id>" \
  --project-path "!`git rev-parse --show-toplevel`" \
  --max-iterations 2
```

### 세션 목록 확인

```bash
${CLAUDE_PLUGIN_ROOT}/bin/braintrust-darwin-arm64 \
  --list-sessions
```

## 인자

| 인자 | 필수 | 설명 |
|------|------|------|
| `--agenda` | 새 회의 시 | 토론 안건 (사용자 질문) |
| `--project-path` | 회의 시 | 프로젝트 루트 절대 경로 (새 회의/이어하기에 필수, 세션 목록엔 불필요) |
| `--context` | No | 추가 맥락 (코드 스니펫, 제약조건 등) |
| `--max-iterations` | No | 최대 토론 라운드 (기본: 3) |
| `--chair-model` | No | 의장 모델: `claude-opus-4-6` (기본) 또는 `gpt-5.3` |
| `--resume` | 이어하기 시 | 이전 meeting_id |
| `--list-sessions` | No | 세션 목록 출력 |

## 출력

- **stderr**: 실시간 진행 상황 (자동 표시)
- **stdout**: 최종 JSON 결과

```json
{
  "meeting_id": "uuid",
  "summary": "의장의 최종 합의문 (한국어, Claim/Evidence/Confidence 테이블 포함)",
  "raw_responses": [...],
  "iterations": [...],
  "total_iterations": 3,
  "elapsed_ms": 127000
}
```

## 결과 표시

1. stdout의 JSON을 파싱
2. `summary` 필드를 마크다운으로 사용자에게 표시
3. 에러가 있으면 명확히 보고
4. meeting_id를 안내 (이어하기 시 필요)

## 인증

codeb 로그인 필요 (`~/.codeb/credentials.json`). 인증 실패 시 `codeb login` 안내.

직접 API 모드: `NO_AIPROXY=1 ANTHROPIC_API_KEY=... OPENAI_API_KEY=... GEMINI_API_KEY=...`
