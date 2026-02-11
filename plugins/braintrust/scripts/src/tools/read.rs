use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

const MAX_READ_LINES: usize = 2000;
const MAX_OUTPUT_BYTES: usize = 200 * 1024;

pub fn read_file(
    path: &str,
    project_path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, String> {
    // Resolve path relative to project root
    let resolved = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        PathBuf::from(project_path).join(path)
    };

    // Security: ensure resolved path is within project
    let canonical = resolved.canonicalize()
        .map_err(|e| format!("Failed to resolve {}: {}", path, e))?;
    let project_canonical = PathBuf::from(project_path).canonicalize()
        .map_err(|e| format!("Failed to resolve project path: {}", e))?;

    if !canonical.starts_with(&project_canonical) {
        return Err(format!("Access denied: {} is outside project root", path));
    }

    let file = fs::File::open(&canonical)
        .map_err(|e| format!("Failed to open {}: {}", path, e))?;

    let reader = BufReader::new(file);
    let start = offset.unwrap_or(1).saturating_sub(1);
    let line_limit = limit.unwrap_or(MAX_READ_LINES).min(MAX_READ_LINES);
    let end = start + line_limit;

    let mut lines = Vec::new();
    let mut total_bytes = 0usize;
    let mut truncated_by_bytes = false;

    for (idx, line_result) in reader.lines().enumerate() {
        if idx < start {
            continue;
        }
        if idx >= end {
            break;
        }

        let line = line_result.map_err(|e| format!("Failed to read line: {}", e))?;

        total_bytes += line.len() + 1;
        if total_bytes > MAX_OUTPUT_BYTES {
            truncated_by_bytes = true;
            break;
        }

        lines.push(format!("{:6}|{}", idx + 1, line));
    }

    if lines.is_empty() {
        return Ok("File is empty or no lines in range".to_string());
    }

    let mut result = lines.join("\n");

    if truncated_by_bytes {
        result.push_str("\n... (Truncated at 200KB byte limit)");
    } else if lines.len() >= MAX_READ_LINES {
        result.push_str(&format!("\n... (Truncated at {} lines)", MAX_READ_LINES));
    }

    Ok(result)
}
