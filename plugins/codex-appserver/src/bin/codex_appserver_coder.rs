//! CLI binary: run code implementation via Codex App Server.
//!
//! Usage:
//!   codex-appserver-coder --project-path <path> --model <model> <session-name> <prompt-file>

use std::path::{Path, PathBuf};
use std::time::Duration;

use codex_appserver::appserver::protocol::{coder_output_schema, CoderOutput};
use codex_appserver::appserver::CodexAppServerClient;
use serde_json::{json, Value};

fn validate_session_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Session name must be 1-64 characters".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("Session name must contain only A-Za-z0-9._- characters".to_string());
    }
    if !name.chars().next().unwrap().is_ascii_alphanumeric() {
        return Err("Session name must start with an alphanumeric character".to_string());
    }
    Ok(())
}

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
                    "Usage: codex-appserver-coder --project-path <path> [--model <model>] <session-name> <prompt-file>"
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
            "Usage: codex-appserver-coder --project-path <path> [--model <model>] <session-name> <prompt-file>\n\
             Got {} positional args: {:?}",
            positional.len(),
            positional
        ));
    }

    let session_name = positional[0].clone();
    validate_session_name(&session_name)?;
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
                    "name": "codex-appserver-coder",
                    "version": "0.1.0"
                },
                "capabilities": {}
            }),
        )
        .await?;

    client.notify("initialized", Value::Null).await?;

    // 2. Create thread — workspace-write sandbox
    let project_path_str = project_path.to_string_lossy().to_string();
    eprintln!("Creating thread (model: {model}, sandbox: workspace-write)...");
    let thread_result = client
        .request(
            "thread/start",
            json!({
                "model": model,
                "cwd": &project_path_str,
                "sandbox": "workspace-write",
                "approvalPolicy": "never"
            }),
        )
        .await?;

    let thread_id = thread_result
        .get("thread")
        .and_then(|t| t.get("id"))
        .or_else(|| thread_result.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "thread/start did not return a thread id".to_string())?
        .to_string();

    eprintln!("Thread created: {}", &thread_id[..thread_id.len().min(16)]);

    // 3. Start turn with prompt + outputSchema
    eprintln!("Starting implementation turn...");
    client.clear_text().await;
    let turn_result = client
        .request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": [{ "type": "text", "text": prompt }],
                "outputSchema": coder_output_schema()
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
    eprintln!("Waiting for implementation completion (timeout: {}s)...", turn_timeout.as_secs());
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

    let coder_output = parse_last_coder_output(&agent_text)?;

    // 7. Capture git diff --stat
    let diff_stat = capture_git_diff_stat(&project_path);

    // 8. Save to cache
    let cache_dir = project_path.join(".codex-coder-cache/implementations");
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create cache dir: {e}"))?;

    save_coder_json(&cache_dir, &session_name, &coder_output)?;
    save_coder_markdown(&cache_dir, &session_name, &coder_output, &diff_stat)?;
    if let Some(ref diff) = diff_stat {
        save_coder_diff(&cache_dir, &session_name, diff)?;
    }

    // 9. Print summary
    print_summary(&session_name, &cache_dir, &coder_output, &diff_stat);

    // 10. Shutdown
    eprintln!("Shutting down app server...");
    let shutdown_status = client.shutdown().await;
    if !shutdown_status.is_clean() {
        eprintln!("Warning: shutdown was not fully clean:");
        if let Err(e) = &shutdown_status.shutdown_request {
            eprintln!("  shutdown request: {e}");
        }
        if let Err(e) = &shutdown_status.exit_notify {
            eprintln!("  exit notification: {e}");
        }
        if !shutdown_status.process_exited {
            eprintln!("  process did not exit within timeout (kill_on_drop will handle)");
        }
    }

    Ok(())
}

fn capture_git_diff_stat(project_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(project_path)
        .output()
        .ok()?;

    if output.status.success() {
        let stat = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stat.is_empty() {
            None
        } else {
            Some(stat)
        }
    } else {
        None
    }
}

fn save_coder_json(
    cache_dir: &Path,
    session_name: &str,
    output: &CoderOutput,
) -> Result<(), String> {
    let path = cache_dir.join(format!("{session_name}.json"));
    let json = serde_json::to_string_pretty(output).map_err(|e| format!("JSON serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Write {}: {e}", path.display()))?;
    eprintln!("Saved: {}", path.display());
    Ok(())
}

fn save_coder_markdown(
    cache_dir: &Path,
    session_name: &str,
    output: &CoderOutput,
    diff_stat: &Option<String>,
) -> Result<(), String> {
    let path = cache_dir.join(format!("{session_name}.md"));
    let mut md = String::new();

    md.push_str(&format!("# Implementation: {session_name}\n\n"));
    md.push_str(&format!("**Status**: {}\n\n", output.status));
    md.push_str(&format!("## Summary\n\n{}\n\n", output.summary));

    if !output.files_changed.is_empty() {
        md.push_str("## Files Changed\n\n");
        md.push_str("| Action | File | Description |\n");
        md.push_str("|--------|------|-------------|\n");
        for fc in &output.files_changed {
            md.push_str(&format!("| {} | `{}` | {} |\n", fc.action, fc.path, fc.description));
        }
        md.push('\n');
    }

    if let Some(stat) = diff_stat {
        md.push_str("## Git Diff Stats\n\n```\n");
        md.push_str(stat);
        md.push_str("\n```\n\n");
    }

    if !output.notes.is_empty() {
        md.push_str("## Notes\n\n");
        for note in &output.notes {
            md.push_str(&format!("- {note}\n"));
        }
        md.push('\n');
    }

    std::fs::write(&path, md).map_err(|e| format!("Write {}: {e}", path.display()))?;
    eprintln!("Saved: {}", path.display());
    Ok(())
}

fn save_coder_diff(
    cache_dir: &Path,
    session_name: &str,
    diff_stat: &str,
) -> Result<(), String> {
    let path = cache_dir.join(format!("{session_name}.diff"));
    std::fs::write(&path, diff_stat).map_err(|e| format!("Write {}: {e}", path.display()))?;
    eprintln!("Saved: {}", path.display());
    Ok(())
}

fn print_summary(
    session_name: &str,
    cache_dir: &Path,
    output: &CoderOutput,
    diff_stat: &Option<String>,
) {
    println!();
    println!("## Implementation Complete (App Server)");
    println!();
    println!("**Session**: {session_name}");
    println!("**Status**: {}", output.status);
    println!(
        "**Full report**: {}",
        cache_dir.join(format!("{session_name}.md")).display()
    );
    println!();

    if !output.files_changed.is_empty() {
        println!("| Action   | File |");
        println!("|----------|------|");
        for fc in &output.files_changed {
            println!("| {:<8} | {} |", format!("{}", fc.action), fc.path);
        }
        println!();
    }

    if let Some(stat) = diff_stat {
        // Print last line of git diff --stat (summary line)
        if let Some(last_line) = stat.lines().last() {
            println!("**Git diff stats**: {last_line}");
        }
    }

    println!("**Summary**: {}", output.summary);

    if !output.notes.is_empty() {
        println!();
        println!("**Notes**:");
        for note in &output.notes {
            println!("- {note}");
        }
    }
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

/// Parse the last valid CoderOutput from concatenated JSON objects.
/// Codex with outputSchema streams multiple JSON objects: reasoning steps
/// followed by the final structured answer. We extract each top-level JSON
/// object and return the last one that deserializes as CoderOutput.
fn parse_last_coder_output(text: &str) -> Result<CoderOutput, String> {
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
        if let Ok(output) = serde_json::from_str::<CoderOutput>(obj) {
            return Ok(output);
        }
    }

    Err(format!(
        "No valid CoderOutput found in {} JSON objects ({} chars). First 200 chars: {}",
        objects.len(),
        text.len(),
        &text[..text.len().min(200)]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_appserver::appserver::protocol::CoderStatus;

    #[test]
    fn parse_single_object() {
        let text = r#"{"status":"completed","summary":"done","files_changed":[],"notes":[]}"#;
        let output = parse_last_coder_output(text).unwrap();
        assert_eq!(output.status, CoderStatus::Completed);
    }

    #[test]
    fn parse_multiple_objects_returns_last_valid() {
        let text = r#"{"status":"completed","summary":"thinking...","files_changed":[],"notes":[]}{"status":"completed","summary":"implemented rate limiting","files_changed":[{"path":"src/middleware/rate-limit.ts","action":"created","description":"Rate limiting middleware"}],"notes":[]}"#;
        let output = parse_last_coder_output(text).unwrap();
        assert_eq!(output.summary, "implemented rate limiting");
        assert_eq!(output.files_changed.len(), 1);
    }

    #[test]
    fn parse_handles_braces_in_strings() {
        let text = r#"{"status":"completed","summary":"code uses {} syntax","files_changed":[],"notes":["handles {braces}"]}"#;
        let output = parse_last_coder_output(text).unwrap();
        assert!(output.summary.contains("{}"));
    }

    #[test]
    fn parse_empty_text_fails() {
        assert!(parse_last_coder_output("").is_err());
    }

    #[test]
    fn parse_no_json_fails() {
        assert!(parse_last_coder_output("just plain text").is_err());
    }

    #[test]
    fn parse_invalid_json_fails() {
        assert!(parse_last_coder_output("{not valid json}").is_err());
    }

    #[test]
    fn parse_handles_escaped_quotes_in_strings() {
        let text = r#"{"status":"partial","summary":"says \"hello\"","files_changed":[],"notes":[]}"#;
        let output = parse_last_coder_output(text).unwrap();
        assert!(output.summary.contains("hello"));
        assert_eq!(output.status, CoderStatus::Partial);
    }

    // --- Session name validation ---

    #[test]
    fn valid_session_names() {
        assert!(validate_session_name("rate-limit-a3f7b2c1").is_ok());
        assert!(validate_session_name("auth_middleware.test").is_ok());
        assert!(validate_session_name("a").is_ok());
    }

    #[test]
    fn rejects_empty_session_name() {
        assert!(validate_session_name("").is_err());
    }

    #[test]
    fn rejects_path_traversal_session_name() {
        assert!(validate_session_name("../../etc/passwd").is_err());
        assert!(validate_session_name("../malicious").is_err());
    }

    #[test]
    fn rejects_slash_in_session_name() {
        assert!(validate_session_name("foo/bar").is_err());
    }

    #[test]
    fn rejects_session_name_starting_with_dot() {
        assert!(validate_session_name(".hidden").is_err());
        assert!(validate_session_name("-dash").is_err());
    }

    #[test]
    fn rejects_too_long_session_name() {
        let long = "a".repeat(65);
        assert!(validate_session_name(&long).is_err());
        let exact = "a".repeat(64);
        assert!(validate_session_name(&exact).is_ok());
    }
}
