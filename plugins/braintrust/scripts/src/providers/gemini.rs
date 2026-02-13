use crate::config::{self, AIProxyConfig};
use crate::events;
use crate::session::ParticipantSession;
use crate::sse::{SseParser, StreamAction, StopReason, IDLE_TIMEOUT};
use crate::sse::gemini::{parse_gemini_sse, GeminiSseState};
use crate::tools::{self, ToolDefinition};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::time::timeout;

const MAX_STEPS: usize = 100;

/// Gemini 3 Pro participant with tool loop (streamGenerateContent API, SSE streaming)
pub async fn call_gemini_participant(
    system_prompt: &str,
    user_prompt: &str,
    tools: &[ToolDefinition],
    project_path: &str,
    config: &AIProxyConfig,
) -> Result<ParticipantSession, Box<dyn std::error::Error + Send + Sync>> {
    let client = config::build_http_client();
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

        // Use streamGenerateContent with alt=sse
        let url = config.gemini_url("/v1beta/models/gemini-3-pro-preview:streamGenerateContent?alt=sse");
        let (auth_header, auth_value) = config.gemini_auth();
        events::log_stderr(&format!(
            "[braintrust/gemini] POST {} (step {}, streaming)", url, step_num
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

        // SSE streaming loop
        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();
        let mut state = GeminiSseState::new();
        let mut text_content = String::new();
        // function_calls: (name, args_json, thought_signature)
        let mut function_calls: Vec<(String, String, Option<String>)> = Vec::new();
        let mut stop_reason = StopReason::Unknown;

        loop {
            match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    for event in parser.feed(&chunk) {
                        for action in parse_gemini_sse(&event, &mut state) {
                            match action {
                                StreamAction::TextDelta { text, .. } => {
                                    text_content.push_str(&text);
                                }
                                StreamAction::ToolUseStart { name, thought_signature, .. } => {
                                    function_calls.push((name, String::new(), thought_signature));
                                }
                                StreamAction::InputJsonDelta { partial_json, .. } => {
                                    if let Some(fc) = function_calls.last_mut() {
                                        fc.1.push_str(&partial_json);
                                    }
                                }
                                StreamAction::MessageComplete { stop_reason: sr } => {
                                    stop_reason = sr;
                                }
                                StreamAction::Error(msg) => {
                                    return Err(format!("Gemini SSE error: {}", msg).into());
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok(Some(Err(e))) => return Err(format!("Gemini stream error: {}", e).into()),
                Ok(None) => break,
                Err(_) => {
                    if !text_content.is_empty() {
                        session.finalize(text_content, true, Some("Stream idle timeout (60s)".into()));
                        return Ok(session);
                    }
                    return Err("Gemini stream idle timeout (60s)".into());
                }
            }
        }
        // Double flush — Gemini's last event may not end with \n\n
        for event in parser.flush() {
            for action in parse_gemini_sse(&event, &mut state) {
                match action {
                    StreamAction::TextDelta { text, .. } => text_content.push_str(&text),
                    StreamAction::MessageComplete { stop_reason: sr } => stop_reason = sr,
                    StreamAction::ToolUseStart { name, thought_signature, .. } => {
                        function_calls.push((name, String::new(), thought_signature));
                    }
                    StreamAction::InputJsonDelta { partial_json, .. } => {
                        if let Some(fc) = function_calls.last_mut() {
                            fc.1.push_str(&partial_json);
                        }
                    }
                    _ => {}
                }
            }
        }

        if function_calls.is_empty() {
            if stop_reason == StopReason::EndTurn {
                session.finalize(text_content, true, None);
                return Ok(session);
            }
            session.finalize(
                text_content,
                false,
                Some(format!("Gemini stopped without tool calls (stop_reason={:?})", stop_reason)),
            );
            return Ok(session);
        }

        // Build model response for conversation history (preserve thoughtSignature!)
        let mut model_parts: Vec<Value> = Vec::new();
        if !text_content.is_empty() {
            model_parts.push(json!({"text": text_content}));
        }
        for (name, args_str, thought_sig) in &function_calls {
            let args: Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
            let mut fc_part = json!({"functionCall": {"name": name, "args": args}});
            if let Some(sig) = thought_sig {
                fc_part["thoughtSignature"] = json!(sig);
            }
            model_parts.push(fc_part);
        }
        contents.push(json!({"role": "model", "parts": model_parts}));

        // Execute tool calls
        for (name, args_str, _) in function_calls {
            events::emit_participant_step("gemini", step_num, Some(&name));

            let args: Value = serde_json::from_str(&args_str).unwrap_or_else(|_| json!({}));
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
