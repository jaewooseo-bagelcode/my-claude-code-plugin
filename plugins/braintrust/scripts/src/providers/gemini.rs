use crate::config::AIProxyConfig;
use crate::events;
use crate::session::ParticipantSession;
use crate::tools::{self, ToolDefinition};
use serde_json::{json, Value};

const MAX_STEPS: usize = 100;

/// Gemini 3 Pro participant with tool loop (generateContent API)
pub async fn call_gemini_participant(
    system_prompt: &str,
    user_prompt: &str,
    tools: &[ToolDefinition],
    project_path: &str,
    config: &AIProxyConfig,
) -> Result<ParticipantSession, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let mut contents = vec![json!({
        "role": "user",
        "parts": [{ "text": user_prompt }]
    })];
    let mut session = ParticipantSession::new("gemini", "gemini-3-pro-preview");

    let function_declarations: Vec<Value> = tools.iter().map(|tool| {
        json!({
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        })
    }).collect();

    let tools_payload = json!([{ "functionDeclarations": function_declarations }]);

    for step_num in 0..MAX_STEPS {
        let request_body = json!({
            "contents": contents,
            "tools": tools_payload,
            "toolConfig": {
                "functionCallingConfig": {
                    "mode": "AUTO"
                }
            },
            "systemInstruction": {
                "parts": [{ "text": system_prompt }]
            }
        });

        let url = config.gemini_url("/v1beta/models/gemini-3-pro-preview:generateContent");
        let (auth_header, auth_value) = config.gemini_auth();
        events::log_stderr(&format!(
            "[braintrust/gemini] POST {} (step {})", url, step_num
        ));

        let response = client
            .post(&url)
            .header(auth_header, &auth_value)
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            events::log_stderr(&format!(
                "[braintrust/gemini] API error {}: {}",
                status, &body[..body.len().min(500)]
            ));
            session.finalize(String::new(), false, Some(format!("Gemini API error ({}): {}", status, body)));
            return Ok(session);
        }

        let response_json: Value = response.json().await?;
        let candidate = response_json.get("candidates")
            .and_then(|v| v.get(0))
            .ok_or("Gemini response missing candidates")?;

        let finish_reason = candidate.get("finishReason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let content = candidate.get("content").cloned().unwrap_or_else(|| json!({}));
        let parts = content.get("parts")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut text_fragments = Vec::new();
        let mut function_calls: Vec<(String, Value)> = Vec::new();

        for part in &parts {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                text_fragments.push(text.to_string());
            }
            if let Some(fc) = part.get("functionCall") {
                let name = fc.get("name").and_then(|v| v.as_str())
                    .ok_or("Gemini function call missing name")?.to_string();
                let raw_args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                let args = if let Some(arg_str) = raw_args.as_str() {
                    serde_json::from_str(arg_str)?
                } else {
                    raw_args
                };
                function_calls.push((name, args));
            }
        }

        if function_calls.is_empty() {
            if finish_reason == "STOP" {
                session.finalize(text_fragments.join(""), true, None);
                return Ok(session);
            }
            session.finalize(
                text_fragments.join(""),
                false,
                Some(format!("Gemini stopped without tool calls (finishReason={})", finish_reason)),
            );
            return Ok(session);
        }

        // Add model response to conversation
        let mut model_content = content;
        if model_content.get("role").is_none() {
            if let Some(obj) = model_content.as_object_mut() {
                obj.insert("role".to_string(), json!("model"));
            }
        }
        contents.push(model_content);

        // Execute tool calls
        for (name, args) in function_calls {
            events::emit_participant_step("gemini", step_num, Some(&name));

            let tool_result = tools::execute_tool(&name, args.clone(), project_path).await;
            session.add_tool_call(
                &name,
                args.clone(),
                tool_result.as_ref().map(|r| r.clone()).map_err(|e| e.to_string()),
            );

            let result_str = match tool_result {
                Ok(r) => r,
                Err(e) => format!("Tool execution error: {}", e),
            };

            contents.push(json!({
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "name": name,
                        "response": { "ok": true, "result": result_str }
                    }
                }]
            }));
        }
    }

    session.finalize(String::new(), false, Some("Gemini tool loop exceeded maximum steps".to_string()));
    Ok(session)
}
