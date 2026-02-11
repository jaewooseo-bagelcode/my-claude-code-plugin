# Braintrust Meeting Plugin - Implementation Plan

## Context

syntaxos에 있는 Braintrust 에이전트(다중 AI 합의 시스템)를 Claude Code 플러그인으로 포팅한다. 3개 AI(GPT-5.2, Gemini 3 Pro, Claude Opus 4.6)가 병렬로 코드베이스를 분석하고, 의장이 멀티라운드 토론을 진행하여 최종 합의문을 도출하는 Rust CLI 바이너리를 구현한다.

## Key Decisions

| 항목 | 결정 | 근거 |
|------|------|------|
| 의장 모델 | Claude Opus 4.6 (기본), `CHAIR_MODEL` env로 변경 가능 | 합성 품질 우수, 200K 컨텍스트 충분 |
| 출력 방식 | stderr=진행상황, stdout=최종 JSON | 사용자 실시간 확인 + Claude Code 파싱 |
| 도구 | glob, grep, read_file, git_diff (샌드박싱 없이) | syntaxos 코드 재활용, 보안 간소화 |
| 인증 | codeb only (`~/.codeb/credentials.json`) | 사용자 요구사항 |
| search_docs | 제외 | DocsRAG 의존성 없음 |

## Plugin Structure

```
plugins/braintrust-meeting/
├── .claude-plugin/
│   └── plugin.json                    # v1.0.0
├── skills/
│   └── braintrust-meeting/
│       ├── SKILL.md                   # Claude Code skill 정의
│       ├── bin/
│       │   └── braintrust-meeting-darwin-arm64   # 빌드된 바이너리
│       └── scripts/                   # Rust 소스
│           ├── Cargo.toml
│           ├── build.sh               # 크로스 빌드 스크립트
│           └── src/
│               ├── main.rs            # CLI 진입점, 인자 파싱
│               ├── orchestrator.rs    # 회의 루프 (mod.rs 포팅)
│               ├── providers/
│               │   ├── mod.rs         # Provider trait + tool execution
│               │   ├── openai.rs      # GPT-5.2 참여자 (Responses API)
│               │   ├── claude.rs      # Claude Opus 4.6 참여자 + 의장
│               │   └── gemini.rs      # Gemini 3 Pro 참여자
│               ├── tools/
│               │   ├── mod.rs         # Tool executor (dispatch)
│               │   ├── glob.rs        # 파일 패턴 매칭
│               │   ├── grep.rs        # 내용 검색 (regex)
│               │   ├── read.rs        # 파일 읽기
│               │   └── git_diff.rs    # Git diff
│               ├── session.rs         # 데이터 구조, 디스크 저장
│               ├── config.rs          # codeb 인증, AIProxy 설정
│               └── events.rs          # stderr 진행 출력
└── LICENSE
```

## Implementation Steps

### Step 1: Cargo 프로젝트 초기화
- `plugins/braintrust-meeting/skills/braintrust-meeting/scripts/` 에 Rust 프로젝트 생성
- 의존성: `reqwest` (HTTP), `tokio` (async), `serde`/`serde_json` (JSON), `glob` (파일), `regex` (grep), `uuid` (meeting ID), `clap` (CLI 파싱)

### Step 2: config.rs - 인증 및 API 설정
- `~/.codeb/credentials.json` 에서 토큰 로드
- AIProxy URL 빌더 (openai/anthropic/google 경로)
- 환경변수 오버라이드: `CHAIR_MODEL`, `MAX_ITERATIONS`, `REASONING_EFFORT`

### Step 3: tools/ - 코드베이스 분석 도구
- syntaxos의 `subagent/toolkit.rs` 에서 포팅 (샌드박싱 제거)
- `glob_files(pattern, path)` → glob 크레이트 사용
- `grep_content(pattern, path, glob, ...)` → regex + 파일 순회
- `read_file(file_path, offset, limit)` → 직접 파일 읽기
- `git_diff()` → `git diff` 커맨드 실행
- 거부 경로: `.git`, `node_modules`, `.env` 등

### Step 4: providers/ - AI 제공자 구현
syntaxos 코드를 참조하여 포팅:

**openai.rs** (참여자)
- `POST {aiproxy}/openai/v1/responses` (Responses API)
- `reasoning.effort = "medium"` (참여자)
- Tool loop: function_call → execute → function_call_output → 반복

**claude.rs** (참여자 + 의장)
- `POST {aiproxy}/anthropic/v1/messages` (Messages API)
- `thinking.type = "adaptive"` (참여자)
- Tool loop: tool_use → execute → tool_result → 반복
- 의장 모드: tools 없이, extended thinking으로 합성

**gemini.rs** (참여자)
- `POST {aiproxy}/google/v1beta/models/gemini-3-pro-preview:generateContent`
- Tool loop: functionCall → execute → functionResponse → 반복

### Step 5: session.rs - 데이터 구조 및 저장
- `BraintrustResult`, `BraintrustIteration`, `ParticipantSession`, `ParticipantStep`
- 디스크 저장: `{project_path}/.braintrust-sessions/{meeting_id}/`
- debug.jsonl 이벤트 로깅

### Step 6: orchestrator.rs - 핵심 회의 루프
syntaxos의 `llm/mod.rs` 포팅:
1. 참여자 3명 병렬 실행 (`tokio::join!`)
2. 의장 분석: CONTINUE/DONE 판단
3. CONTINUE → 새 질문으로 다음 라운드
4. DONE 또는 max_iterations → 최종 합성
5. 실패한 참여자는 건너뛰고 계속 (graceful degradation)

### Step 7: events.rs - 진행상황 출력
- stderr에 실시간 진행 출력:
  ```
  [braintrust] 🏛️ Meeting started: {agenda preview}
  [braintrust] 📋 Round 1/5
  [braintrust]   ├─ GPT-5.2: analyzing... (step 3: grep_content)
  [braintrust]   ├─ Gemini: analyzing... (step 1: glob_files)
  [braintrust]   └─ Claude: completed ✓ (4.2s)
  [braintrust] 🪑 Chair analyzing...
  [braintrust] 📋 Round 2/5: [follow-up question]
  [braintrust] ...
  [braintrust] 📝 Chair synthesizing final consensus...
  [braintrust] ✅ Meeting completed (127s, 3 rounds)
  ```

### Step 8: main.rs - CLI 진입점
```
braintrust-meeting --agenda "..." [--context "..."] --project-path "/..." [--max-iterations 5]
```
- clap으로 인자 파싱
- codeb 인증 로드
- 프로젝트 메모리 로드 (CLAUDE.md, .claude/rules/*.md)
- `run_braintrust()` 호출
- stdout에 `BraintrustResult` JSON 출력

### Step 9: SKILL.md 작성
```markdown
---
description: Multi-AI consensus meeting (GPT-5.2 + Gemini + Claude) for architecture decisions
---
[Claude Code가 바이너리를 호출하는 방법 및 결과 해석 지침]
```

### Step 10: plugin.json + marketplace.json 등록
- `plugins/braintrust-meeting/.claude-plugin/plugin.json` (v1.0.0)
- `.claude-plugin/marketplace.json`에 플러그인 추가

### Step 11: 빌드 및 테스트
- `cargo build --release` → `bin/braintrust-meeting-darwin-arm64`
- `claude --plugin-dir ./plugins/braintrust-meeting` 으로 로컬 테스트

## Key Files to Reference (syntaxos)

| 소스 파일 | 용도 |
|-----------|------|
| `~/git/syntaxos/src-tauri/src/llm/mod.rs` | 회의 루프, 프롬프트 템플릿 |
| `~/git/syntaxos/src-tauri/src/llm/openai.rs` | GPT-5.2 Responses API |
| `~/git/syntaxos/src-tauri/src/llm/claude.rs` | Claude Messages API |
| `~/git/syntaxos/src-tauri/src/llm/gemini.rs` | Gemini generateContent API |
| `~/git/syntaxos/src-tauri/src/llm/session.rs` | 데이터 구조 |
| `~/git/syntaxos/src-tauri/src/subagent/toolkit.rs` | 도구 구현 |
| `~/git/syntaxos/src-tauri/src/commands.rs` | AIProxyConfig |

## Verification

1. **빌드 확인**: `cargo build --release` 성공
2. **단위 테스트**: 도구 모듈 (glob, grep, read) 테스트
3. **통합 테스트**: 실제 미팅 실행 (codeb 인증으로)
   ```bash
   ./bin/braintrust-meeting-darwin-arm64 \
     --agenda "이 프로젝트의 에러 핸들링 전략을 분석해주세요" \
     --project-path /path/to/test/repo \
     --max-iterations 3
   ```
4. **플러그인 테스트**: `claude --plugin-dir ./plugins/braintrust-meeting` 으로 스킬 호출
