# Plan: aiproxy PROVIDER_TIMEOUTS 기본값 상향 PR

## Context

aiproxy의 `PROVIDER_TIMEOUTS`가 reasoning 모델 등장 이전 기준(OpenAI 120초)으로 하드코딩되어 있어, gpt-5.2-codex + reasoning=high 조합에서 504 Gateway Timeout이 발생한다. OpenAI Python SDK 기본값(600초), Azure OpenAI(600초), LiteLLM(600초) 등 업계 표준에 맞춰 기본값을 상향한다.

## 변경 사항

### 수정 파일 (1개)

**`backend/src/common/constants/providers.constants.ts`**

```typescript
// Before
export const PROVIDER_TIMEOUTS = {
  [AI_PROVIDERS.OPENAI]: 120_000,
  [AI_PROVIDERS.ANTHROPIC]: 180_000,
  [AI_PROVIDERS.GOOGLE]: 60_000,
  [AI_PROVIDERS.GOOGLE_VERTEX]: 60_000,
  [AI_PROVIDERS.ELEVENLABS]: 120_000,
  [AI_PROVIDERS.MOONSHOT]: 120_000,
} as const;

// After
export const PROVIDER_TIMEOUTS = {
  [AI_PROVIDERS.OPENAI]: 600_000,      // 120s → 600s (10min)
  [AI_PROVIDERS.ANTHROPIC]: 600_000,   // 180s → 600s (10min)
  [AI_PROVIDERS.GOOGLE]: 300_000,      // 60s → 300s (5min)
  [AI_PROVIDERS.GOOGLE_VERTEX]: 300_000, // 60s → 300s (5min)
  [AI_PROVIDERS.ELEVENLABS]: 120_000,  // 유지
  [AI_PROVIDERS.MOONSHOT]: 300_000,    // 120s → 300s (5min)
} as const;
```

핸들러 코드는 변경 불필요 — 이미 `PROVIDER_TIMEOUTS[AI_PROVIDERS.XXX]`를 참조하고 있으므로 상수만 바꾸면 전체 반영됨.

## 실행 단계

1. `~/git/aifirst-aiproxyOS`에서 `feat/increase-provider-timeouts` 브랜치 생성
2. `providers.constants.ts` 수정
3. 커밋 (조사 내용 포함한 상세 커밋 메시지)
4. `gh pr create`로 PR 생성 — body에 조사 자료 전문 첨부

## PR 본문 구성

- Summary: 왜 올려야 하는지 (reasoning 모델 504 발생)
- 업계 비교표 (OpenAI SDK, Azure, LiteLLM, AWS Bedrock)
- Reasoning effort별 권장 타임아웃 표
- 변경 전/후 비교표
- 영향 범위: handlers는 수정 없음, 상수만 변경

## 검증

- codex-review로 reasoning=high 리뷰 실행하여 504 해소 확인
