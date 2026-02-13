use aiproxy_common::config::{self, AIProxyConfig};
use aiproxy_common::session::ParticipantSession;
use aiproxy_common::sse::StopReason;
use aiproxy_common::sse::streaming;
use aiproxy_common::tools::{self, ToolDefinition};
use crate::events;
use serde_json::{json, Value};

const MAX_STEPS: usize = 100;

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

    let url = config.openai_url("/v1/responses");
    let token = config.openai_token().to_string();

    for step_num in 0..MAX_STEPS {
        let payload = json!({
            "model": "gpt-5.2",
            "tools": request_tools,
            "tool_choice": if request_tools.is_empty() { "none" } else { "auto" },
            "input": input_items,
            "instructions": system_prompt,
            "reasoning": { "effort": "medium" },
        });

        events::log_stderr(&format!(
            "[braintrust/openai] POST {} (step {}, streaming)", url, step_num
        ));

        let result = match streaming::stream_openai_responses(&client, &url, &token, payload).await {
            Ok(r) => r,
            Err(e) => {
                if !all_output_content.is_empty() {
                    session.finalize(all_output_content, true, Some(format!("Partial: {}", e)));
                    return Ok(session);
                }
                session.finalize(String::new(), false, Some(format!("GPT-5.2 error: {}", e)));
                return Ok(session);
            }
        };

        all_output_content.push_str(&result.text);

        if result.stop_reason == StopReason::ToolUse && !result.tool_calls.is_empty() {
            for tc in &result.tool_calls {
                let args: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));

                input_items.push(json!({
                    "type": "function_call",
                    "call_id": tc.id,
                    "name": tc.name,
                    "arguments": tc.arguments,
                }));

                events::emit_participant_step("openai", step_num, Some(&tc.name));

                let tool_result = tools::execute_tool(&tc.name, args.clone(), project_path).await;
                let tool_output = tool_result.as_ref().map(|s| s.clone()).map_err(|e| e.to_string());
                session.add_tool_call(&tc.name, args, tool_output);

                let output_str = match tool_result {
                    Ok(s) => s,
                    Err(e) => format!("Tool error: {}", e),
                };

                input_items.push(json!({
                    "type": "function_call_output",
                    "call_id": tc.id,
                    "output": output_str,
                }));
            }
            continue;
        }

        break;
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
) -> Result<aiproxy_common::session::AiResponse, Box<dyn std::error::Error + Send + Sync>> {
    let client = config::build_http_client();

    let payload = json!({
        "model": "gpt-5.2",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": prompt}
        ],
    });

    let url = config.openai_url("/v1/chat/completions");
    let token = config.openai_token().to_string();
    events::log_stderr(&format!("[braintrust/chair] POST {} (streaming)", url));

    let result = streaming::stream_openai_chat(&client, &url, &token, payload).await?;

    if result.text.is_empty() {
        return Err("OpenAI chair response missing content".into());
    }

    Ok(aiproxy_common::session::AiResponse {
        provider: "openai".to_string(),
        content: result.text,
        model: "gpt-5.2".to_string(),
        success: true,
        error: None,
    })
}
