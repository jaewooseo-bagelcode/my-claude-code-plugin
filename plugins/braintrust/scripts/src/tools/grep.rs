use std::process::Command;

const MAX_OUTPUT_BYTES: usize = 200 * 1024;

pub fn grep_content(
    pattern: &str,
    path: &str,
    glob_filter: Option<&str>,
    output_mode: Option<&str>,
    case_insensitive: Option<bool>,
    context: Option<usize>,
    head_limit: Option<usize>,
) -> Result<String, String> {
    let mode = output_mode.unwrap_or("files_with_matches");

    let mut cmd = Command::new("grep");
    cmd.arg("-r");

    if case_insensitive.unwrap_or(false) {
        cmd.arg("-i");
    }

    match mode {
        "files_with_matches" => { cmd.arg("-l"); }
        "count" => { cmd.arg("-c"); }
        _ => { cmd.arg("-n"); }
    }

    if let Some(ctx) = context {
        cmd.arg(format!("-C{}", ctx));
    }

    // Glob filter
    if let Some(glob_pattern) = glob_filter {
        cmd.arg("--include").arg(glob_pattern);
    }

    // Exclude common binary/build dirs
    cmd.arg("--exclude-dir=.git");
    cmd.arg("--exclude-dir=node_modules");
    cmd.arg("--exclude-dir=target");
    cmd.arg("--exclude-dir=.venv");
    cmd.arg("--exclude-dir=__pycache__");

    cmd.arg("--");
    cmd.arg(pattern);
    cmd.arg(path);

    let output = cmd.output()
        .map_err(|e| format!("Failed to execute grep: {}", e))?;

    let mut result = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in grep output: {}", e))?;

    if let Some(limit) = head_limit {
        let lines: Vec<&str> = result.lines().take(limit).collect();
        result = lines.join("\n");
    }

    if result.len() > MAX_OUTPUT_BYTES {
        result.truncate(MAX_OUTPUT_BYTES);
        result.push_str("\n... (Truncated at 200KB)");
    }

    Ok(result)
}
