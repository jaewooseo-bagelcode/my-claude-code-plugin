/// Write progress messages to stderr for real-time user feedback
pub fn log_stderr(msg: &str) {
    eprintln!("{}", msg);
}

pub fn emit_meeting_started(_meeting_id: &str, agenda: &str) {
    let preview = preview_str(agenda, 80);
    eprintln!("[braintrust] \u{1f3db}\u{fe0f} Meeting started: {}", preview);
}

pub fn emit_iteration_started(iteration: u32, max_iterations: u32, question: &str) {
    let preview = preview_str(question, 80);
    eprintln!("[braintrust] \u{1f4cb} Round {}/{}: {}", iteration + 1, max_iterations, preview);
}

pub fn emit_participant_started(provider: &str, _model: &str) {
    eprintln!("[braintrust]   \u{251c}\u{2500} {}: analyzing...", provider);
}

pub fn emit_participant_step(provider: &str, step: usize, tool_name: Option<&str>) {
    if let Some(tool) = tool_name {
        eprintln!("[braintrust]   \u{2502}  {}: step {} ({})", provider, step + 1, tool);
    }
}

pub fn emit_participant_completed(provider: &str, success: bool, elapsed_ms: u64) {
    if success {
        eprintln!("[braintrust]   \u{2514}\u{2500} {}: completed \u{2713} ({:.1}s)", provider, elapsed_ms as f64 / 1000.0);
    } else {
        eprintln!("[braintrust]   \u{2514}\u{2500} {}: failed \u{2717}", provider);
    }
}

pub fn emit_chair_analyzing(iteration: u32) {
    eprintln!("[braintrust] \u{1fa91} Chair analyzing round {}...", iteration + 1);
}

pub fn emit_chair_follow_up(_iteration: u32, question: &str) {
    let preview = preview_str(question, 80);
    eprintln!("[braintrust] \u{1fa91} Chair follow-up: {}", preview);
}

pub fn emit_chair_synthesizing() {
    eprintln!("[braintrust] \u{1f4dd} Chair synthesizing final consensus...");
}

pub fn emit_chair_completed(elapsed_ms: u64) {
    eprintln!("[braintrust] \u{1f4dd} Chair completed ({:.1}s)", elapsed_ms as f64 / 1000.0);
}

pub fn emit_meeting_completed(elapsed_ms: u64, total_iterations: u32) {
    eprintln!(
        "[braintrust] \u{2705} Meeting completed ({:.0}s, {} round{})",
        elapsed_ms as f64 / 1000.0,
        total_iterations,
        if total_iterations == 1 { "" } else { "s" }
    );
}

fn preview_str(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => format!("{}...", &s[..idx]),
        None => s.to_string(),
    }
}
