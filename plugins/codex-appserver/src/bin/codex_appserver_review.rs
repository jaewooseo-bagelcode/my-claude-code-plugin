//! CLI binary: run a code review via Codex App Server.
//!
//! Usage:
//!   codex-appserver-review --project-path <path> --model <model> <session-name> <prompt-file>

use std::path::{Path, PathBuf};
use std::time::Duration;

use codex_appserver::appserver::protocol::{review_output_schema, ReviewOutput, Severity};
use codex_appserver::appserver::CodexAppServerClient;
use serde_json::{json, Value};

fn parse_args() -> Result<(PathBuf, String, String, PathBuf), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut project_path: Option<PathBuf> = None;
    let mut model: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--project-path" => {
                i += 1;
                project_path =
                    Some(PathBuf::from(args.get(i).ok_or("Missing --project-path value")?));
            }
            "--model" => {
                i += 1;
                model = Some(args.get(i).ok_or("Missing --model value")?.clone());
            }
            "--help" | "-h" => {
                eprintln!(
                    "Usage: codex-appserver-review --project-path <path> [--model <model>] <session-name> <prompt-file>"
                );
                std::process::exit(0);
            }
            _ => {
                positional.push(args[i].clone());
            }
        }
        i += 1;
    }

    let project_path = project_path.ok_or("--project-path is required")?;
    if !project_path.is_dir() {
        return Err(format!(
            "--project-path does not exist or is not a directory: {}",
            project_path.display()
        ));
    }
    let model = model.unwrap_or_else(|| "gpt-5.4".to_string());

    if positional.len() != 2 {
        return Err(format!(
            "Usage: codex-appserver-review --project-path <path> [--model <model>] <session-name> <prompt-file>\n\
             Got {} positional args: {:?}",
            positional.len(),
            positional
        ));
    }

    let session_name = positional[0].clone();
    let prompt_file = PathBuf::from(&positional[1]);

    if !prompt_file.is_file() {
        return Err(format!(
            "Prompt file does not exist: {}",
            prompt_file.display()
        ));
    }

    Ok((project_path, model, session_name, prompt_file))
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let (project_path, model, session_name, prompt_file) = parse_args()?;

    let prompt = std::fs::read_to_string(&prompt_file)
        .map_err(|e| format!("Failed to read prompt file {}: {e}", prompt_file.display()))?;

    if prompt.trim().is_empty() {
        return Err("Prompt file is empty".to_string());
    }

    eprintln!("Spawning codex app-server...");
    let mut client = CodexAppServerClient::spawn().await?;

    // 1. Initialize handshake
    eprintln!("Initializing...");
    client
        .request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "codex-appserver-review",
                    "version": "0.1.0"
                },
                "capabilities": {}
            }),
        )
        .await?;

    client.notify("initialized", Value::Null).await?;

    // 2. Create thread
    eprintln!("Creating thread (model: {model}, sandbox: read-only)...");
    let thread_result = client
        .request(
            "thread/start",
            json!({
                "model": model,
                "cwd": project_path.to_string_lossy(),
                "sandbox": "read-only",
                "approvalPolicy": "never"
            }),
        )
        .await?;

    let thread_id = thread_result
        .get("thread")
        .and_then(|t| t.get("id"))
        .or_else(|| thread_result.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("thread/start did not return a thread id"))?
        .to_string();

    eprintln!("Thread created: {}", &thread_id[..thread_id.len().min(16)]);

    // 3. Start turn with prompt + outputSchema
    eprintln!("Starting review turn...");
    client.clear_text().await;
    let turn_result = client
        .request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": [{ "type": "text", "text": prompt }],
                "outputSchema": review_output_schema()
            }),
        )
        .await?;

    // Extract turn ID for correlation
    let turn_id = turn_result
        .get("id")
        .or_else(|| turn_result.get("turn").and_then(|t| t.get("id")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 4. Wait for matching turn/completed
    let turn_timeout = parse_turn_timeout();
    eprintln!("Waiting for review completion (timeout: {}s)...", turn_timeout.as_secs());
    let completed = client
        .wait_turn_completed(turn_id.as_deref(), turn_timeout)
        .await?;

    // 5. Check for turn-level error
    let turn_obj = completed.get("turn");
    let status = turn_obj
        .and_then(|t| t.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");

    match status {
        "completed" => {}
        "interrupted" => {
            return Err("Turn was interrupted before completion".to_string());
        }
        "failed" => {
            let err_msg = turn_obj
                .and_then(|t| t.get("error"))
                .map(|e| {
                    e.get("message")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| e.to_string())
                })
                .unwrap_or_else(|| "unknown error".to_string());
            return Err(format!("Turn failed: {err_msg}"));
        }
        other => {
            return Err(format!("Unexpected turn status: {other}"));
        }
    }

    // 6. Parse accumulated agent text as structured output.
    let agent_text = client.accumulated_text().await;

    if agent_text.is_empty() {
        return Err("Agent produced no output text".to_string());
    }

    let review = parse_last_review_output(&agent_text)?;

    // 7. Save to cache
    let cache_dir = project_path.join(".codex-review-cache/reviews");
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create cache dir: {e}"))?;

    save_review_json(&cache_dir, &session_name, &review)?;
    save_review_markdown(&cache_dir, &session_name, &review)?;

    // 8. Print summary
    print_summary(&session_name, &cache_dir, &review);

    // 9. Shutdown
    eprintln!("Shutting down app server...");
    let status = client.shutdown().await;
    if !status.is_clean() {
        eprintln!("Warning: shutdown was not fully clean:");
        if let Err(e) = &status.shutdown_request {
            eprintln!("  shutdown request: {e}");
        }
        if let Err(e) = &status.exit_notify {
            eprintln!("  exit notification: {e}");
        }
        if !status.process_exited {
            eprintln!("  process did not exit within timeout (kill_on_drop will handle)");
        }
    }

    Ok(())
}

fn save_review_json(
    cache_dir: &Path,
    session_name: &str,
    review: &ReviewOutput,
) -> Result<(), String> {
    let path = cache_dir.join(format!("{session_name}.json"));
    let json = serde_json::to_string_pretty(review).map_err(|e| format!("JSON serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Write {}: {e}", path.display()))?;
    eprintln!("Saved: {}", path.display());
    Ok(())
}

fn save_review_markdown(
    cache_dir: &Path,
    session_name: &str,
    review: &ReviewOutput,
) -> Result<(), String> {
    let path = cache_dir.join(format!("{session_name}.md"));
    let mut md = String::new();

    md.push_str(&format!("# Code Review: {session_name}\n\n"));
    md.push_str(&format!("**Score**: {}/10\n\n", review.score));
    md.push_str(&format!("## Summary\n\n{}\n\n", review.summary));

    if !review.strengths.is_empty() {
        md.push_str("## Strengths\n\n");
        for s in &review.strengths {
            md.push_str(&format!("- {s}\n"));
        }
        md.push('\n');
    }

    if !review.findings.is_empty() {
        md.push_str("## Findings\n\n");
        for f in &review.findings {
            md.push_str(&format!(
                "### [{severity}] [{dim}] {title}\n\n",
                severity = f.severity,
                dim = f.dimension,
                title = f.title
            ));
            let loc = match f.line {
                Some(line) => format!("**File**: `{}:{}`\n\n", f.file, line),
                None => format!("**File**: `{}`\n\n", f.file),
            };
            md.push_str(&loc);
            md.push_str(&format!("**Problem**: {}\n\n", f.problem));
            md.push_str(&format!("**Suggestion**: {}\n\n", f.suggestion));
        }
    }

    std::fs::write(&path, md).map_err(|e| format!("Write {}: {e}", path.display()))?;
    eprintln!("Saved: {}", path.display());
    Ok(())
}

fn print_summary(session_name: &str, cache_dir: &Path, review: &ReviewOutput) {
    let mut counts = [0u32; 4]; // CRITICAL, HIGH, MEDIUM, LOW
    for f in &review.findings {
        match f.severity {
            Severity::Critical => counts[0] += 1,
            Severity::High => counts[1] += 1,
            Severity::Medium => counts[2] += 1,
            Severity::Low => counts[3] += 1,
        }
    }

    println!();
    println!("## Review Complete (App Server)");
    println!();
    println!("**Session**: {session_name}");
    println!("**Score**: {}/10", review.score);
    println!(
        "**Full report**: {}",
        cache_dir.join(format!("{session_name}.md")).display()
    );
    println!();
    println!("| Severity | Count |");
    println!("|----------|-------|");
    println!("| Critical | {} |", counts[0]);
    println!("| High     | {} |", counts[1]);
    println!("| Medium   | {} |", counts[2]);
    println!("| Low      | {} |", counts[3]);
    println!();
    println!("**Summary**: {}", review.summary);
}

/// Read turn timeout from `CODEX_TURN_TIMEOUT` env var (seconds).
/// Default: 1 hour. Set to 0 for effectively unlimited (~584 billion years).
fn parse_turn_timeout() -> Duration {
    const DEFAULT_SECS: u64 = 3600;
    match std::env::var("CODEX_TURN_TIMEOUT") {
        Ok(val) => match val.parse::<u64>() {
            Ok(0) => Duration::from_secs(u64::MAX / 2),
            Ok(s) => Duration::from_secs(s),
            Err(_) => {
                eprintln!("Warning: invalid CODEX_TURN_TIMEOUT={val:?}, using default {DEFAULT_SECS}s");
                Duration::from_secs(DEFAULT_SECS)
            }
        },
        Err(_) => Duration::from_secs(DEFAULT_SECS),
    }
}

/// Parse the last valid ReviewOutput from concatenated JSON objects.
/// Codex with outputSchema streams multiple JSON objects: reasoning steps
/// followed by the final structured answer. We extract each top-level JSON
/// object and return the last one that deserializes as ReviewOutput.
fn parse_last_review_output(text: &str) -> Result<ReviewOutput, String> {
    if text.is_empty() {
        return Err("Empty agent output".to_string());
    }

    let mut objects = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    let mut in_string = false;
    let mut escape_next = false;

    // Track brace depth while respecting JSON string boundaries.
    for (i, ch) in text.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        objects.push(&text[s..=i]);
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }

    if objects.is_empty() {
        return Err(format!(
            "No JSON objects found in agent output ({} chars). First 200 chars: {}",
            text.len(),
            &text[..text.len().min(200)]
        ));
    }

    // Try from last to first — the final object is the real answer.
    for obj in objects.iter().rev() {
        if let Ok(review) = serde_json::from_str::<ReviewOutput>(obj) {
            return Ok(review);
        }
    }

    Err(format!(
        "No valid ReviewOutput found in {} JSON objects ({} chars). First 200 chars: {}",
        objects.len(),
        text.len(),
        &text[..text.len().min(200)]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_object() {
        let text = r#"{"findings":[],"score":9,"summary":"good","strengths":["clean"]}"#;
        let review = parse_last_review_output(text).unwrap();
        assert_eq!(review.score, 9);
    }

    #[test]
    fn parse_multiple_objects_returns_last_valid() {
        let text = r#"{"findings":[],"score":10,"summary":"thinking...","strengths":[]}{"findings":[{"severity":"HIGH","dimension":"Bugs","title":"bug","file":"f","line":null,"problem":"p","suggestion":"s"}],"score":6,"summary":"found bugs","strengths":["a"]}"#;
        let review = parse_last_review_output(text).unwrap();
        assert_eq!(review.score, 6);
        assert_eq!(review.findings.len(), 1);
    }

    #[test]
    fn parse_handles_braces_in_strings() {
        let text = r#"{"findings":[],"score":8,"summary":"code uses {} syntax","strengths":["handles {braces}"]}"#;
        let review = parse_last_review_output(text).unwrap();
        assert_eq!(review.score, 8);
        assert!(review.summary.contains("{}"));
    }

    #[test]
    fn parse_empty_text_fails() {
        assert!(parse_last_review_output("").is_err());
    }

    #[test]
    fn parse_no_json_fails() {
        assert!(parse_last_review_output("just plain text").is_err());
    }

    #[test]
    fn parse_invalid_json_fails() {
        assert!(parse_last_review_output("{not valid json}").is_err());
    }

    #[test]
    fn parse_handles_escaped_quotes_in_strings() {
        let text = r#"{"findings":[],"score":7,"summary":"says \"hello\"","strengths":[]}"#;
        let review = parse_last_review_output(text).unwrap();
        assert!(review.summary.contains("hello"));
    }
}
