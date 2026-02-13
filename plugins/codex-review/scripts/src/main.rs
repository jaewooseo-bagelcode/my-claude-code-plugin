mod project_memory;
mod review_loop;

use aiproxy_common::session::JsonlLogger;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_MAX_ITERS: usize = 50;

#[derive(Debug, Serialize, Deserialize)]
struct SessionData {
    last_response_id: String,
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(r#"Usage: codex-review "<session-name>" "<review-prompt>""#);
        std::process::exit(2);
    }

    let session_name = &args[1];
    let review_prompt: String = args[2..].join(" ");

    // Validate session name
    let safe_session_re = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._\-]{0,63}$").unwrap();
    if !safe_session_re.is_match(session_name) {
        eprintln!("Invalid session name: use A-Za-z0-9._- only, max 64 chars, must start with alphanumeric");
        std::process::exit(2);
    }

    // Authentication: codeb credentials > OPENAI_API_KEY env
    let api_key = load_codeb_token().unwrap_or_else(|| {
        std::env::var("OPENAI_API_KEY").unwrap_or_default()
    });
    if api_key.is_empty() {
        eprintln!("No authentication found. Run 'codeb login' or set OPENAI_API_KEY");
        std::process::exit(2);
    }

    let model = get_env("OPENAI_MODEL", "gpt-5.2-codex");
    let reasoning_effort = get_env("REASONING_EFFORT", "high");
    let max_iters = get_env_int("MAX_ITERS", DEFAULT_MAX_ITERS);

    // Detect repo root
    let repo_root = detect_repo_root();

    // Session management
    let sessions_dir = get_env("STATE_DIR", &format!("{}/.codex-sessions", repo_root));
    if let Err(e) = std::fs::create_dir_all(&sessions_dir) {
        eprintln!("Failed to create sessions dir: {}", e);
        std::process::exit(2);
    }

    let session_file = format!("{}/{}.json", sessions_dir, session_name);
    let log_file = format!("{}/{}.log", sessions_dir, session_name);

    // Create logger (nil-safe if creation fails)
    let logger = JsonlLogger::new(&log_file);

    logger.log("session_start", 0, Some(serde_json::json!({
        "model": model,
        "repoRoot": repo_root,
        "session": session_name,
    })));

    // Load project memory (CLAUDE.md + rules) like Claude Code
    let project_memory = project_memory::load_project_memory(&repo_root);
    let system_prompt = project_memory::build_system_prompt(&repo_root, session_name, &project_memory);

    // Load previous response ID for session continuity
    let last_response_id = load_session(&session_file);

    // Execute review with tool loop
    match review_loop::execute_review(
        &api_key,
        &model,
        &reasoning_effort,
        &system_prompt,
        last_response_id.as_deref(),
        &review_prompt,
        &repo_root,
        max_iters,
        &logger,
    )
    .await
    {
        Ok(new_response_id) => {
            logger.log("session_end", 0, Some(serde_json::json!({"status": "ok"})));

            // Save latest response ID for session resumption
            if let Some(ref id) = new_response_id {
                if let Err(e) = save_session(&session_file, id) {
                    eprintln!("Warning: failed to save session: {}", e);
                }
            }
        }
        Err(e) => {
            logger.log("session_end", 0, Some(serde_json::json!({"error": e.to_string()})));
            eprintln!("{}", e);
            logger.close();
            std::process::exit(3);
        }
    }

    logger.close();
}

fn get_env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn get_env_int(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn detect_repo_root() -> String {
    if let Ok(root) = std::env::var("REPO_ROOT") {
        return std::fs::canonicalize(&root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(root);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.clone();

    loop {
        if dir.join(".git").exists() {
            return dir.to_string_lossy().to_string();
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => break,
        }
    }

    cwd.to_string_lossy().to_string()
}

fn load_codeb_token() -> Option<String> {
    let home = dirs::home_dir()?;
    let cred_path = home.join(".codeb/credentials.json");
    let data = std::fs::read_to_string(&cred_path).ok()?;
    let creds: serde_json::Value = serde_json::from_str(&data).ok()?;
    creds.get("token").and_then(|t| t.as_str()).map(|s| s.to_string())
}

fn load_session(session_file: &str) -> Option<String> {
    let data = std::fs::read_to_string(session_file).ok()?;
    let session: SessionData = serde_json::from_str(&data).ok()?;
    if session.last_response_id.is_empty() {
        None
    } else {
        Some(session.last_response_id)
    }
}

fn save_session(session_file: &str, last_response_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let session = SessionData {
        last_response_id: last_response_id.to_string(),
    };
    let data = serde_json::to_string_pretty(&session)?;

    // Atomic write: temp file + rename
    let dir = Path::new(session_file).parent().unwrap_or(Path::new("."));
    let tmp_path = dir.join(format!("session-{}.tmp", std::process::id()));

    std::fs::write(&tmp_path, &data)?;
    std::fs::rename(&tmp_path, session_file)?;

    Ok(())
}
