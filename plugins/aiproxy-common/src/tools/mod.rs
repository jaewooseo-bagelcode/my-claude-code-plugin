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

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.contains("..") {
        return Err("Path cannot contain '..'".to_string());
    }
    if Path::new(path).is_absolute() {
        return Err("Path must be relative".to_string());
    }
    Ok(())
}

fn get_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Missing required parameter: {}", key))
}

/// Tool set determines which naming convention to use for tool definitions.
#[derive(Debug, Clone, Copy)]
pub enum ToolSet {
    /// codex-review style: Glob, Grep, Read, GitDiff (Go naming)
    CodeReview,
    /// braintrust style: glob_files, grep_content, read_file, git_diff
    Full,
}

/// Build tool definitions for the specified tool set.
pub fn build_tool_definitions(tool_set: ToolSet) -> Vec<ToolDefinition> {
    match tool_set {
        ToolSet::CodeReview => build_code_review_tools(),
        ToolSet::Full => build_full_tools(),
    }
}

/// Execute a tool by name (supports dual naming: both CodeReview and Full names).
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
        // Dual naming support
        "Glob" | "glob_files" => {
            let pattern = get_str(input, "pattern")?;
            validate_relative_path(pattern)?;
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(project_path);
            let max_results = input.get("max_results").and_then(|v| v.as_u64()).map(|v| v as usize);
            glob::execute(pattern, path, max_results)
        }
        "Grep" | "grep_content" => {
            // Support both naming conventions for the query/pattern parameter
            let pattern = get_str(input, "query")
                .or_else(|_| get_str(input, "pattern"))?;
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(project_path);
            let glob_filter = input.get("glob").and_then(|v| v.as_str());
            let output_mode = input.get("output_mode").and_then(|v| v.as_str());
            let case_insensitive = input.get("case_insensitive").and_then(|v| v.as_bool());
            let context = input.get("context").and_then(|v| v.as_u64()).map(|v| v as usize);
            let head_limit = input.get("head_limit").and_then(|v| v.as_u64()).map(|v| v as usize);
            let max_results = input.get("max_results").and_then(|v| v.as_u64()).map(|v| v as usize);
            grep::execute(pattern, path, glob_filter, output_mode, case_insensitive, context, head_limit, max_results)
        }
        "Read" | "read_file" => {
            // Support both naming conventions for path/file_path parameter
            let file_path = get_str(input, "path")
                .or_else(|_| get_str(input, "file_path"))?;
            if is_denied_path(file_path) {
                return Err(format!("Access denied: {}", file_path));
            }
            let offset = input.get("offset")
                .or_else(|| input.get("start_line"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let limit = input.get("limit")
                .or_else(|| input.get("max_lines"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let end_line = input.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize);
            read::execute(file_path, project_path, offset, limit, end_line)
        }
        "GitDiff" | "git_diff" => {
            let base = input.get("base").and_then(|v| v.as_str());
            let path = input.get("path")
                .or_else(|| input.get("file_path"))
                .and_then(|v| v.as_str());
            git_diff::execute(project_path, base, path).await
        }
        _ => Err(format!("Unknown tool: {}", name)),
    }
}

// ============================================================================
// CodeReview tool definitions (Go naming for system prompt compatibility)
// ============================================================================

fn build_code_review_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "Glob".to_string(),
            description: "Find repository files matching a glob pattern relative to repo root. Supports ** for recursive directory matching.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern like src/**/*.ts or **/*.go (relative to repo root). ** matches zero or more directories."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Max results (<=200). Default 200."
                    }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "Grep".to_string(),
            description: "Search for text or regex patterns in repository files; optionally restrict to a glob.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (supports regex). Falls back to literal match if regex is invalid."
                    },
                    "glob": {
                        "type": "string",
                        "description": "Optional file glob scope like src/**/*.ts (supports ** recursive matching)"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Max matches (<=200). Default 200."
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "Read".to_string(),
            description: "Read a file snippet by line range (relative path).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative file path from repo root."
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "1-based start line. Default 1."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "1-based end line (inclusive)."
                    },
                    "max_lines": {
                        "type": "integer",
                        "description": "Max lines to return (<=400). Default 400."
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "GitDiff".to_string(),
            description: "Get git diff of changes since a base branch. Useful for reviewing PR changes.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "base": {
                        "type": "string",
                        "description": "Base branch to diff against (default: 'main')."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional file path to restrict diff to a specific file."
                    }
                },
                "required": []
            }),
        },
    ]
}

// ============================================================================
// Full tool definitions (braintrust naming)
// ============================================================================

fn build_full_tools() -> Vec<ToolDefinition> {
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
