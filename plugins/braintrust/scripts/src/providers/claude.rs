use aiproxy_common::config::{self, AIProxyConfig};
use aiproxy_common::session::ParticipantSession;
use aiproxy_common::sse::StopReason;
use aiproxy_common::sse::streaming;
use aiproxy_common::tools::{self, ToolDefinition};
use crate::events;
use serde_json::{json, Value};

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

    let (auth_header, auth_value) = config.anthropic_auth();
    let url = config.anthropic_url("/v1/messages");

    for step_num in 0..MAX_STEPS {
        let payload = json!({
            "model": "claude-opus-4-6",
            "max_tokens": 16000,
            "system": system_prompt,
            "tools": tool_definitions,
            "tool_choice": { "type": "auto" },
            "messages": messages,
            "thinking": { "type": "adaptive" },
        });

        events::log_stderr(&format!(
            "[braintrust/claude] POST {} (step {}, streaming)", url, step_num
        ));

        let result = match streaming::stream_anthropic(&client, &url, &auth_header, &auth_value, payload).await {
            Ok(r) => r,
            Err(e) => {
                session.finalize(String::new(), false, Some(format!("Claude error: {}", e)));
                return Ok(session);
            }
        };

        // Rebuild content_blocks for conversation history
        let mut content_blocks: Vec<Value> = Vec::new();
        if !result.text.is_empty() {
            content_blocks.push(json!({"type": "text", "text": result.text}));
        }
        for tc in &result.tool_calls {
            let input: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
            content_blocks.push(json!({"type": "tool_use", "id": tc.id, "name": tc.name, "input": input}));
        }

        if result.stop_reason == StopReason::ToolUse && !result.tool_calls.is_empty() {
            messages.push(json!({
                "role": "assistant",
                "content": content_blocks
            }));

            for tc in &result.tool_calls {
                events::emit_participant_step("claude", step_num, Some(&tc.name));

                let (result_content, is_error) = match serde_json::from_str::<Value>(&tc.arguments) {
                    Ok(args) => {
                        let tool_result = tools::execute_tool(&tc.name, args.clone(), project_path).await;
                        let tool_output = tool_result.as_ref().map(|s| s.clone()).map_err(|e| e.to_string());
                        session.add_tool_call(&tc.name, args, tool_output);
                        match tool_result {
                            Ok(r) => (r, false),
                            Err(err) => (format!("Tool execution error: {}", err), true),
                        }
                    }
                    Err(e) => {
                        let err = format!("Invalid tool input JSON: {}", e);
                        session.add_tool_call(&tc.name, json!({}), Err(err.clone()));
                        (err, true)
                    }
                };

                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tc.id,
                        "content": result_content,
                        "is_error": is_error
                    }]
                }));
            }

            continue;
        }

        if result.stop_reason == StopReason::EndTurn {
            session.finalize(result.text, true, None);
            return Ok(session);
        }

        session.finalize(
            result.text,
            false,
            Some(format!("Claude stopped unexpectedly (stop_reason={:?})", result.stop_reason)),
        );
        return Ok(session);
    }

    session.finalize(String::new(), false, Some("Claude tool loop exceeded maximum steps".to_string()));
    Ok(session)
}
