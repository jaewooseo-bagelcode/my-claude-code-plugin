mod glob;
mod grep;
mod read;
mod git_diff;

use serde_json::{json, Value};
use std::path::Path;
use tokio::time::{timeout, Duration};

const TOOL_TIMEOUT_MS: u64 = 120_000;

const DENY_BASENAMES: &[&str] = &[
    ".git", ".svn", ".hg", "node_modules", "venv", "__pycache__",
    ".env", ".DS_Store", "Thumbs.db",
];

const DENY_EXTENSIONS: &[&str] = &[
    "pyc", "pyo", "so", "dll", "dylib", "exe", "bin", "class", "jar", "sqlite", "db", "lock",
];

pub fn is_denied_path(path: &str) -> bool {
    let candidate = Path::new(path);

    for component in candidate.components() {
        if let Some(name) = component.as_os_str().to_str() {
            if DENY_BASENAMES.iter().any(|&b| name == b) {
                return true;
            }
        }
    }

    if let Some(ext) = candidate.extension().and_then(|e| e.to_str()) {
        if DENY_EXTENSIONS.iter().any(|&d| ext == d) {
            return true;
        }
    }

    false
}

fn validate_glob_pattern(pattern: &str) -> Result<(), String> {
    if pattern.contains("..") {
        return Err("Glob pattern cannot contain '..'".to_string());
    }
    if Path::new(pattern).is_absolute() {
        return Err("Glob pattern must be a relative path".to_string());
    }
    Ok(())
}

fn get_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Missing required parameter: {}", key))
}

pub async fn execute_tool(
    name: &str,
    input: Value,
    project_path: &str,
) -> Result<String, String> {
    let execution = execute_tool_inner(name, &input, project_path);

    timeout(Duration::from_millis(TOOL_TIMEOUT_MS), execution)
        .await
        .map_err(|_| format!("Tool '{}' timed out after {}ms", name, TOOL_TIMEOUT_MS))?
}

async fn execute_tool_inner(
    name: &str,
    input: &Value,
    project_path: &str,
) -> Result<String, String> {
    match name {
        "glob_files" => {
            let pattern = get_str(input, "pattern")?;
            validate_glob_pattern(pattern)?;
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(project_path);
            glob::glob_files(pattern, path)
        }
        "grep_content" => {
            let pattern = get_str(input, "pattern")?;
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(project_path);
            let glob_filter = input.get("glob").and_then(|v| v.as_str());
            let output_mode = input.get("output_mode").and_then(|v| v.as_str());
            let case_insensitive = input.get("case_insensitive").and_then(|v| v.as_bool());
            let context = input.get("context").and_then(|v| v.as_u64()).map(|v| v as usize);
            let head_limit = input.get("head_limit").and_then(|v| v.as_u64()).map(|v| v as usize);
            grep::grep_content(pattern, path, glob_filter, output_mode, case_insensitive, context, head_limit)
        }
        "read_file" => {
            let file_path = get_str(input, "file_path")?;
            if is_denied_path(file_path) {
                return Err(format!("Access denied: {}", file_path));
            }
            let offset = input.get("offset").and_then(|v| v.as_u64()).map(|v| v as usize);
            let limit = input.get("limit").and_then(|v| v.as_u64()).map(|v| v as usize);
            read::read_file(file_path, project_path, offset, limit)
        }
        "git_diff" => {
            git_diff::git_diff(project_path).await
        }
        _ => Err(format!("Unknown tool: {}", name)),
    }
}

/// Tool definitions for participants
pub fn build_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "glob_files".to_string(),
            description: "Find files by glob pattern.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern" },
                    "path": { "type": "string", "description": "Search base path" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "grep_content".to_string(),
            description: "Search file contents with regex.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "glob": { "type": "string" },
                    "output_mode": { "type": "string", "enum": ["content", "files_with_matches", "count"] },
                    "case_insensitive": { "type": "boolean" },
                    "context": { "type": "number" },
                    "head_limit": { "type": "number" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read file from project. Supports offset/limit.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "File path (relative to project root)" },
                    "offset": { "type": "number", "description": "Start line (1-indexed)" },
                    "limit": { "type": "number", "description": "Max lines to read" }
                },
                "required": ["file_path"]
            }),
        },
        ToolDefinition {
            name: "git_diff".to_string(),
            description: "Show git diff of uncommitted changes.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ]
}

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}
