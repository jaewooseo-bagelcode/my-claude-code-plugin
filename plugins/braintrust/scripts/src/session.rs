use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub provider: String,
    pub content: String,
    pub model: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantStep {
    pub step: usize,
    pub step_type: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_output: Option<String>,
    pub tool_error: Option<String>,
    pub content: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantSession {
    pub provider: String,
    pub model: String,
    pub steps: Vec<ParticipantStep>,
    pub final_content: String,
    pub success: bool,
    pub error: Option<String>,
}

impl ParticipantSession {
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.to_string(),
            model: model.to_string(),
            steps: Vec::new(),
            final_content: String::new(),
            success: false,
            error: None,
        }
    }

    pub fn add_tool_call(
        &mut self,
        tool_name: &str,
        tool_input: Value,
        tool_output: Result<String, String>,
    ) {
        let timestamp = now_millis();
        let (output, error) = match tool_output {
            Ok(out) => (Some(out), None),
            Err(err) => (None, Some(err)),
        };

        self.steps.push(ParticipantStep {
            step: self.steps.len() + 1,
            step_type: "tool_call".to_string(),
            tool_name: Some(tool_name.to_string()),
            tool_input: Some(tool_input),
            tool_output: output,
            tool_error: error,
            content: None,
            timestamp,
        });
    }

    pub fn finalize(&mut self, final_content: String, success: bool, error: Option<String>) {
        self.final_content = final_content;
        self.success = success;
        self.error = error;
    }

    pub fn to_ai_response(&self) -> AiResponse {
        AiResponse {
            provider: self.provider.clone(),
            content: self.final_content.clone(),
            model: self.model.clone(),
            success: self.success,
            error: self.error.clone(),
        }
    }
}

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

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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

pub fn save_meeting_meta(project_path: &str, meta: &BraintrustMeetingMeta) -> Result<(), String> {
    let dir = format!("{}/.braintrust-sessions/{}", project_path, meta.meeting_id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path = format!("{}/metadata.json", dir);
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn save_iteration(
    project_path: &str,
    meeting_id: &str,
    iteration: &BraintrustIteration,
) -> Result<(), String> {
    let dir = format!(
        "{}/.braintrust-sessions/{}/iteration_{}",
        project_path, meeting_id, iteration.iteration
    );
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    for session in &iteration.participant_sessions {
        let path = format!("{}/{}.json", dir, session.provider);
        let json = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())?;
    }

    let meta = json!({
        "iteration": iteration.iteration,
        "question": iteration.question,
        "timestamp": iteration.timestamp,
        "participant_count": iteration.participant_sessions.len(),
    });
    let meta_path = format!("{}/metadata.json", dir);
    let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    fs::write(&meta_path, json).map_err(|e| e.to_string())?;

    Ok(())
}

pub fn save_chair_summary(
    project_path: &str,
    meeting_id: &str,
    summary: &AiResponse,
) -> Result<(), String> {
    let dir = format!("{}/.braintrust-sessions/{}", project_path, meeting_id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path = format!("{}/chair.json", dir);
    let json = serde_json::to_string_pretty(summary).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn update_meeting_status(
    project_path: &str,
    meeting_id: &str,
    status: &str,
    elapsed_ms: u64,
) -> Result<(), String> {
    let path = format!(
        "{}/.braintrust-sessions/{}/metadata.json",
        project_path, meeting_id
    );

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut meta: BraintrustMeetingMeta = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    meta.status = status.to_string();
    meta.completed_at = Some(now_millis());
    meta.elapsed_ms = Some(elapsed_ms);

    let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// List all meeting sessions in the project
pub fn list_sessions(project_path: &str) -> Result<Vec<BraintrustMeetingMeta>, String> {
    let sessions_dir = format!("{}/.braintrust-sessions", project_path);
    let dir = match fs::read_dir(&sessions_dir) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()), // No sessions directory yet
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
pub fn load_iterations(project_path: &str, meeting_id: &str) -> Result<Vec<BraintrustIteration>, String> {
    let session_dir = format!("{}/.braintrust-sessions/{}", project_path, meeting_id);

    // Find all iteration_N directories
    let mut iteration_dirs: Vec<(u32, std::path::PathBuf)> = Vec::new();
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
        // Read iteration metadata
        let meta_path = iter_path.join("metadata.json");
        let meta_content = fs::read_to_string(&meta_path)
            .map_err(|e| format!("Cannot read iteration metadata: {}", e))?;
        let meta: serde_json::Value = serde_json::from_str(&meta_content)
            .map_err(|e| format!("Cannot parse iteration metadata: {}", e))?;
        let question = meta.get("question")
            .and_then(|q| q.as_str())
            .unwrap_or("")
            .to_string();

        // Read participant sessions
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
pub fn load_meeting_meta(project_path: &str, meeting_id: &str) -> Result<BraintrustMeetingMeta, String> {
    let path = format!("{}/.braintrust-sessions/{}/metadata.json", project_path, meeting_id);
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
    project_path: &str,
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
    let _ = append_debug_log(project_path, meeting_id, &entry);
}

fn append_debug_log(
    project_path: &str,
    meeting_id: &str,
    entry: &DebugLogEntry,
) -> Result<(), String> {
    use std::io::Write;
    use std::fs::OpenOptions;

    let dir = format!("{}/.braintrust-sessions/{}", project_path, meeting_id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path = format!("{}/debug.jsonl", dir);
    let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;

    writeln!(file, "{}", line).map_err(|e| e.to_string())
}
