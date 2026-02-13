use aiproxy_common::session::{JsonlLogger, summarize_args};
use aiproxy_common::sse::streaming;
use aiproxy_common::tools;
use serde_json::{json, Value};
use std::time::Instant;
use tokio::task::JoinSet;

const DEFAULT_API_BASE: &str = "https://aiproxy-api.backoffice.bagelgames.com/openai/v1";

fn get_api_base() -> String {
    std::env::var("AIPROXY_API_BASE")
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|_| DEFAULT_API_BASE.to_string())
}

/// Get tool definitions for code review (CodeReview naming: Glob, Grep, Read, GitDiff)
fn get_tools_schema() -> Vec<Value> {
    let tool_defs = tools::build_tool_definitions(tools::ToolSet::CodeReview);
    tool_defs
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters
            })
        })
        .collect()
}

/// Execute the review loop using OpenAI Responses API with SSE streaming.
/// Uses previous_response_id chaining for session continuity.
/// Returns the last response ID for session persistence.
pub async fn execute_review(
    api_key: &str,
    model: &str,
    reasoning_effort: &str,
    system_prompt: &str,
    previous_response_id: Option<&str>,
    review_prompt: &str,
    repo_root: &str,
    max_iters: usize,
    logger: &JsonlLogger,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    // No overall timeout — idle timeout (60s) in SSE loop handles hangs
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    let api_base = get_api_base();
    let url = format!("{}/responses", api_base);
    let tools_schema = get_tools_schema();

    let mut last_response_id: Option<String> = None;

    // Initial input: user's review prompt
    let mut input_items: Vec<Value> = vec![json!({
        "role": "user",
        "content": review_prompt
    })];

    for iteration in 0..max_iters {
        let iter_start = Instant::now();
        logger.log(
            "iteration_start",
            iteration,
            Some(json!({"iteration": iteration})),
        );

        let mut payload = json!({
            "model": model,
            "tools": tools_schema,
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "input": input_items,
        });

        // System prompt via instructions field
        if !system_prompt.is_empty() {
            payload["instructions"] = json!(system_prompt);
        }

        // Chain to previous response for session continuity
        if let Some(ref id) = last_response_id {
            payload["previous_response_id"] = json!(id);
        } else if let Some(prev_id) = previous_response_id {
            payload["previous_response_id"] = json!(prev_id);
        }

        if !reasoning_effort.is_empty() {
            payload["reasoning"] = json!({"effort": reasoning_effort});
        }

        // Call Responses API with SSE streaming
        logger.log(
            "api_call_start",
            iteration,
            Some(json!({"model": model, "streaming": true})),
        );
        let api_start = Instant::now();

        let result = streaming::stream_openai_responses(&client, &url, api_key, payload)
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                let api_duration = api_start.elapsed().as_millis() as u64;
                logger.log(
                    "api_call_error",
                    iteration,
                    Some(json!({"duration_ms": api_duration, "error": e.to_string()})),
                );
                e.to_string().into()
            })?;

        let api_duration = api_start.elapsed().as_millis() as u64;

        // Track response ID for chaining
        if let Some(ref id) = result.response_id {
            last_response_id = Some(id.clone());
        }

        logger.log(
            "api_call_success",
            iteration,
            Some(json!({
                "duration_ms": api_duration,
                "response_id": result.response_id.as_deref().unwrap_or(""),
                "stop_reason": format!("{:?}", result.stop_reason),
            })),
        );

        // Print output text immediately
        if !result.text.is_empty() {
            print!("{}", result.text);
        }

        if result.tool_calls.is_empty() {
            // No tool calls => review complete
            logger.log(
                "review_complete",
                iteration,
                Some(json!({"reason": "no_tool_calls"})),
            );
            return Ok(last_response_id);
        }

        // Execute tool calls in parallel
        let mut join_set = JoinSet::new();

        for (i, tc) in result.tool_calls.iter().enumerate() {
            let call_id = tc.id.clone();
            let name = tc.name.clone();
            let args_str = tc.arguments.clone();
            let repo = repo_root.to_string();

            join_set.spawn(async move {
                let args: Value =
                    serde_json::from_str(&args_str).unwrap_or_else(|_| json!({}));

                let tool_start = Instant::now();
                let result = tools::execute_tool(&name, args, &repo).await;
                let tool_duration = tool_start.elapsed().as_millis() as u64;

                let (output, ok) = match result {
                    Ok(s) => (s, true),
                    Err(e) => (format!("{{\"ok\": false, \"error\": \"{}\"}}", e), false),
                };

                (i, call_id, name, args_str, ok, tool_duration, output)
            });
        }

        // Collect results in order
        let mut results: Vec<(usize, String, String, String, bool, u64, String)> = Vec::new();
        while let Some(join_result) = join_set.join_next().await {
            if let Ok(r) = join_result {
                results.push(r);
            }
        }
        results.sort_by_key(|(i, ..)| *i);

        // Log tool calls and build output items
        let mut outputs: Vec<Value> = Vec::new();
        for (_, call_id, name, args_str, ok, tool_duration, output) in results {
            logger.log(
                "tool_call",
                iteration,
                Some(json!({
                    "tool": name,
                    "args": summarize_args(&args_str, 200),
                    "ok": ok,
                    "duration_ms": tool_duration,
                })),
            );
            outputs.push(json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": output,
            }));
        }

        logger.log(
            "iteration_end",
            iteration,
            Some(json!({
                "tool_count": result.tool_calls.len(),
                "duration_ms": iter_start.elapsed().as_millis() as u64,
            })),
        );

        // Set tool results as next input
        input_items = outputs;
    }

    logger.log(
        "review_complete",
        max_iters.saturating_sub(1),
        Some(json!({"reason": "max_iterations"})),
    );
    Err(format!("reached MAX_ITERS={} without completion", max_iters).into())
}
