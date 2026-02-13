use aiproxy_common::config::{self, AIProxyConfig};
use aiproxy_common::session::ParticipantSession;
use aiproxy_common::sse::StopReason;
use aiproxy_common::sse::streaming;
use aiproxy_common::tools::{self, ToolDefinition};
use crate::events;
use serde_json::{json, Value};

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

    let url = config.gemini_url("/v1beta/models/gemini-3-pro-preview:streamGenerateContent?alt=sse");
    let (auth_header, auth_value) = config.gemini_auth();

    for step_num in 0..MAX_STEPS {
        let payload = json!({
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

        events::log_stderr(&format!(
            "[braintrust/gemini] POST {} (step {}, streaming)", url, step_num
        ));

        let result = match streaming::stream_gemini(&client, &url, &auth_header, &auth_value, payload).await {
            Ok(r) => r,
            Err(e) => {
                session.finalize(String::new(), false, Some(format!("Gemini error: {}", e)));
                return Ok(session);
            }
        };

        if result.tool_calls.is_empty() {
            if result.stop_reason == StopReason::EndTurn {
                session.finalize(result.text, true, None);
                return Ok(session);
            }
            session.finalize(
                result.text,
                false,
                Some(format!("Gemini stopped without tool calls (stop_reason={:?})", result.stop_reason)),
            );
            return Ok(session);
        }

        // Build model response for conversation history (preserve thoughtSignature!)
        let mut model_parts: Vec<Value> = Vec::new();
        if !result.text.is_empty() {
            model_parts.push(json!({"text": result.text}));
        }
        for tc in &result.tool_calls {
            let args: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
            let mut fc_part = json!({"functionCall": {"name": tc.name, "args": args}});
            if let Some(ref sig) = tc.thought_signature {
                fc_part["thoughtSignature"] = json!(sig);
            }
            model_parts.push(fc_part);
        }
        contents.push(json!({"role": "model", "parts": model_parts}));

        for tc in &result.tool_calls {
            events::emit_participant_step("gemini", step_num, Some(&tc.name));

            let args: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
            let tool_result = tools::execute_tool(&tc.name, args.clone(), project_path).await;
            let tool_output = tool_result.as_ref().map(|s| s.clone()).map_err(|e| e.to_string());
            session.add_tool_call(&tc.name, args, tool_output);

            let output_str = match tool_result {
                Ok(s) => s,
                Err(e) => format!("Tool execution error: {}", e),
            };

            contents.push(json!({
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "name": tc.name,
                        "response": { "ok": true, "result": output_str }
                    }
                }]
            }));
        }
    }

    session.finalize(String::new(), false, Some("Gemini tool loop exceeded maximum steps".to_string()));
    Ok(session)
}
