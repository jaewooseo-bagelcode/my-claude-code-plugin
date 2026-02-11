use crate::config::AIProxyConfig;
use crate::events;
use crate::session::ParticipantSession;
use crate::tools::{self, ToolDefinition};
use serde_json::{json, Value};

const MAX_STEPS: usize = 100;

/// Claude Opus 4.6 participant with tool loop (Messages API)
pub async fn call_claude_participant(
    system_prompt: &str,
    user_prompt: &str,
    tools: &[ToolDefinition],
    project_path: &str,
    config: &AIProxyConfig,
) -> Result<ParticipantSession, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
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
            "thinking": { "type": "adaptive" }
        });

        let (auth_header, auth_value) = config.anthropic_auth();
        let url = config.anthropic_url("/v1/messages");
        events::log_stderr(&format!(
            "[braintrust/claude] POST {} (step {})", url, step_num
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

        let response_json: Value = response.json().await?;
        let stop_reason = response_json.get("stop_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let content_blocks = response_json.get("content")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut text_fragments = Vec::new();
        let mut tool_calls: Vec<(String, String, Result<Value, String>)> = Vec::new();

        for block in &content_blocks {
            match block.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        text_fragments.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    let id = block.get("id").and_then(|v| v.as_str())
                        .ok_or("Claude tool_use block missing id")?.to_string();
                    let name = block.get("name").and_then(|v| v.as_str())
                        .ok_or("Claude tool_use block missing name")?.to_string();
                    let raw_input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    let parsed_input = if let Some(input_str) = raw_input.as_str() {
                        serde_json::from_str(input_str)
                            .map_err(|e| format!("Invalid tool input JSON: {}", e))
                    } else {
                        Ok(raw_input)
                    };
                    tool_calls.push((id, name, parsed_input));
                }
                Some("thinking") => {}
                _ => {}
            }
        }

        if stop_reason == "tool_use" {
            messages.push(json!({
                "role": "assistant",
                "content": content_blocks
            }));

            if tool_calls.is_empty() {
                continue;
            }

            for (tool_use_id, name, input_result) in tool_calls {
                events::emit_participant_step("claude", step_num, Some(&name));

                let (content, is_error) = match input_result {
                    Ok(args) => {
                        let tool_result = tools::execute_tool(&name, args.clone(), project_path).await;
                        let tool_output = match &tool_result {
                            Ok(s) => Ok(s.clone()),
                            Err(e) => Err(e.to_string()),
                        };
                        session.add_tool_call(&name, args, tool_output);
                        match tool_result {
                            Ok(result) => (result, false),
                            Err(err) => (format!("Tool execution error: {}", err), true),
                        }
                    }
                    Err(err) => {
                        session.add_tool_call(&name, json!({}), Err(err.clone()));
                        (err, true)
                    }
                };

                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                        "is_error": is_error
                    }]
                }));
            }

            continue;
        }

        if stop_reason == "end_turn" {
            session.finalize(text_fragments.join(""), true, None);
            return Ok(session);
        }

        session.finalize(
            text_fragments.join(""),
            false,
            Some(format!("Claude stopped unexpectedly (stop_reason={})", stop_reason)),
        );
        return Ok(session);
    }

    session.finalize(String::new(), false, Some("Claude tool loop exceeded maximum steps".to_string()));
    Ok(session)
}
