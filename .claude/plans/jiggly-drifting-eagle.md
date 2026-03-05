# gemini-lens: Gemini 멀티모달 분석 플러그인

## Context

Gemini CLI는 `@file` 구문으로 이미지/비디오를 직접 모델에 전달할 수 있다. 실험으로 headless 모드(`gemini -p`)에서 이미지 인식, 비디오 분석, 다중 파일 비교, JSON 출력 모두 동작 확인 완료.
codex-review가 "코드 리뷰"를 위한 Codex CLI 래퍼라면, **gemini-lens**는 "시각 콘텐츠 분석"을 위한 Gemini CLI 래퍼다.

## 디렉토리 구조

```
plugins/gemini-lens/
├── .claude-plugin/
│   └── plugin.json                # 매니페스트 (v1.0.0)
├── bin/
│   ├── gemini-lens.sh             # 메인 쉘 스크립트 래퍼
│   └── analysis-instructions.md   # Gemini 프롬프트 템플릿
├── skills/
│   └── gemini-lens/
│       └── SKILL.md               # 스킬 오케스트레이션 레이어
└── README.md
```

런타임 캐시 (프로젝트 루트에 생성):
```
{repo}/.gemini-lens-cache/analyses/{session}.md
```

루트 `.gitignore`에 `.gemini-lens-cache/` 추가 필요.

---

## 구현 순서 (9단계)

### Step 1: `plugin.json`
```json
{
  "name": "gemini-lens",
  "description": "Multimodal visual analysis using Gemini 3.1 Pro — images, videos, screenshots, diagrams, documents",
  "version": "1.0.0",
  "author": { "name": "jaewooseo" },
  "license": "MIT"
}
```

### Step 2: `bin/analysis-instructions.md` (프롬프트 템플릿)

역할: Visual Analysis Expert (코드 리뷰어가 아닌 시각 분석 전문가)

**5개 분석 모드** — 모드별 분석 프레임워크 + 출력 구조 정의:

| 모드 | 용도 | 출력 |
|------|------|------|
| `review` | UI/UX 디자인 리뷰, 접근성 | 시각 계층, 색상/대비, 타이포, WCAG 준수 |
| `compare` | before/after, A/B 비교 | 차이점, 개선점, 퇴보, 추천 |
| `describe` | 일반 시각 설명 (기본값) | 요소, 레이아웃, 텍스트, 스타일 |
| `extract` | OCR, 데이터 추출 | 구조화된 텍스트/테이블/숫자 |
| `debug` | 에러 스크린샷, 깨진 레이아웃 | 이슈 식별, 원인 추정, 수정 제안 |

템플릿 변수: `{repo_root}`, `{session_name}`, `{analysis_mode}`, `{project_memory}`

구조화된 출력 포맷 (Summary → Mode-Specific Sections → Recommendations by Priority).

### Step 3: `bin/gemini-lens.sh` (쉘 스크립트)

codex-review.sh + braintrust-gemini.sh 패턴 기반.

#### 인자 인터페이스 (검증 반영: `--file` 반복 플래그)

```bash
gemini-lens.sh [--project-path <path>] [--mode <mode>] --file <path> [--file <path> ...] <session-name> <analysis-prompt>
```

- `--file`: 반복 가능한 named 플래그 → bash 배열로 수집
- `<session-name>`: 첫 번째 positional arg
- `<analysis-prompt>`: 나머지 positional args를 공백 join (codex-review 패턴과 동일)

#### 전체 처리 흐름 (12단계)

1. **`set -euo pipefail`**
2. **인자 파싱**: `--project-path`, `--mode`, `--file` (배열), positional `<session>`, `<prompt>`
3. **gemini 설치 체크**: `command -v gemini &>/dev/null || exit 2`
4. **세션명 검증**: `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`
5. **모드 검증**: `case "$MODE" in review|compare|describe|extract|debug) ;; *) exit 2`
6. **파일 검증**: 존재 여부 + 포맷 allowlist (png/jpg/jpeg/gif/webp/mp4/mov/avi/webm/pdf) → 실패 시 `exit 2`
7. **repo root 탐지**: 4단계 fallback (codex-review.sh L45-77 동일)
8. **프로젝트 메모리 로딩**: `~/.claude/CLAUDE.md` + rules + 프로젝트 CLAUDE.md (codex-review.sh L91-134 동일)
9. **프롬프트 빌드**: mktemp + python3 템플릿 변수 치환 + `trap 'rm -f ...' EXIT`
10. **Gemini 실행**: subshell + stderr 분리 + `|| {}` 에러 블록
11. **캐시 저장**: `.gemini-lens-cache/analyses/{session}.md` (메타데이터 헤더 포함)
12. **요약 출력**: heredoc Markdown 포맷

#### Gemini 실행 패턴 (braintrust-gemini.sh L74-93 참조)

```bash
# @file 인자 빌드 (배열 기반, 공백 안전)
GEMINI_FILE_ARGS=""
for f in "${FILES[@]}"; do
  GEMINI_FILE_ARGS="$GEMINI_FILE_ARGS @$f"
done

# Gemini 실행 + stderr 분리 + 에러 핸들링
STDERR_LOG="${TMPDIR:-/tmp}/gemini-lens-stderr-$SESSION_NAME.log"
(
  cd "$REPO_ROOT" && \
  gemini \
    -p "$(cat "$TEMP_PROMPT") $GEMINI_FILE_ARGS" \
    --model "$MODEL" \
    --output-format text \
    -e none \
    -y \
    > "$GEMINI_OUTPUT" \
    2>"$STDERR_LOG"
) || {
  EXIT_CODE=$?
  echo "Error: gemini exec failed (exit $EXIT_CODE). See $STDERR_LOG" >&2
  exit 1
}
```

#### 요약 출력 포맷 (heredoc)

```bash
cat <<SUMMARY

## Analysis Complete

**Session**: $SESSION_NAME
**Mode**: $ANALYSIS_MODE
**Files analyzed**: $FILE_COUNT
**Full report**: .gemini-lens-cache/analyses/$SESSION_NAME.md
**Output**: $WORD_COUNT words, $LINES lines

SUMMARY
```

### Step 4: `SKILL.md` (오케스트레이션)

codex-review SKILL.md 패턴 기반. 검증에서 지적된 누락 섹션 모두 포함.

#### 프론트매터

```yaml
---
name: gemini-lens
description: >
  Multimodal visual analysis using Gemini 3.1 Pro. Analyzes images, videos,
  screenshots, diagrams, and documents for UI/UX review, comparison, OCR,
  debugging, and general visual Q&A. Triggers on "analyze this image",
  "review this UI", "compare these screenshots", "OCR this", "describe this video",
  "what's in this screenshot", "extract text from". NOT for code review — use
  codex-review. NOT for code generation from images.
---
```

#### 필수 섹션 목록

1. **Plan Mode Guard** — plan mode일 때 실행 금지, Shift+Tab 안내
2. **Invocation** — `--file` 반복 플래그 호출 템플릿
3. **Mode Detection** — 자동감지 결정 테이블 (아래)
4. **Supported File Formats** — 포맷 목록
5. **Context Preparation** — 3단계 워크플로우 (check history → sufficient? → invoke or ask)
6. **Context7 Integration** — WCAG/디자인 시스템 문서 자동 조회
7. **Session Management** — 세션명 규칙 + 팔로업 정책
8. **Environment** — 전제조건, 환경변수, 캐시 경로
9. **Workflow Examples** — 3개 (rich context, minimal context, partial context)
10. **Best Practices** — 요약만 반환 규칙, 캐시 읽기 금지 등

#### 호출 템플릿 (검증 반영)

```bash
bash ${CLAUDE_PLUGIN_ROOT}/bin/gemini-lens.sh \
  --project-path "!`git rev-parse --show-toplevel`" \
  --mode "<mode>" \
  --file "<absolute-path-1>" \
  --file "<absolute-path-2>" \
  "<session-name>" "<analysis-prompt>"
```

#### 모드 자동감지 결정 테이블

| 조건 | 모드 |
|------|------|
| 2+ 파일 + "compare/difference/before/after" | `compare` |
| "extract/OCR/text/read/transcribe" | `extract` |
| "error/bug/broken/wrong/fix/debug" | `debug` |
| "review/UI/UX/design/accessibility/layout" | `review` |
| 기본값 | `describe` |

#### Context7 연동

UI/접근성 리뷰 시 → WCAG 2.2 가이드라인 조회
특정 프레임워크 UI 리뷰 시 → 해당 디자인 시스템 문서 조회 (Material Design, Apple HIG 등)

#### 팔로업 정책 (stateless)

Gemini는 stateless → 팔로업 시 동일 세션명으로 재실행.
Claude는 이전 캐시 파일을 Read하여 컨텍스트 보강 후 새 프롬프트로 재호출.

#### Best Practices (핵심 규칙)

1. 대화 컨텍스트 활용 — 이미 알면 묻지 말고 바로 실행
2. **캐시 파일 읽지 마라** — 요약만 반환, 사용자가 명시적으로 요청할 때만 Read
3. 실행 전 파일 존재 확인 — `ls`로 검증
4. 모드 자동감지 우선 — 명확하지 않으면 `describe`
5. 관련 파일 일괄 분석 — 개별 호출보다 `--file` 다중 전달
6. Context7 자동 조회 — UI 리뷰 시 WCAG/디자인 시스템 문서

### Step 5: `chmod +x bin/gemini-lens.sh`

### Step 6: `README.md` 작성

### Step 7: `marketplace.json` 업데이트

```json
{
  "name": "gemini-lens",
  "source": "./plugins/gemini-lens",
  "description": "Multimodal visual analysis using Gemini 3.1 Pro — images, videos, screenshots, diagrams, documents"
}
```

### Step 8: `.gitignore` 업데이트

루트 `.gitignore`에 추가:
```
.gemini-lens-cache/
```

### Step 9: 테스트

1. `bash -n bin/gemini-lens.sh` — 문법 검사
2. 인자 검증 테스트 (잘못된 세션명, 잘못된 모드, 없는 파일, 미지원 포맷)
3. 실제 Gemini 실행 테스트 (테스트 이미지로 describe 모드)
4. 멀티 파일 테스트 (2개 이미지 compare 모드)
5. `claude --plugin-dir ./plugins/gemini-lens` 통합 테스트

---

## codex-review와의 차이점

| | codex-review | gemini-lens |
|---|---|---|
| CLI | `codex exec` | `gemini -p` |
| 모델 | gpt-5.3-codex | gemini-3.1-pro-preview |
| 입력 | 코드 (read-only sandbox) | 이미지/비디오/PDF (`@file`) |
| 파일 전달 | 프롬프트 텍스트에 포함 | `--file` 반복 플래그 → `@path` 변환 |
| 검증 | verify-review 에이전트 | 없음 (시각 콘텐츠는 코드로 검증 불가) |
| 세션 | codex session resume | stateless (캐시 파일 참조, 팔로업 시 재실행) |
| 출력 | 심각도 테이블 | 요약 Markdown + 캐시 파일 경로 |

## 참조 파일

- `plugins/codex-review/bin/codex-review.sh` — 쉘 스크립트 구조 (arg, repo detect, memory, template, cache, summary)
- `plugins/codex-review/skills/codex-review/SKILL.md` — 스킬 구조 (context workflow, examples, best practices)
- `plugins/codex-review/bin/review-instructions.md` — 프롬프트 구조 (role, framework, output format)
- `plugins/braintrust/bin/braintrust-gemini.sh` — Gemini CLI 호출 패턴 (`-p`, `-e none`, `-y`, stderr, error block)
- `.claude-plugin/marketplace.json` — 등록 위치
- `.gitignore` — 캐시 디렉토리 추가 위치
