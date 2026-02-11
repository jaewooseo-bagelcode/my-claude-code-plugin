use std::process::Command;

const MAX_OUTPUT_BYTES: usize = 200 * 1024;

pub async fn git_diff(project_path: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to execute git diff: {}", e))?;

    if !output.status.success() {
        // Try without HEAD (for repos with no commits)
        let output2 = Command::new("git")
            .args(["diff"])
            .current_dir(project_path)
            .output()
            .map_err(|e| format!("Failed to execute git diff: {}", e))?;

        let mut result = String::from_utf8(output2.stdout)
            .map_err(|e| format!("Invalid UTF-8 in git diff output: {}", e))?;

        if result.len() > MAX_OUTPUT_BYTES {
            result.truncate(MAX_OUTPUT_BYTES);
            result.push_str("\n... (Truncated at 200KB)");
        }

        return Ok(result);
    }

    let mut result = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in git diff output: {}", e))?;

    if result.len() > MAX_OUTPUT_BYTES {
        result.truncate(MAX_OUTPUT_BYTES);
        result.push_str("\n... (Truncated at 200KB)");
    }

    if result.is_empty() {
        Ok("No uncommitted changes.".to_string())
    } else {
        Ok(result)
    }
}
