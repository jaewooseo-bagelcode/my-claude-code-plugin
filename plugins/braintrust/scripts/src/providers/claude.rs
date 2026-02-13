use crate::config::{self, AIProxyConfig};
use crate::events;
use crate::session::ParticipantSession;
use crate::sse::{SseParser, StreamAction, StopReason, IDLE_TIMEOUT};
use crate::sse::anthropic::parse_anthropic_sse;
use crate::tools::{self, ToolDefinition};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::time::timeout;

const MAX_STEPS: usize = 100;

/// Claude Opus 4.6 participant with tool loop (Messages API, SSE streaming)
pub async fn call_claude_participant(
    system_prompt: &str,
    user_prompt: &str,
    tools: &[ToolDefinition],
    project_path: &str,
    config: &AIProxyConfig,
) -> Result<ParticipantSession, Box<dyn std::error::Error + Send + Sync>> {
    let client = config::build_http_client();
    let mut messages = vec![json!({
        "role": "user",
        "content": user_prompt,
    })];
    let mut session = ParticipantSession::new("claude", "claude-opus-4-6");

    let tool_definitions: Vec<Value> = tools.iter().map(|tool| {
        json!({
            "name": tool.name,
            "description": tool.description,
            "input_schema": tool.parameters,
        })
    }).collect();

    for step_num in 0..MAX_STEPS {
        let request_body = json!({
            "model": "claude-opus-4-6",
            "max_tokens": 16000,
            "system": system_prompt,
            "tools": tool_definitions,
            "tool_choice": { "type": "auto" },
            "messages": messages,
            "thinking": { "type": "adaptive" },
            "stream": true,
        });

        let (auth_header, auth_value) = config.anthropic_auth();
        let url = config.anthropic_url("/v1/messages");
        events::log_stderr(&format!(
            "[braintrust/claude] POST {} (step {}, streaming)", url, step_num
        ));

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
            events::log_stderr(&format!(
                "[braintrust/claude] API error {}: {}",
                status, &body[..body.len().min(500)]
            ));
            session.finalize(String::new(), false, Some(format!("Claude API error ({}): {}", status, body)));
            return Ok(session);
        }

        // SSE streaming loop
        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();
        let mut stop_reason = StopReason::Unknown;

        // Accumulate content blocks: (block_type, id, name, content)
        // block_type: "text" or "tool_use"
        struct BlockAcc {
            block_type: String,
            id: String,
            name: String,
            content: String,
        }
        let mut blocks: Vec<BlockAcc> = Vec::new();
        let mut current_text_idx: Option<usize> = None;

        loop {
            match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    for event in parser.feed(&chunk) {
                        if let Some(action) = parse_anthropic_sse(&event) {
                            match action {
                                StreamAction::TextDelta { text, .. } => {
                                    if let Some(idx) = current_text_idx {
                                        blocks[idx].content.push_str(&text);
                                    } else {
                                        // New text block
                                        let idx = blocks.len();
                                        blocks.push(BlockAcc {
                                            block_type: "text".into(),
                                            id: String::new(),
                                            name: String::new(),
                                            content: text,
                                        });
                                        current_text_idx = Some(idx);
                                    }
                                }
                                StreamAction::ToolUseStart { id, name, .. } => {
                                    current_text_idx = None;
                                    blocks.push(BlockAcc {
                                        block_type: "tool_use".into(),
                                        id,
                                        name,
                                        content: String::new(),
                                    });
                                }
                                StreamAction::InputJsonDelta { partial_json, .. } => {
                                    if let Some(b) = blocks.last_mut() {
                                        if b.block_type == "tool_use" {
                                            b.content.push_str(&partial_json);
                                        }
                                    }
                                }
                                StreamAction::ContentBlockStop { .. } => {
                                    current_text_idx = None;
                                }
                                StreamAction::MessageComplete { stop_reason: sr } => {
                                    stop_reason = sr;
                                }
                                StreamAction::Error(msg) => {
                                    return Err(format!("Claude SSE error: {}", msg).into());
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok(Some(Err(e))) => return Err(format!("Claude stream error: {}", e).into()),
                Ok(None) => break,
                Err(_) => {
                    let has_text = blocks.iter().any(|b| b.block_type == "text" && !b.content.is_empty());
                    if has_text {
                        let text = blocks.iter()
                            .filter(|b| b.block_type == "text")
                            .map(|b| b.content.as_str())
                            .collect::<Vec<_>>()
                            .join("");
                        session.finalize(text, true, Some("Stream idle timeout (60s)".into()));
                        return Ok(session);
                    }
                    return Err("Claude stream idle timeout (60s)".into());
                }
            }
        }
        // Flush
        for event in parser.flush() {
            if let Some(action) = parse_anthropic_sse(&event) {
                match action {
                    StreamAction::TextDelta { text, .. } => {
                        if let Some(idx) = current_text_idx {
                            blocks[idx].content.push_str(&text);
                        }
                    }
                    StreamAction::MessageComplete { stop_reason: sr } => stop_reason = sr,
                    _ => {}
                }
            }
        }

        // Rebuild content_blocks JSON for the conversation history
        let content_blocks: Vec<Value> = blocks.iter().map(|b| {
            if b.block_type == "tool_use" {
                let input: Value = serde_json::from_str(&b.content).unwrap_or_else(|_| json!({}));
                json!({"type": "tool_use", "id": b.id, "name": b.name, "input": input})
            } else {
                json!({"type": "text", "text": b.content})
            }
        }).collect();

        let text_content: String = blocks.iter()
            .filter(|b| b.block_type == "text")
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join("");

        let tool_calls: Vec<&BlockAcc> = blocks.iter()
            .filter(|b| b.block_type == "tool_use")
            .collect();

        if stop_reason == StopReason::ToolUse {
            messages.push(json!({
                "role": "assistant",
                "content": content_blocks
            }));

            if tool_calls.is_empty() {
                continue;
            }

            for b in &tool_calls {
                events::emit_participant_step("claude", step_num, Some(&b.name));

                let input_result: Result<Value, String> = serde_json::from_str(&b.content)
                    .map_err(|e| format!("Invalid tool input JSON: {}", e));

                let (result_content, is_error) = match input_result {
                    Ok(args) => {
                        let tool_result = tools::execute_tool(&b.name, args.clone(), project_path).await;
                        let tool_output = match &tool_result {
                            Ok(s) => Ok(s.clone()),
                            Err(e) => Err(e.to_string()),
                        };
                        session.add_tool_call(&b.name, args, tool_output);
                        match tool_result {
                            Ok(result) => (result, false),
                            Err(err) => (format!("Tool execution error: {}", err), true),
                        }
                    }
                    Err(err) => {
                        session.add_tool_call(&b.name, json!({}), Err(err.clone()));
                        (err, true)
                    }
                };

                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": b.id,
                        "content": result_content,
                        "is_error": is_error
                    }]
                }));
            }

            continue;
        }

        if stop_reason == StopReason::EndTurn {
            session.finalize(text_content, true, None);
            return Ok(session);
        }

        session.finalize(
            text_content,
            false,
            Some(format!("Claude stopped unexpectedly (stop_reason={:?})", stop_reason)),
        );
        return Ok(session);
    }

    session.finalize(String::new(), false, Some("Claude tool loop exceeded maximum steps".to_string()));
    Ok(session)
}
