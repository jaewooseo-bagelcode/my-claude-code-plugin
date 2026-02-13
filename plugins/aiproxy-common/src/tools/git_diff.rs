use regex::Regex;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tokio::time::{timeout, Duration};

const MAX_DIFF_LINES: usize = 10_000;
const DIFF_TIMEOUT_SECS: u64 = 60;

/// Execute git diff with streaming, branch comparison, and fallback (ported from Go).
pub async fn execute(
    project_path: &str,
    base: Option<&str>,
    file_path: Option<&str>,
) -> Result<String, String> {
    let base_branch = base.unwrap_or("main");

    // Validate branch name
    if !base_branch.is_empty() {
        let safe_branch_re = Regex::new(r"^[A-Za-z0-9._/\-]+$").unwrap();
        if !safe_branch_re.is_match(base_branch) {
            return Err("GitDiff: invalid base branch name".to_string());
        }
    }

    // Try base...HEAD first (three-dot diff for branch comparison)
    let mut args = vec!["diff".to_string(), format!("{}...HEAD", base_branch)];
    if let Some(fp) = file_path {
        args.push("--".to_string());
        args.push(fp.to_string());
    }

    match run_git_diff_streaming(project_path, &args).await {
        Ok((content, truncated)) => {
            return Ok(format_diff_output(content, truncated, project_path, base_branch, file_path));
        }
        Err(_) => {
            // Fallback: try simple diff against base (two-dot)
            let mut args2 = vec!["diff".to_string(), base_branch.to_string()];
            if let Some(fp) = file_path {
                args2.push("--".to_string());
                args2.push(fp.to_string());
            }

            match run_git_diff_streaming(project_path, &args2).await {
                Ok((content, truncated)) => {
                    return Ok(format_diff_output(content, truncated, project_path, base_branch, file_path));
                }
                Err(_) => {
                    // Final fallback: uncommitted changes (git diff HEAD)
                    let args3 = vec!["diff".to_string(), "HEAD".to_string()];
                    match run_git_diff_streaming(project_path, &args3).await {
                        Ok((content, truncated)) => {
                            return Ok(format_diff_output(content, truncated, project_path, "HEAD", file_path));
                        }
                        Err(_) => {
                            // Last resort: plain git diff
                            let args4 = vec!["diff".to_string()];
                            match run_git_diff_streaming(project_path, &args4).await {
                                Ok((content, truncated)) => {
                                    if content.is_empty() {
                                        return Ok("No uncommitted changes.".to_string());
                                    }
                                    return Ok(format_diff_output(content, truncated, project_path, "", file_path));
                                }
                                Err(e2) => {
                                    return Err(format!("GitDiff: {}", e2));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn format_diff_output(content: String, truncated: bool, _project_path: &str, _base: &str, _file_path: Option<&str>) -> String {
    if content.is_empty() {
        return "No changes found.".to_string();
    }
    if truncated {
        format!("{}\n\n... truncated (showing {} of total lines)", content, MAX_DIFF_LINES)
    } else {
        content
    }
}

/// Run git diff with streaming output, line limit, and timeout.
async fn run_git_diff_streaming(project_path: &str, args: &[String]) -> Result<(String, bool), String> {
    let result = timeout(
        Duration::from_secs(DIFF_TIMEOUT_SECS),
        tokio::task::spawn_blocking({
            let project_path = project_path.to_string();
            let args = args.to_vec();
            move || run_git_diff_sync(&project_path, &args)
        }),
    )
    .await
    .map_err(|_| format!("GitDiff timed out after {}s", DIFF_TIMEOUT_SECS))?
    .map_err(|e| format!("GitDiff task error: {}", e))?;

    result
}

fn run_git_diff_sync(project_path: &str, args: &[String]) -> Result<(String, bool), String> {
    let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let mut child = Command::new("git")
        .args(&str_args)
        .current_dir(project_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start git diff: {}", e))?;

    let stdout = child.stdout.take()
        .ok_or("Failed to capture git diff stdout")?;

    let reader = BufReader::new(stdout);
    let mut output = String::new();
    let mut line_count = 0;
    let mut truncated = false;

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        line_count += 1;
        if line_count > MAX_DIFF_LINES {
            truncated = true;
            break;
        }

        output.push_str(&line);
        output.push('\n');
    }

    // Kill process early if truncated
    if truncated {
        let _ = child.kill();
    }
    let status = child.wait();

    let content = output.trim_end_matches('\n').to_string();

    // If command failed and produced no output, propagate error for fallback
    if !truncated && content.is_empty() {
        if let Ok(s) = status {
            if !s.success() {
                return Err("git diff returned non-zero with no output".to_string());
            }
        }
    }

    Ok((content, truncated))
}
