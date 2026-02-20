use aiproxy_common::config::{self, AIProxyConfig};
use aiproxy_common::sse::{SseParser, StreamAction, IDLE_TIMEOUT};
use aiproxy_common::sse::anthropic::parse_anthropic_sse;
use aiproxy_common::tools;
use aiproxy_common::session::{AiResponse, ParticipantSession};
use crate::events;
use crate::providers;
use crate::session::{self, BraintrustIteration, BraintrustResult};
use futures_util::StreamExt;
use serde_json::json;
use std::time::Instant;
use tokio::time::timeout;

const MAX_RETRIES: u32 = 3;
const RETRY_BASE_DELAY_MS: u64 = 2000;

pub struct BraintrustRequest {
    pub agenda: String,
    pub context: Option<String>,
    pub project_path: String,
    pub max_iterations: u32,
    pub chair_model: String,
}

pub async fn run_braintrust(
    request: BraintrustRequest,
) -> Result<BraintrustResult, Box<dyn std::error::Error + Send + Sync>> {
    let start = Instant::now();
    let meeting_id = uuid::Uuid::new_v4().to_string();
    let max_iterations = request.max_iterations;

    // Load config
    let config = aiproxy_common::config::load_config()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;

    session::log_debug(
        &meeting_id,
        "info", "system", "meeting_start",
        &format!("Braintrust meeting started: {} (max_iterations={})", meeting_id, max_iterations),
        Some(json!({ "agenda": &request.agenda, "max_iterations": max_iterations })),
    );

    events::emit_meeting_started(&meeting_id, &request.agenda);

    let meta = session::create_meeting_meta(&meeting_id, &request.agenda, request.context.as_deref());
    let _ = session::save_meeting_meta(&meta);

    let participant_system_prompt = build_participant_system_prompt(&request.project_path);
    let tool_defs = tools::build_tool_definitions(tools::ToolSet::Full);

    let chair_system_prompt = build_chair_system_prompt();

    // Iteration loop
    let mut all_iterations: Vec<BraintrustIteration> = Vec::new();
    let mut current_question = request.agenda.clone();

    for iter_num in 0..max_iterations {
        events::emit_iteration_started(iter_num, max_iterations, &current_question);

        session::log_debug(
            &meeting_id,
            "info", "system", "iteration_start",
            &format!("Iteration {} starting", iter_num),
            Some(json!({ "iteration": iter_num, "question": &current_question })),
        );

        let participant_prompt = if iter_num == 0 {
            build_participant_prompt(&request.agenda, request.context.as_deref())
        } else {
            build_followup_participant_prompt(&current_question, &request.agenda, request.context.as_deref())
        };

        // Run 3 participants in parallel
        events::emit_participant_started("GPT-5.3", "gpt-5.3");
        events::emit_participant_started("Gemini", "gemini-3.1-pro-preview");
        events::emit_participant_started("Claude", "claude-opus-4-6");

        let gpt_start = Instant::now();
        let gemini_start = Instant::now();
        let claude_start = Instant::now();

        let (gpt_result, gemini_result, claude_result) = tokio::join!(
            run_participant_with_retry("openai", &participant_system_prompt, &participant_prompt, &tool_defs, &request.project_path, &config, &meeting_id),
            run_participant_with_retry("gemini", &participant_system_prompt, &participant_prompt, &tool_defs, &request.project_path, &config, &meeting_id),
            run_participant_with_retry("claude", &participant_system_prompt, &participant_prompt, &tool_defs, &request.project_path, &config, &meeting_id),
        );

        let gpt_session = gpt_result?;
        let gemini_session = gemini_result?;
        let claude_session = claude_result?;

        events::emit_participant_completed("GPT-5.3", gpt_session.success, gpt_start.elapsed().as_millis() as u64);
        events::emit_participant_completed("Gemini", gemini_session.success, gemini_start.elapsed().as_millis() as u64);
        events::emit_participant_completed("Claude", claude_session.success, claude_start.elapsed().as_millis() as u64);

        session::log_debug(
            &meeting_id, "info", "system", "participants_completed",
            &format!("Iteration {} participants completed", iter_num),
            Some(json!({
                "iteration": iter_num,
                "gpt_success": gpt_session.success,
                "gemini_success": gemini_session.success,
                "claude_success": claude_session.success,
            })),
        );

        let iteration = BraintrustIteration {
            iteration: iter_num,
            question: current_question.clone(),
            participant_sessions: vec![gpt_session, gemini_session, claude_session],
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };

        let _ = session::save_iteration(&meeting_id, &iteration);
        all_iterations.push(iteration);

        // Last iteration: skip chair analysis
        if iter_num >= max_iterations - 1 {
            break;
        }

        // Chair analysis: determine if follow-up needed
        events::emit_chair_analyzing(iter_num);

        session::log_debug(
            &meeting_id, "info", "chair", "analysis_start",
            &format!("Chair analyzing iteration {}", iter_num),
            Some(json!({ "iteration": iter_num })),
        );

        let analysis_prompt = build_chair_analysis_prompt(
            &request.agenda,
            request.context.as_deref(),
            &all_iterations,
        );

        match run_chair(&chair_system_prompt, &analysis_prompt, &config, &request.chair_model).await {
            Ok(analysis_response) => {
                let content = analysis_response.content.trim().to_string();

                session::log_debug(
                    &meeting_id, "info", "chair", "analysis_complete",
                    &format!("Chair analysis complete for iteration {}", iter_num),
                    Some(json!({ "iteration": iter_num, "decision": if content.starts_with("CONTINUE:") { "continue" } else { "done" } })),
                );

                if let Some(follow_up) = content.strip_prefix("CONTINUE:") {
                    let next_question = follow_up.trim().to_string();
                    if next_question.is_empty() {
                        break;
                    }
                    events::emit_chair_follow_up(iter_num, &next_question);
                    current_question = next_question;
                } else {
                    // "DONE" or other → sufficient
                    break;
                }
            }
            Err(e) => {
                session::log_debug(
                    &meeting_id, "error", "chair", "analysis_error",
                    &format!("Chair analysis failed: {}", e),
                    Some(json!({ "iteration": iter_num, "error": e.to_string() })),
                );
                events::log_stderr(&format!("[braintrust] Chair analysis failed: {}, proceeding to synthesis", e));
                break;
            }
        }
    }

    let total_iterations = all_iterations.len() as u32;

    // Final chair synthesis
    events::emit_chair_synthesizing();

    session::log_debug(
        &meeting_id, "info", "chair", "synthesis_start",
        "Chair starting final synthesis",
        Some(json!({ "total_iterations": total_iterations })),
    );

    let chair_prompt = build_final_synthesis_prompt(
        &request.agenda,
        request.context.as_deref(),
        &all_iterations,
    );

    let chair_start = Instant::now();
    let chair_response = run_chair(&chair_system_prompt, &chair_prompt, &config, &request.chair_model).await?;
    let chair_elapsed = chair_start.elapsed().as_millis() as u64;

    events::emit_chair_completed(chair_elapsed);

    session::log_debug(
        &meeting_id, "info", "chair", "synthesis_complete",
        &format!("Chair synthesis completed in {}ms", chair_elapsed),
        Some(json!({ "elapsed_ms": chair_elapsed })),
    );

    let _ = session::save_chair_summary(&meeting_id, &chair_response);

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let _ = session::update_meeting_status(&meeting_id, "completed", elapsed_ms);

    session::log_debug(
        &meeting_id, "info", "system", "meeting_complete",
        &format!("Meeting completed in {}ms ({} iterations)", elapsed_ms, total_iterations),
        Some(json!({ "elapsed_ms": elapsed_ms, "total_iterations": total_iterations })),
    );

    // Build raw_responses from last iteration
    let last_iteration = all_iterations.last();
    let raw_responses: Vec<AiResponse> = last_iteration
        .map(|it| it.participant_sessions.iter().map(|s| s.to_ai_response()).collect())
        .unwrap_or_default();

    events::emit_meeting_completed(elapsed_ms, total_iterations);

    Ok(BraintrustResult {
        meeting_id,
        summary: chair_response.content,
        raw_responses,
        iterations: all_iterations,
        total_iterations,
        elapsed_ms,
    })
}

pub struct ResumeRequest {
    pub meeting_id: String,
    pub project_path: String, // still needed for tool execution (glob/grep/read/git_diff)
    pub max_iterations: u32,
    pub chair_model: String,
}

pub async fn resume_braintrust(
    request: ResumeRequest,
) -> Result<BraintrustResult, Box<dyn std::error::Error + Send + Sync>> {
    let start = Instant::now();
    let meeting_id = request.meeting_id;
    let max_iterations = request.max_iterations;

    // Load previous meeting data
    let meta = session::load_meeting_meta(&meeting_id)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;

    let mut all_iterations = session::load_iterations(&meeting_id)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;

    let config = aiproxy_common::config::load_config()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;

    events::emit_meeting_started(&meeting_id, &format!("[RESUME] {}", meta.agenda));

    let participant_system_prompt = build_participant_system_prompt(&request.project_path);
    let tool_defs = tools::build_tool_definitions(tools::ToolSet::Full);
    let chair_system_prompt = build_chair_system_prompt();

    let prev_count = all_iterations.len() as u32;
    events::log_stderr(&format!("[braintrust] Resuming meeting {} with {} previous rounds", meeting_id, prev_count));

    // Ask chair what to explore next based on previous iterations
    let analysis_prompt = build_chair_analysis_prompt(
        &meta.agenda,
        meta.context.as_deref(),
        &all_iterations,
    );

    let mut current_question = match run_chair(&chair_system_prompt, &analysis_prompt, &config, &request.chair_model).await {
        Ok(response) => {
            let content = response.content.trim().to_string();
            if let Some(follow_up) = content.strip_prefix("CONTINUE:") {
                follow_up.trim().to_string()
            } else {
                // Chair says DONE - just re-synthesize with existing data
                events::log_stderr("[braintrust] Chair says previous discussion was sufficient, re-synthesizing...");
                let chair_prompt = build_final_synthesis_prompt(&meta.agenda, meta.context.as_deref(), &all_iterations);
                let chair_response = run_chair(&chair_system_prompt, &chair_prompt, &config, &request.chair_model).await?;
                let _ = session::save_chair_summary(&meeting_id, &chair_response);
                let elapsed_ms = start.elapsed().as_millis() as u64;
                let _ = session::update_meeting_status(&meeting_id, "completed", elapsed_ms);

                let total_iterations = all_iterations.len() as u32;
                let raw_responses = all_iterations.last()
                    .map(|it| it.participant_sessions.iter().map(|s| s.to_ai_response()).collect())
                    .unwrap_or_default();

                return Ok(BraintrustResult {
                    meeting_id,
                    summary: chair_response.content,
                    raw_responses,
                    iterations: all_iterations,
                    total_iterations,
                    elapsed_ms,
                });
            }
        }
        Err(e) => {
            return Err(format!("Chair failed to analyze previous rounds: {}", e).into());
        }
    };

    // Continue with additional rounds
    for iter_num in 0..max_iterations {
        let global_iter = prev_count + iter_num;
        events::emit_iteration_started(global_iter, prev_count + max_iterations, &current_question);

        let participant_prompt = build_followup_participant_prompt(&current_question, &meta.agenda, meta.context.as_deref());

        events::emit_participant_started("GPT-5.3", "gpt-5.3");
        events::emit_participant_started("Gemini", "gemini-3.1-pro-preview");
        events::emit_participant_started("Claude", "claude-opus-4-6");

        let gpt_start = Instant::now();
        let gemini_start = Instant::now();
        let claude_start = Instant::now();

        let (gpt_result, gemini_result, claude_result) = tokio::join!(
            run_participant_with_retry("openai", &participant_system_prompt, &participant_prompt, &tool_defs, &request.project_path, &config, &meeting_id),
            run_participant_with_retry("gemini", &participant_system_prompt, &participant_prompt, &tool_defs, &request.project_path, &config, &meeting_id),
            run_participant_with_retry("claude", &participant_system_prompt, &participant_prompt, &tool_defs, &request.project_path, &config, &meeting_id),
        );

        let gpt_session = gpt_result?;
        let gemini_session = gemini_result?;
        let claude_session = claude_result?;

        events::emit_participant_completed("GPT-5.3", gpt_session.success, gpt_start.elapsed().as_millis() as u64);
        events::emit_participant_completed("Gemini", gemini_session.success, gemini_start.elapsed().as_millis() as u64);
        events::emit_participant_completed("Claude", claude_session.success, claude_start.elapsed().as_millis() as u64);

        session::log_debug(
            &meeting_id, "info", "system", "participants_completed",
            &format!("Resume iteration {} participants completed", global_iter),
            Some(json!({
                "iteration": global_iter,
                "gpt_success": gpt_session.success,
                "gemini_success": gemini_session.success,
                "claude_success": claude_session.success,
            })),
        );

        let iteration = BraintrustIteration {
            iteration: global_iter,
            question: current_question.clone(),
            participant_sessions: vec![gpt_session, gemini_session, claude_session],
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };

        let _ = session::save_iteration(&meeting_id, &iteration);
        all_iterations.push(iteration);

        if iter_num >= max_iterations - 1 {
            break;
        }

        // Chair analysis
        events::emit_chair_analyzing(global_iter);

        session::log_debug(
            &meeting_id, "info", "chair", "analysis_start",
            &format!("Chair analyzing resume iteration {}", global_iter),
            Some(json!({ "iteration": global_iter })),
        );

        let analysis_prompt = build_chair_analysis_prompt(&meta.agenda, meta.context.as_deref(), &all_iterations);

        match run_chair(&chair_system_prompt, &analysis_prompt, &config, &request.chair_model).await {
            Ok(analysis_response) => {
                let content = analysis_response.content.trim().to_string();

                session::log_debug(
                    &meeting_id, "info", "chair", "analysis_complete",
                    &format!("Chair analysis complete for resume iteration {}", global_iter),
                    Some(json!({ "iteration": global_iter, "decision": if content.starts_with("CONTINUE:") { "continue" } else { "done" } })),
                );

                if let Some(follow_up) = content.strip_prefix("CONTINUE:") {
                    let next_question = follow_up.trim().to_string();
                    if next_question.is_empty() { break; }
                    events::emit_chair_follow_up(global_iter, &next_question);
                    current_question = next_question;
                } else {
                    break;
                }
            }
            Err(e) => {
                session::log_debug(
                    &meeting_id, "error", "chair", "analysis_error",
                    &format!("Chair analysis failed on resume: {}", e),
                    Some(json!({ "iteration": global_iter, "error": e.to_string() })),
                );
                events::log_stderr(&format!("[braintrust] Chair analysis failed: {}, proceeding to synthesis", e));
                break;
            }
        }
    }

    // Final synthesis
    events::emit_chair_synthesizing();

    session::log_debug(
        &meeting_id, "info", "chair", "synthesis_start",
        "Chair starting final synthesis (resume)",
        Some(json!({ "total_iterations": all_iterations.len() })),
    );

    let chair_prompt = build_final_synthesis_prompt(&meta.agenda, meta.context.as_deref(), &all_iterations);
    let chair_start = Instant::now();
    let chair_response = run_chair(&chair_system_prompt, &chair_prompt, &config, &request.chair_model).await?;
    let chair_elapsed = chair_start.elapsed().as_millis() as u64;
    let _ = session::save_chair_summary(&meeting_id, &chair_response);

    session::log_debug(
        &meeting_id, "info", "chair", "synthesis_complete",
        &format!("Chair synthesis completed in {}ms (resume)", chair_elapsed),
        Some(json!({ "elapsed_ms": chair_elapsed })),
    );

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let _ = session::update_meeting_status(&meeting_id, "completed", elapsed_ms);

    let total_iterations = all_iterations.len() as u32;
    let raw_responses = all_iterations.last()
        .map(|it| it.participant_sessions.iter().map(|s| s.to_ai_response()).collect())
        .unwrap_or_default();

    session::log_debug(
        &meeting_id, "info", "system", "meeting_complete",
        &format!("Resumed meeting completed in {}ms ({} iterations)", elapsed_ms, total_iterations),
        Some(json!({ "elapsed_ms": elapsed_ms, "total_iterations": total_iterations })),
    );

    events::emit_meeting_completed(elapsed_ms, total_iterations);

    Ok(BraintrustResult {
        meeting_id,
        summary: chair_response.content,
        raw_responses,
        iterations: all_iterations,
        total_iterations,
        elapsed_ms,
    })
}

async fn run_participant_with_retry(
    provider: &str,
    system_prompt: &str,
    user_prompt: &str,
    tools: &[tools::ToolDefinition],
    project_path: &str,
    config: &AIProxyConfig,
    meeting_id: &str,
) -> Result<ParticipantSession, Box<dyn std::error::Error + Send + Sync>> {
    let mut last_error: Option<Box<dyn std::error::Error + Send + Sync>> = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay_ms = RETRY_BASE_DELAY_MS * (1 << (attempt - 1));
            session::log_debug(
                meeting_id, "warn", provider, "retry",
                &format!("Retry {}/{} after {}ms", attempt, MAX_RETRIES - 1, delay_ms),
                Some(json!({ "attempt": attempt, "delay_ms": delay_ms })),
            );
            events::log_stderr(&format!("[braintrust/{}] Retry {}/{} after {}ms...", provider, attempt, MAX_RETRIES - 1, delay_ms));
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }

        session::log_debug(
            meeting_id, "debug", provider, "api_call_start",
            &format!("{} API call attempt {}", provider, attempt + 1),
            Some(json!({ "attempt": attempt + 1 })),
        );
        let call_start = Instant::now();

        let result = match provider {
            "openai" => providers::openai::call_gpt53_participant(system_prompt, user_prompt, tools, project_path, config).await,
            "gemini" => providers::gemini::call_gemini_participant(system_prompt, user_prompt, tools, project_path, config).await,
            "claude" => providers::claude::call_claude_participant(system_prompt, user_prompt, tools, project_path, config).await,
            _ => return Err(format!("Unknown provider: {}", provider).into()),
        };

        let elapsed_ms = call_start.elapsed().as_millis() as u64;

        match result {
            Ok(session) => {
                session::log_debug(
                    meeting_id, "info", provider, "api_call_success",
                    &format!("{} completed in {}ms", provider, elapsed_ms),
                    Some(json!({ "elapsed_ms": elapsed_ms })),
                );
                return Ok(session);
            }
            Err(e) => {
                session::log_debug(
                    meeting_id, "error", provider, "api_call_error",
                    &format!("{} attempt {} failed: {}", provider, attempt + 1, e),
                    Some(json!({ "elapsed_ms": elapsed_ms, "error": e.to_string() })),
                );
                events::log_stderr(&format!("[braintrust/{}] Attempt {} failed: {}", provider, attempt + 1, e));
                last_error = Some(e);
            }
        }
    }

    // All retries exhausted — return a failed session instead of error (graceful degradation)
    let err_msg = last_error.map(|e| e.to_string()).unwrap_or_else(|| "Unknown error".to_string());
    session::log_debug(
        meeting_id, "error", provider, "all_retries_exhausted",
        &format!("{} failed after {} retries: {}", provider, MAX_RETRIES, err_msg),
        Some(json!({ "retries": MAX_RETRIES, "error": &err_msg })),
    );

    let mut session = ParticipantSession::new(provider, "unknown");
    session.finalize(
        format!("[{} failed after {} retries: {}]", provider, MAX_RETRIES, err_msg),
        false,
        Some(err_msg),
    );
    Ok(session)
}

async fn run_chair(
    system_prompt: &str,
    prompt: &str,
    config: &AIProxyConfig,
    chair_model: &str,
) -> Result<AiResponse, Box<dyn std::error::Error + Send + Sync>> {
    // Use GPT-5.2 as default chair (or Claude if specified)
    if chair_model.starts_with("claude") {
        run_claude_chair(system_prompt, prompt, config).await
    } else {
        providers::openai::call_gpt53_chair(system_prompt, prompt, config).await
    }
}

async fn run_claude_chair(
    system_prompt: &str,
    prompt: &str,
    config: &AIProxyConfig,
) -> Result<AiResponse, Box<dyn std::error::Error + Send + Sync>> {
    let client = config::build_http_client();

    let request_body = json!({
        "model": "claude-opus-4-6",
        "max_tokens": 16000,
        "system": system_prompt,
        "messages": [{"role": "user", "content": prompt}],
        "thinking": { "type": "enabled", "budget_tokens": 10000 },
        "stream": true,
    });

    let (auth_header, auth_value) = config.anthropic_auth();
    let url = config.anthropic_url("/v1/messages");
    events::log_stderr(&format!("[braintrust/chair] POST {} (claude, streaming)", url));

    let response = client
        .post(&url)
        .header(auth_header, &auth_value)
        .header("anthropic-version", "2023-06-01")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Chair Claude API error ({}): {}", status, body).into());
    }

    // SSE streaming loop
    let mut parser = SseParser::new();
    let mut byte_stream = response.bytes_stream();
    let mut content = String::new();

    loop {
        match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for event in parser.feed(&chunk) {
                    if let Some(action) = parse_anthropic_sse(&event) {
                        match action {
                            StreamAction::TextDelta { text, .. } => content.push_str(&text),
                            StreamAction::Error(msg) => {
                                return Err(format!("Chair Claude SSE error: {}", msg).into());
                            }
                            _ => {} // thinking, content_block_stop, message_complete — ignored for chair
                        }
                    }
                }
            }
            Ok(Some(Err(e))) => return Err(format!("Chair Claude stream error: {}", e).into()),
            Ok(None) => break,
            Err(_) => {
                if !content.is_empty() {
                    // Return partial content on idle timeout
                    break;
                }
                return Err("Chair Claude stream idle timeout (60s)".into());
            }
        }
    }
    // Flush
    for event in parser.flush() {
        if let Some(StreamAction::TextDelta { text, .. }) = parse_anthropic_sse(&event) {
            content.push_str(&text);
        }
    }

    if content.is_empty() {
        return Err("No text in Claude chair response".into());
    }

    Ok(AiResponse {
        provider: "claude".to_string(),
        content,
        model: "claude-opus-4-6".to_string(),
        success: true,
        error: None,
    })
}

// =============================================================================
// Prompt builders
// =============================================================================

fn build_participant_system_prompt(project_path: &str) -> String {
    let mut prompt = format!(
        r#"당신은 로컬 저장소에서 작동하는 코드베이스 분석 어시스턴트입니다.
저장소 루트: {project_path}

다음 네 가지 읽기 전용 도구를 사용할 수 있습니다:
- glob_files(pattern): 저장소 루트 기준 glob 패턴으로 파일 검색
- grep_content(pattern, glob): 파일 내 텍스트 검색, glob으로 범위 제한 가능
- read_file(file_path): 파일의 내용 읽기
- git_diff(): 커밋되지 않은 변경사항 확인

## 규칙
1) 저장소 내용을 추측하지 마라. 사실이 필요하면 도구를 사용하라.
2) 도구 사용을 최소화하라: grep_content/glob_files로 위치 파악, read_file로 필요한 부분만 확인.
3) 비밀 정보를 요청하지 마라. 요청받으면 거부하고 이유를 설명하라.
4) 충분한 정보가 모이면 직접적인 답변을 제공하라.
5) 코드를 참조할 때는 파일 경로와 라인 범위를 명시하라.
6) 항상 한국어로 응답하라."#
    );

    // Append project memory if available
    if let Some(memory) = aiproxy_common::config::load_project_memory(project_path) {
        prompt.push_str(&format!("\n\n## 프로젝트 메모리\n{}", memory));
    }

    prompt
}

fn build_participant_prompt(agenda: &str, context: Option<&str>) -> String {
    let mut prompt = format!(
        r#"## Braintrust 회의 참여

**안건:**
{agenda}
"#
    );

    if let Some(ctx) = context {
        prompt.push_str("\n**맥락:**\n");
        prompt.push_str(ctx);
        prompt.push('\n');
    }

    prompt.push_str("\n도구를 사용하여 근거를 수집하고, 안건에 대해 분석 의견을 제시하세요.");
    prompt
}

fn build_followup_participant_prompt(question: &str, original_agenda: &str, context: Option<&str>) -> String {
    let mut prompt = format!(
        r#"## Braintrust 회의 참여 (추가 질문)

**원래 안건:**
{original_agenda}
"#
    );

    if let Some(ctx) = context {
        prompt.push_str("\n**맥락:**\n");
        prompt.push_str(ctx);
        prompt.push('\n');
    }

    prompt.push_str(&format!(
        "\n**의장의 추가 질문:**\n{question}\n\n도구를 사용하여 근거를 수집하고, 위 질문에 대해 분석 의견을 제시하세요.",
    ));
    prompt
}

fn build_chair_system_prompt() -> String {
    "You are the chair of a Braintrust meeting — a multi-AI deliberation system. Your role is to analyze participant responses, identify gaps, and synthesize consensus. Always respond in Korean.".to_string()
}

fn format_iterations_block(iterations: &[BraintrustIteration]) -> String {
    let mut out = String::new();
    for iter in iterations {
        out.push_str(&format!("\n=== Round {} ===\n", iter.iteration + 1));
        out.push_str(&format!("Question: {}\n", iter.question));
        for ps in &iter.participant_sessions {
            let status = if ps.success { "" } else { " [FAILED]" };
            out.push_str(&format!("\n{}{}: {}\n", ps.provider, status, ps.final_content));
        }
    }
    out
}

fn build_chair_analysis_prompt(
    agenda: &str,
    context: Option<&str>,
    iterations: &[BraintrustIteration],
) -> String {
    let mut prompt = format!("You are the Braintrust chair reviewing participant responses.\n\nOriginal Agenda:\n{agenda}\n");
    if let Some(ctx) = context {
        prompt.push_str("\nContext:\n");
        prompt.push_str(ctx);
        prompt.push('\n');
    }
    prompt.push_str(&format_iterations_block(iterations));
    prompt.push_str(
        r#"
## Task

Review ALL responses above. Decide if follow-up questions are needed.

**Rules:**
1. If participants missed important aspects, ask a focused follow-up question.
2. If responses are contradictory, ask for clarification.
3. If sufficient information has been gathered, end the discussion.
4. Ask only ONE question per round.

**Output format (CRITICAL):**
- If follow-up needed: "CONTINUE: [your question in Korean]"
- If sufficient: "DONE"
"#,
    );
    prompt
}

fn build_final_synthesis_prompt(
    agenda: &str,
    context: Option<&str>,
    iterations: &[BraintrustIteration],
) -> String {
    let mut prompt = format!("You are the Braintrust chair synthesizing multi-round discussion.\n\nOriginal Agenda:\n{agenda}\n");
    if let Some(ctx) = context {
        prompt.push_str("\nContext:\n");
        prompt.push_str(ctx);
        prompt.push('\n');
    }
    prompt.push_str(&format_iterations_block(iterations));
    prompt.push_str(
        r#"
## Task

Based on ALL rounds of discussion, produce a structured meeting report in Korean.

### Confidence 레벨
- **H (High)**: 강한 확신, 명확한 근거
- **M (Medium)**: 중간 확신, 합리적 추론
- **L (Low)**: 약한 확신, 가정 기반

### Evidence 등급
- **A**: 공식 문서, 벤치마크 데이터
- **B**: 업계 표준, Best Practice
- **C**: 논리적 추론
- **D**: 추측, 개인 의견

### 출력 형식 (반드시 이 형식을 따르세요)

## 브레인트러스트 회의록

### 주제
[안건 요약]

### AI별 핵심 주장 (Claims)

#### GPT-5.2
| Claim | Evidence | Confidence |
|-------|----------|------------|
| [주장] | [근거] (등급) | H/M/L |

#### Gemini 3 Pro
| Claim | Evidence | Confidence |
|-------|----------|------------|
| [주장] | [근거] (등급) | H/M/L |

#### Claude Opus 4.6
| Claim | Evidence | Confidence |
|-------|----------|------------|
| [주장] | [근거] (등급) | H/M/L |

### 의견 비교
| 항목 | GPT-5.2 | Gemini 3 Pro | Claude Opus 4.6 |
|------|---------|--------------|-----------------|
| 핵심 관점 | ... | ... | ... |
| 강조점 | ... | ... | ... |
| 독특한 시각 | ... | ... | ... |

### 합의점 (Consensus)
[세 AI가 동의하는 부분 - Confidence H인 것 우선]

### 분기점 (Divergence)
[의견이 다른 부분과 각 AI의 근거 비교]

### 종합 분석
[의장으로서의 종합적인 분석]

### 권고
⭐ **최선의 선택**: [가장 권장하는 옵션과 이유]
**대안**: [차선책이 있다면]
"#,
    );
    prompt
}
