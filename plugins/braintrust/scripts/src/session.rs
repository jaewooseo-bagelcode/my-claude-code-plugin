// Re-export shared types from aiproxy-common
pub use aiproxy_common::session::{AiResponse, ParticipantSession};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

use aiproxy_common::session::now_millis;

// ============================================================================
// Braintrust-specific meeting types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraintrustIteration {
    pub iteration: u32,
    pub question: String,
    pub participant_sessions: Vec<ParticipantSession>,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraintrustResult {
    pub meeting_id: String,
    pub summary: String,
    pub raw_responses: Vec<AiResponse>,
    pub iterations: Vec<BraintrustIteration>,
    pub total_iterations: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraintrustMeetingMeta {
    pub meeting_id: String,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub agenda: String,
    pub context: Option<String>,
    pub status: String,
}

/// Returns ~/.braintrust/sessions/ base directory.
fn sessions_base_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    Ok(home.join(".braintrust").join("sessions"))
}

/// Returns the session directory for a specific meeting: ~/.braintrust/sessions/{meeting_id}/
fn meeting_dir(meeting_id: &str) -> Result<PathBuf, String> {
    Ok(sessions_base_dir()?.join(meeting_id))
}

pub fn create_meeting_meta(meeting_id: &str, agenda: &str, context: Option<&str>) -> BraintrustMeetingMeta {
    BraintrustMeetingMeta {
        meeting_id: meeting_id.to_string(),
        created_at: now_millis(),
        completed_at: None,
        elapsed_ms: None,
        agenda: agenda.to_string(),
        context: context.map(|s| s.to_string()),
        status: "running".to_string(),
    }
}

pub fn save_meeting_meta(meta: &BraintrustMeetingMeta) -> Result<(), String> {
    let dir = meeting_dir(&meta.meeting_id)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path = dir.join("metadata.json");
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn save_iteration(
    meeting_id: &str,
    iteration: &BraintrustIteration,
) -> Result<(), String> {
    let dir = meeting_dir(meeting_id)?.join(format!("iteration_{}", iteration.iteration));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    for session in &iteration.participant_sessions {
        let path = dir.join(format!("{}.json", session.provider));
        let json = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())?;
    }

    let meta = json!({
        "iteration": iteration.iteration,
        "question": iteration.question,
        "timestamp": iteration.timestamp,
        "participant_count": iteration.participant_sessions.len(),
    });
    let meta_path = dir.join("metadata.json");
    let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    fs::write(&meta_path, json).map_err(|e| e.to_string())?;

    Ok(())
}

pub fn save_chair_summary(
    meeting_id: &str,
    summary: &AiResponse,
) -> Result<(), String> {
    let dir = meeting_dir(meeting_id)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path = dir.join("chair.json");
    let json = serde_json::to_string_pretty(summary).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn update_meeting_status(
    meeting_id: &str,
    status: &str,
    elapsed_ms: u64,
) -> Result<(), String> {
    let path = meeting_dir(meeting_id)?.join("metadata.json");

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut meta: BraintrustMeetingMeta = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    meta.status = status.to_string();
    meta.completed_at = Some(now_millis());
    meta.elapsed_ms = Some(elapsed_ms);

    let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// List all meeting sessions
pub fn list_sessions() -> Result<Vec<BraintrustMeetingMeta>, String> {
    let sessions_dir = sessions_base_dir()?;
    let dir = match fs::read_dir(&sessions_dir) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };

    let mut sessions = Vec::new();
    for entry in dir.flatten() {
        if entry.path().is_dir() {
            let meta_path = entry.path().join("metadata.json");
            if let Ok(content) = fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<BraintrustMeetingMeta>(&content) {
                    sessions.push(meta);
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(sessions)
}

/// Load previous iterations from a meeting session
pub fn load_iterations(meeting_id: &str) -> Result<Vec<BraintrustIteration>, String> {
    let session_dir = meeting_dir(meeting_id)?;

    let mut iteration_dirs: Vec<(u32, PathBuf)> = Vec::new();
    let dir = fs::read_dir(&session_dir).map_err(|e| format!("Cannot read session dir: {}", e))?;
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(num_str) = name.strip_prefix("iteration_") {
            if let Ok(num) = num_str.parse::<u32>() {
                iteration_dirs.push((num, entry.path()));
            }
        }
    }
    iteration_dirs.sort_by_key(|(num, _)| *num);

    let mut iterations = Vec::new();
    for (iter_num, iter_path) in iteration_dirs {
        let meta_path = iter_path.join("metadata.json");
        let meta_content = fs::read_to_string(&meta_path)
            .map_err(|e| format!("Cannot read iteration metadata: {}", e))?;
        let meta: serde_json::Value = serde_json::from_str(&meta_content)
            .map_err(|e| format!("Cannot parse iteration metadata: {}", e))?;
        let question = meta.get("question")
            .and_then(|q| q.as_str())
            .unwrap_or("")
            .to_string();

        let mut participant_sessions = Vec::new();
        for provider in &["openai", "gemini", "claude"] {
            let ps_path = iter_path.join(format!("{}.json", provider));
            if let Ok(content) = fs::read_to_string(&ps_path) {
                if let Ok(ps) = serde_json::from_str::<ParticipantSession>(&content) {
                    participant_sessions.push(ps);
                }
            }
        }

        iterations.push(BraintrustIteration {
            iteration: iter_num,
            question,
            participant_sessions,
            timestamp: meta.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0),
        });
    }

    Ok(iterations)
}

/// Load meeting metadata
pub fn load_meeting_meta(meeting_id: &str) -> Result<BraintrustMeetingMeta, String> {
    let path = meeting_dir(meeting_id)?.join("metadata.json");
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read meeting metadata: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Cannot parse meeting metadata: {}", e))
}

/// Debug log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugLogEntry {
    pub timestamp: u64,
    pub level: String,
    pub provider: String,
    pub event: String,
    pub message: String,
    pub data: Option<Value>,
}

pub fn log_debug(
    meeting_id: &str,
    level: &str,
    provider: &str,
    event: &str,
    message: &str,
    data: Option<Value>,
) {
    let entry = DebugLogEntry {
        timestamp: now_millis(),
        level: level.to_string(),
        provider: provider.to_string(),
        event: event.to_string(),
        message: message.to_string(),
        data,
    };
    let _ = append_debug_log(meeting_id, &entry);
}

fn append_debug_log(
    meeting_id: &str,
    entry: &DebugLogEntry,
) -> Result<(), String> {
    use std::io::Write;
    use std::fs::OpenOptions;

    let dir = meeting_dir(meeting_id)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path = dir.join("debug.jsonl");
    let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;

    writeln!(file, "{}", line).map_err(|e| e.to_string())
}
