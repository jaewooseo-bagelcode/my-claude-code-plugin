---
name: codex-review
description: Professional code review and analysis using GPT-5.2-Codex (READ-ONLY, never modifies code). Analyzes bugs, security vulnerabilities, performance issues, and code quality. Provides detailed reports with actionable suggestions but does NOT implement fixes. Use when the user wants to understand code issues, find bugs, or get improvement suggestions. Triggers on phrases like "review this code", "analyze this", "find bugs in", "what's wrong with", "check security of", "audit this code", "is this code safe", "identify issues". NOT for implementing fixes - use codex-task-executor for that.
---

# Instructions

Execute Codex-powered code review with complete context preparation.

**IMPORTANT: This skill provides READ-ONLY analysis.** It identifies issues and provides suggestions but does NOT modify code. For implementing fixes, use `codex-task-executor`.

## Invocation

```bash
bash ${CLAUDE_PLUGIN_ROOT}/bin/codex-review.sh \
  --project-path "!`git rev-parse --show-toplevel`" \
  "<session-name>" "<review-context>"
```

**Session Name**: Generate using plan file pattern (adjective-verb-noun).
- Examples: "security-reviewing-turing", "auth-analyzing-hopper"
- Same name for follow-up questions in same review

**Review Context**: Structured context for Codex analysis (see Context Preparation below).

**Example**:
```python
review_context = """
Code Review Request:

FILES: src/auth/login.ts
FOCUS: Security - SQL injection
PRIORITY: Critical security first
"""

Bash(
    command=f'${{CLAUDE_PLUGIN_ROOT}}/bin/codex-review.sh --project-path "$(git rev-parse --show-toplevel)" "security-reviewing-turing" "{review_context}"',
    description="Security review"
)
```

## Context Preparation (Critical)

**Codex operates in headless execution mode and requires complete context upfront.**

### Use Conversation Context

**IMPORTANT: Check the conversation history before asking questions.**

If the user has already provided context in previous messages:
- Files mentioned or read in conversation
- Issues or bugs discussed
- Code snippets shared
- Error messages or logs

**Extract and use this information automatically.**

**Example conversation:**
```
User: "I'm getting SQL injection warnings in auth.ts"
User: "The login function at line 45 looks suspicious"
User: "Can you use codex to review it?"

You (Claude Code):
[Don't ask - you already have context!]
→ File: auth.ts
→ Focus: Security (SQL injection)
→ Specific: login function, line 45
→ Invoke with complete context immediately
```

### When Context is Missing

Before invoking codex-review, YOU (Claude Code) must gather and provide:

### 1. Files to Review (Required)
- Specific file paths (e.g., `src/auth.ts`)
- Use Read tool to preview files if needed
- Identify related files (imports, tests, middleware)

### 2. Focus Area (Required)
Specify what aspects to prioritize:
- **Security**: SQL injection, XSS, auth bypass, data exposure
- **Bugs**: Logic errors, null references, type issues, edge cases
- **Performance**: N+1 queries, algorithm efficiency, memory leaks
- **Code Quality**: Readability, naming, SOLID principles, duplication
- **Refactoring**: Structure improvements, design patterns
- **Comprehensive**: All aspects (default if unclear)

### 3. Scope (Required)
Define review boundary:
- Single file only
- File + related dependencies
- File + tests
- Entire module/directory

### 4. Context (Optional but Recommended)
- Specific bug or issue user is facing
- Recent changes (git diff if available)
- Production incidents or error logs
- Performance concerns or metrics
- Security vulnerabilities suspected

### 5. External Dependencies Context (Use Context7)

**CRITICAL: When code uses external libraries/frameworks, fetch latest documentation BEFORE invoking.**

Per anti-pattern rule #3: "외부 의존성... Context7나 WebSearch를 통해서 메뉴얼 탐독"

**Workflow:**
```
1. Detect libraries in code (React, FastAPI, Express, etc.)
2. Use Context7 to get latest best practices
3. Include in prompt to Codex

Example:
- Code uses React hooks → Query Context7 for React 19 best practices
- Code uses FastAPI → Query Context7 for FastAPI security guidelines
- Code uses SQL → Query Context7 for SQL injection prevention
```

**Pattern with Context7:**
```python
# Query Context7 first
context7_react = query_context7("React 19 best practices")

# Build enriched review context
review_context = """
Code Review Request:

FILES: src/Component.tsx (React component)

FOCUS: Best practices compliance + Security

EXTERNAL DEPENDENCIES (from Context7):
- React 19: Use new 'use' hook, avoid componentDidMount, Server Components best practices...

CONTEXT: Check for deprecated APIs and security issues
PRIORITY: Modern React compliance, then security
"""

Bash(
    command=f'${{CLAUDE_PLUGIN_ROOT}}/bin/codex-review.sh --project-path "$(git rev-parse --show-toplevel)" "{session_id}" "{review_context}"',
    description="React review"
)
```


## Session Management

**Session naming**: Use plan file pattern (adjective-verb-noun) for readable, unique names.

**Examples**:
- `security-reviewing-turing`
- `performance-auditing-knuth`
- `auth-analyzing-hopper`

**Follow-up**: Reuse same session name to continue conversation.

## Context Construction Workflow

### Step 1: Check Conversation History

Extract from previous messages:
- **Files**: Any file paths mentioned, code read, or files discussed
- **Issues**: Bugs, errors, performance problems mentioned
- **Focus**: Security concerns, logic errors, performance issues discussed
- **Code locations**: Specific functions, lines, or areas mentioned

### Step 2: Determine if Context is Sufficient

**Sufficient context = Can invoke immediately:**
- File path identified
- General focus area clear (even if not specific)
- Some context about why review is needed

**Insufficient context = Ask questions:**
- No file identified
- No indication of what to focus on
- Zero context

### Step 3A: If Context is Sufficient - Build Rich Prompt

Construct detailed prompt from conversation context:

```python
# Example prompt construction
prompt = f"""
Code Review Request:

FILES:
- {file_from_conversation} (primary)
{related_files_if_discussed}

FOCUS: {inferred_from_conversation}
{specific_issues_mentioned}

CONTEXT:
{summarize_relevant_conversation_context}

{any_specific_concerns_or_requirements}
"""
```

### Step 3B: If Context is Insufficient - Ask First

If user's request is vague **and conversation has no context**, ask clarifying questions:

```
User: "Review auth.ts"

You (Claude Code):
"I can help review auth.ts with Codex. To provide the most valuable analysis, I need:

1. **Focus**: Which aspect is most important?
   - Security vulnerabilities (SQL injection, XSS, auth bypass)
   - Bugs and logic errors
   - Performance issues
   - Code quality and maintainability
   - All aspects (comprehensive review)

2. **Scope**: Should I also review related files?
   - Just auth.ts
   - Include imported dependencies
   - Include tests

3. **Context**: Any specific concerns?
   - Known bugs or issues?
   - Recent production problems?
   - Specific functionality that's failing?"

[Wait for user answers]

[Then build complete context and invoke skill]
```

## Environment

**Prerequisites**:
- `codex` CLI installed (`npm install -g @openai/codex`)
- `codex login` completed (ChatGPT Pro subscription recommended)

**Optional**:
- `OPENAI_MODEL` — override model (default: `gpt-5.2-codex`)

**Sessions**: `{project}/.codex-sessions/` (project-isolated)

## Analysis Framework

Codex analyzes code across 5 dimensions:

- **Bugs & Debugging** (Critical): Logic errors, type mismatches, null references, runtime issues
- **Security** (High): Injections, XSS, auth flaws, data exposure
- **Performance** (Medium): Algorithm efficiency, N+1 queries, memory leaks
- **Code Quality** (Low): Readability, naming, duplication, SOLID principles
- **Refactoring**: Structural improvements, design patterns, abstractions

## Tools Available to Codex

Codex CLI runs in a **read-only sandbox** with built-in shell tools:
- `rg` (ripgrep): Code pattern search
- `cat -n`: File reading with line numbers
- `git diff`: Git diff for PR reviews
- `find`, `ls`: File discovery

## Complete Workflow Examples

### Example 1: Rich Conversation Context (Subagent Delegation)

```
[Earlier in conversation]
User: "I'm debugging auth.ts"
User: "The login function is throwing SQL errors"
User: "Line 45 looks suspicious - it's building queries with string concat"

[Current request]
User: "Can you use codex to review this?"

You (Claude Code):
[Extract context from conversation]
- File: auth.ts
- Focus: Security (SQL injection from string concatenation)
- Location: Line 45, login function
- Issue: SQL errors in production

[Build review context]
review_context = """
Code Review Request:

FILES:
- auth.ts (primary - login function)

FOCUS: Security (CRITICAL)
- SQL injection vulnerability
- String concatenation in query building (line 45)

CONTEXT:
- Production SQL errors
- Login function at line 45 uses string concatenation

PRIORITY: Security vulnerabilities first
"""

[Execute with Bash]
Bash(
    command=f'${{CLAUDE_PLUGIN_ROOT}}/bin/codex-review.sh --project-path "$(git rev-parse --show-toplevel)" "security-reviewing-turing" "{review_context}"',
    description="Security review"
)

[Tell user immediately]
"Delegated security review to subagent. Analyzing SQL injection vulnerability at line 45..."

[Subagent auto-returns results]
[Parse output and summarize]
"Review complete. Found critical SQL injection vulnerability in login function (line 45). Uses string concatenation instead of parameterized queries. Would you like me to implement the fix?"
```

### Example 2: Minimal Context - Ask First

```
User: "코드 리뷰해줘"

You (Claude Code):
[No context in conversation - ask first]

"I can help with code review using Codex. To provide the most valuable analysis:

1. **Which file(s)** should I review?
2. **What aspect** is most important?
   - Security, Bugs, Performance, Code Quality, or Comprehensive
3. **Any specific concerns** or issues you're facing?"

[Wait for response, then build context]
```

### Example 3: Partial Context - Infer and Proceed

```
[Earlier in conversation]
User: "Can you read src/auth/login.ts?"
[You read the file]

[Current request]
User: "Review this with codex"

You (Claude Code):
[Infer from context]
- File: src/auth/login.ts (just read)
- Focus: Comprehensive (not specified, default to all aspects)

[Execute with Bash]
review_context = """
FILES: src/auth/login.ts
FOCUS: Comprehensive review
PRIORITY: Security and bugs first
"""

Bash(
    command=f'${{CLAUDE_PLUGIN_ROOT}}/bin/codex-review.sh --project-path "$(git rev-parse --show-toplevel)" "comprehensive-reviewing-lovelace" "{review_context}"',
    description="Comprehensive review"
)
```


## Best Practices

1. **Use conversation context**: Don't ask if you already know
2. **Delegate to subagent**: Default to Bash subagent with haiku model for better UX
3. **Fetch latest docs with Context7**: When code uses external libraries, query Context7 BEFORE invoking
4. **Preview files**: Use Read tool to check file content and detect dependencies
5. **Identify related files**: Check imports, dependencies, tests
6. **Provide git diff**: If reviewing changes, include diff in context
7. **Be specific**: "Security audit for SQL injection" > "Review this"
8. **Batch related files**: Review login.ts + middleware.ts together rather than separately
9. **Default to comprehensive**: If focus unclear but file is clear, do comprehensive review
10. **Parallel reviews**: Use multiple subagents to review different files concurrently

### When to Use Context7

**AUTOMATICALLY query Context7 when code uses external libraries.**

Detect and fetch docs for:
- UI frameworks: React, Vue, Angular, Svelte
- Backend frameworks: FastAPI, Express, Django, Rails
- Databases: PostgreSQL, MongoDB, Redis
- Security libraries: JWT, OAuth, bcrypt
- Any external dependency for best practices/security guidelines

**Workflow (AUTOMATIC):**
```python
# Step 1: Read file and detect imports automatically
Read("src/api/auth.ts")
# → Detects: import express, jsonwebtoken, bcrypt

# Step 2: Query Context7 for each library
context7_express = query_context7("Express.js security best practices 2026")
context7_jwt = query_context7("JWT token validation security guidelines")
context7_bcrypt = query_context7("bcrypt password hashing best practices")

# Step 3: Build enriched review context
review_context = """
FILES: src/api/auth.ts
FOCUS: Security

EXTERNAL DEPENDENCIES (from Context7):
- Express.js: Use helmet middleware, validate inputs, prevent injection...
- JWT: Verify signature, check expiration, use strong secret...
- bcrypt: Use saltRounds >= 12, async methods only...

Check code compliance with these latest guidelines.
"""

# Step 4: Execute
Bash(
    command=f'${{CLAUDE_PLUGIN_ROOT}}/bin/codex-review.sh --project-path "$(git rev-parse --show-toplevel)" "{session_id}" "{review_context}"',
    description="Security review"
)
```

**This process should happen automatically** - don't ask user if they want Context7 docs.

## Cross-Model Verification (Post-Review)

After receiving Codex review output, **automatically spawn the `verify-review` agent** to cross-check findings against actual code. This provides cross-model verification: GPT-5.2 identifies issues, Claude verifies them by reading the actual source.

### When to Verify

- **Always verify** when the review contains Critical or High severity findings
- **Skip verification** when:
  - The review found no issues (score 9-10)
  - User explicitly requests raw review only (e.g., "skip verification", "raw review")
  - The review is a follow-up in an existing session

### How to Trigger

After receiving codex-review output, delegate to the `verify-review` agent:

```
Use the verify-review agent to validate the code review findings.
Pass it the complete review output and the project root path.
```

The agent reads actual source files with Read/Grep/Glob to verify each finding, then returns a structured verification report with Confirmed / False Positive / Needs Context verdicts.

### Presenting Results

After verification completes, present a **unified report**:

1. Show the original Codex review summary (issue counts, score)
2. Show the verification summary table (confirmed vs false positive counts)
3. **Highlight False Positives** prominently — these save developer time
4. If confidence rate is below 70%, note that the review may need human judgment
5. Recommend actions only for **confirmed Critical/High** findings

**Example presentation**:
```
## Code Review Results

### GPT-5.2-Codex Review
[Original review summary — issues found, overall score]

### Cross-Model Verification (Claude)
Verified: 8 findings | Confirmed: 6 | False Positive: 1 | Needs Context: 1
Confidence Rate: 75%

### Confirmed Action Items
1. [CRITICAL] SQL injection in auth.ts:45 — CONFIRMED
2. [HIGH] Missing CSRF token in form.tsx:12 — CONFIRMED

### False Positives (filtered out)
- [HIGH] "Hardcoded secret in config.ts:3" — actually reads from env var
```

## Reference Materials

**Load these when needed for better review quality:**

- **Security**: [references/common-vulnerabilities.md](references/common-vulnerabilities.md) - Security patterns and vulnerability examples
- **Code Quality**: [references/code-quality-patterns.md](references/code-quality-patterns.md) - Anti-patterns and best practices

## Appendix

*Human reference only (not for Claude):*

- Build guide: [appendix/BUILD.md](appendix/BUILD.md)
- Security analysis: [appendix/SECURITY.md](appendix/SECURITY.md)
