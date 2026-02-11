mod config;
mod events;
mod orchestrator;
mod providers;
mod session;
mod tools;

use clap::Parser;

#[derive(Parser)]
#[command(name = "braintrust")]
#[command(about = "Multi-AI consensus meeting system")]
struct Cli {
    /// Meeting agenda / question to discuss
    #[arg(long, required_unless_present_any = ["resume", "list_sessions"])]
    agenda: Option<String>,

    /// Additional context for the meeting
    #[arg(long)]
    context: Option<String>,

    /// Project root path
    #[arg(long)]
    project_path: String,

    /// Maximum number of discussion rounds
    #[arg(long, default_value = "3")]
    max_iterations: u32,

    /// Chair model (default: claude-opus-4-6)
    #[arg(long, env = "CHAIR_MODEL", default_value = "claude-opus-4-6")]
    chair_model: String,

    /// Resume a previous meeting by meeting_id
    #[arg(long)]
    resume: Option<String>,

    /// List all sessions for the project
    #[arg(long)]
    list_sessions: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // --list-sessions mode
    if cli.list_sessions {
        match session::list_sessions(&cli.project_path) {
            Ok(sessions) => {
                let json = serde_json::to_string_pretty(&sessions).unwrap_or_else(|e| {
                    format!("{{\"error\": \"Failed to serialize: {}\"}}", e)
                });
                println!("{}", json);
            }
            Err(e) => {
                eprintln!("[braintrust] Error listing sessions: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // --resume mode
    if let Some(meeting_id) = cli.resume {
        let request = orchestrator::ResumeRequest {
            meeting_id,
            project_path: cli.project_path,
            max_iterations: cli.max_iterations,
            chair_model: cli.chair_model,
        };

        match orchestrator::resume_braintrust(request).await {
            Ok(result) => {
                let json = serde_json::to_string_pretty(&result).unwrap_or_else(|e| {
                    format!("{{\"error\": \"Failed to serialize result: {}\"}}", e)
                });
                println!("{}", json);
            }
            Err(e) => {
                eprintln!("[braintrust] Fatal error: {}", e);
                let error_json = serde_json::json!({
                    "error": e.to_string(),
                    "meeting_id": null,
                    "summary": "",
                    "raw_responses": [],
                    "iterations": [],
                    "total_iterations": 0,
                    "elapsed_ms": 0
                });
                println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
                std::process::exit(1);
            }
        }
        return;
    }

    // Normal new meeting mode
    let agenda = cli.agenda.expect("agenda is required for new meetings");

    let request = orchestrator::BraintrustRequest {
        agenda,
        context: cli.context,
        project_path: cli.project_path,
        max_iterations: cli.max_iterations,
        chair_model: cli.chair_model,
    };

    match orchestrator::run_braintrust(request).await {
        Ok(result) => {
            let json = serde_json::to_string_pretty(&result).unwrap_or_else(|e| {
                format!("{{\"error\": \"Failed to serialize result: {}\"}}", e)
            });
            println!("{}", json);
        }
        Err(e) => {
            eprintln!("[braintrust] Fatal error: {}", e);
            let error_json = serde_json::json!({
                "error": e.to_string(),
                "meeting_id": null,
                "summary": "",
                "raw_responses": [],
                "iterations": [],
                "total_iterations": 0,
                "elapsed_ms": 0
            });
            println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
            std::process::exit(1);
        }
    }
}
