use crate::config::AIProxyConfig;
use crate::events;
use crate::session::ParticipantSession;
use crate::tools::{self, ToolDefinition};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_TOOL_CALLS: usize = 100;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GptResponse {
    id: Option<String>,
    output: Option<Vec<OutputItem>>,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OutputItem {
    #[serde(rename = "type")]
    item_type: String,
    name: Option<String>,
    call_id: Option<String>,
    arguments: Option<String>,
    content: Option<Vec<MessageContent>>,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

/// GPT-5.2 participant with tool loop (Responses API)
pub async fn call_gpt52_participant(
    system_prompt: &str,
    user_prompt: &str,
    tools: &[ToolDefinition],
    project_path: &str,
    config: &AIProxyConfig,
) -> Result<ParticipantSession, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
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
        });

        let url = config.openai_url("/v1/responses");
        let token = config.openai_token();
        events::log_stderr(&format!(
            "[braintrust/openai] POST {} (step {})", url, step_num
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
        let response_text = response.text().await?;

        if !status.is_success() {
            events::log_stderr(&format!(
                "[braintrust/openai] API error {}: {}",
                status, &response_text[..response_text.len().min(500)]
            ));
            session.finalize(String::new(), false, Some(format!("GPT-5.2 API error ({}): {}", status, response_text)));
            return Ok(session);
        }

        let gpt_response: GptResponse = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse GPT response: {}", e))?;

        let output_items = match gpt_response.output {
            Some(items) => items,
            None => return Err("No output in GPT response".into()),
        };

        let mut has_tool_calls = false;

        for item in &output_items {
            match item.item_type.as_str() {
                "message" => {
                    if let Some(content_blocks) = &item.content {
                        for block in content_blocks {
                            if block.content_type == "output_text" {
                                if let Some(text) = &block.text {
                                    all_output_content.push_str(text);
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    if let (Some(name), Some(call_id)) = (&item.name, &item.call_id) {
                        has_tool_calls = true;
                        let args_str = item.arguments.as_deref().unwrap_or("{}");
                        let args: Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));

                        input_items.push(json!({
                            "type": "function_call",
                            "call_id": call_id,
                            "name": name,
                            "arguments": args_str,
                        }));

                        events::emit_participant_step("openai", step_num, Some(name));

                        let tool_result = tools::execute_tool(name, args.clone(), project_path).await;
                        let tool_output = match &tool_result {
                            Ok(s) => Ok(s.clone()),
                            Err(e) => Err(e.to_string()),
                        };
                        session.add_tool_call(name, args, tool_output);

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
                }
                _ => {} // reasoning, etc
            }
        }

        if !has_tool_calls {
            break;
        }
    }

    if all_output_content.is_empty() {
        session.finalize(String::new(), false, Some("Empty response from GPT-5.2".to_string()));
    } else {
        session.finalize(all_output_content, true, None);
    }
    Ok(session)
}

/// GPT-5.2 chair (Chat Completions API, no tools)
pub async fn call_gpt52_chair(
    system_prompt: &str,
    prompt: &str,
    config: &AIProxyConfig,
) -> Result<crate::session::AiResponse, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();

    let request_body = json!({
        "model": "gpt-5.2",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": prompt}
        ]
    });

    let url = config.openai_url("/v1/chat/completions");
    let token = config.openai_token();
    events::log_stderr(&format!("[braintrust/chair] POST {} (non-streaming)", url));

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

    let result: Value = response.json().await?;

    let content = result
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or("OpenAI response missing content")?;

    Ok(crate::session::AiResponse {
        provider: "openai".to_string(),
        content: content.to_string(),
        model: "gpt-5.2".to_string(),
        success: true,
        error: None,
    })
}
