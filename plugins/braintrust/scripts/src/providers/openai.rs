use crate::config::{self, AIProxyConfig};
use crate::events;
use crate::session::ParticipantSession;
use crate::sse::{SseParser, StreamAction, StopReason, IDLE_TIMEOUT};
use crate::sse::openai::{parse_openai_responses_sse, parse_openai_chat_sse};
use crate::tools::{self, ToolDefinition};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::time::timeout;

const MAX_TOOL_CALLS: usize = 100;

/// GPT-5.2 participant with tool loop (Responses API, SSE streaming)
pub async fn call_gpt52_participant(
    system_prompt: &str,
    user_prompt: &str,
    tools: &[ToolDefinition],
    project_path: &str,
    config: &AIProxyConfig,
) -> Result<ParticipantSession, Box<dyn std::error::Error + Send + Sync>> {
    let client = config::build_http_client();
    let mut session = ParticipantSession::new("openai", "gpt-5.2");

    let request_tools: Vec<Value> = tools.iter().map(|tool| {
        json!({
            "type": "function",
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters
        })
    }).collect();

    let mut input_items: Vec<Value> = vec![
        json!({"role": "user", "content": user_prompt})
    ];
    let mut all_output_content = String::new();

    for step_num in 0..MAX_TOOL_CALLS {
        let request_body = json!({
            "model": "gpt-5.2",
            "tools": request_tools,
            "tool_choice": if request_tools.is_empty() { "none" } else { "auto" },
            "input": input_items,
            "instructions": system_prompt,
            "reasoning": { "effort": "medium" },
            "stream": true,
        });

        let url = config.openai_url("/v1/responses");
        let token = config.openai_token();
        events::log_stderr(&format!(
            "[braintrust/openai] POST {} (step {}, streaming)", url, step_num
        ));

        let response = match client
            .post(&url)
            .bearer_auth(token)
            .json(&request_body)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                if all_output_content.is_empty() {
                    return Err(format!("OpenAI API request failed: {}", e).into());
                }
                session.finalize(all_output_content, true, Some(format!("Partial: {}", e)));
                return Ok(session);
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            events::log_stderr(&format!(
                "[braintrust/openai] API error {}: {}",
                status, &body[..body.len().min(500)]
            ));
            session.finalize(String::new(), false, Some(format!("GPT-5.2 API error ({}): {}", status, body)));
            return Ok(session);
        }

        // SSE streaming loop
        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();
        let mut step_text = String::new();
        // tool_calls: (call_id, name, args_json_buffer)
        let mut tool_calls: Vec<(String, String, String)> = Vec::new();
        let mut stop_reason = StopReason::Unknown;

        loop {
            match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    for event in parser.feed(&chunk) {
                        if let Some(action) = parse_openai_responses_sse(&event) {
                            match action {
                                StreamAction::TextDelta { text, .. } => {
                                    step_text.push_str(&text);
                                }
                                StreamAction::ToolUseStart { id, name, .. } => {
                                    tool_calls.push((id, name, String::new()));
                                }
                                StreamAction::InputJsonDelta { partial_json, .. } => {
                                    if let Some(tc) = tool_calls.last_mut() {
                                        tc.2.push_str(&partial_json);
                                    }
                                }
                                StreamAction::InputJsonFinal { json, .. } => {
                                    // Replace delta buffer with final JSON
                                    if let Some(tc) = tool_calls.last_mut() {
                                        tc.2 = json;
                                    }
                                }
                                StreamAction::MessageComplete { stop_reason: sr } => {
                                    stop_reason = sr;
                                }
                                StreamAction::Error(msg) => {
                                    return Err(format!("OpenAI SSE error: {}", msg).into());
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok(Some(Err(e))) => return Err(format!("OpenAI stream error: {}", e).into()),
                Ok(None) => break, // Stream ended
                Err(_) => {
                    // Idle timeout
                    if !all_output_content.is_empty() || !step_text.is_empty() {
                        all_output_content.push_str(&step_text);
                        session.finalize(all_output_content, true, Some("Stream idle timeout (60s)".into()));
                        return Ok(session);
                    }
                    return Err("OpenAI stream idle timeout (60s)".into());
                }
            }
        }
        // Flush remaining
        for event in parser.flush() {
            if let Some(action) = parse_openai_responses_sse(&event) {
                match action {
                    StreamAction::TextDelta { text, .. } => step_text.push_str(&text),
                    StreamAction::MessageComplete { stop_reason: sr } => stop_reason = sr,
                    _ => {}
                }
            }
        }

        all_output_content.push_str(&step_text);

        // Process tool calls
        if stop_reason == StopReason::ToolUse && !tool_calls.is_empty() {
            for (call_id, name, args_str) in tool_calls {
                let args: Value = serde_json::from_str(&args_str).unwrap_or_else(|_| json!({}));

                input_items.push(json!({
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": args_str,
                }));

                events::emit_participant_step("openai", step_num, Some(&name));

                let tool_result = tools::execute_tool(&name, args.clone(), project_path).await;
                let tool_output = match &tool_result {
                    Ok(s) => Ok(s.clone()),
                    Err(e) => Err(e.to_string()),
                };
                session.add_tool_call(&name, args, tool_output);

                let output_str = match tool_result {
                    Ok(result) => result,
                    Err(e) => format!("Tool error: {}", e),
                };

                input_items.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output_str
                }));
            }
            continue; // Next tool loop iteration
        }

        break; // No tool calls, done
    }

    if all_output_content.is_empty() {
        session.finalize(String::new(), false, Some("Empty response from GPT-5.2".to_string()));
    } else {
        session.finalize(all_output_content, true, None);
    }
    Ok(session)
}

/// GPT-5.2 chair (Chat Completions API, SSE streaming, no tools)
pub async fn call_gpt52_chair(
    system_prompt: &str,
    prompt: &str,
    config: &AIProxyConfig,
) -> Result<crate::session::AiResponse, Box<dyn std::error::Error + Send + Sync>> {
    let client = config::build_http_client();

    let request_body = json!({
        "model": "gpt-5.2",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": prompt}
        ],
        "stream": true,
    });

    let url = config.openai_url("/v1/chat/completions");
    let token = config.openai_token();
    events::log_stderr(&format!("[braintrust/chair] POST {} (streaming)", url));

    let response = client
        .post(&url)
        .bearer_auth(token)
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Chair API error ({}): {}", status, body).into());
    }

    // SSE streaming loop
    let mut parser = SseParser::new();
    let mut byte_stream = response.bytes_stream();
    let mut content = String::new();

    loop {
        match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for event in parser.feed(&chunk) {
                    if let Some(action) = parse_openai_chat_sse(&event) {
                        match action {
                            StreamAction::TextDelta { text, .. } => content.push_str(&text),
                            StreamAction::Error(msg) => {
                                return Err(format!("Chair SSE error: {}", msg).into());
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Some(Err(e))) => return Err(format!("Chair stream error: {}", e).into()),
            Ok(None) => break,
            Err(_) => {
                if !content.is_empty() {
                    // Return partial content on idle timeout
                    break;
                }
                return Err("Chair stream idle timeout (60s)".into());
            }
        }
    }
    // Flush
    for event in parser.flush() {
        if let Some(StreamAction::TextDelta { text, .. }) = parse_openai_chat_sse(&event) {
            content.push_str(&text);
        }
    }

    if content.is_empty() {
        return Err("OpenAI chair response missing content".into());
    }

    Ok(crate::session::AiResponse {
        provider: "openai".to_string(),
        content,
        model: "gpt-5.2".to_string(),
        success: true,
        error: None,
    })
}
