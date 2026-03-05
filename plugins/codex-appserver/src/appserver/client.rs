//! Codex App Server client — JSON-RPC 2.0 over stdio.
//!
//! Spawns `codex app-server`, sends requests/notifications on stdin,
//! reads JSONL responses/notifications from stdout via a background reader task.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;

use super::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, ServerMessage};

/// Maximum accumulated agent text size (16 MB).
const MAX_AGENT_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// Default timeout for JSON-RPC requests (60 seconds).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Maximum bytes per line read from stdout (32 MB).
/// Lines exceeding this are truncated to prevent OOM from malformed output.
const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;

/// Maximum bytes to log from a parse-error line (avoids leaking sensitive content).
const MAX_LOG_LINE_BYTES: usize = 200;

/// Shutdown result reporting what happened during teardown.
#[derive(Debug)]
pub struct ShutdownStatus {
    pub shutdown_request: Result<(), String>,
    pub exit_notify: Result<(), String>,
    pub process_exited: bool,
}

impl ShutdownStatus {
    pub fn is_clean(&self) -> bool {
        self.shutdown_request.is_ok() && self.exit_notify.is_ok() && self.process_exited
    }
}

/// Client for communicating with a `codex app-server` process.
pub struct CodexAppServerClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    response_map: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    turn_completed_rx: mpsc::UnboundedReceiver<Value>,
    agent_text: Arc<Mutex<String>>,
    next_id: AtomicU64,
    _reader_task: JoinHandle<()>,
}

impl CodexAppServerClient {
    /// Spawn the `codex app-server` process and start the background reader.
    pub async fn spawn() -> Result<Self, String> {
        let mut child = Command::new("codex")
            .arg("app-server")
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to spawn codex app-server: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or("Failed to capture stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Failed to capture stdout")?;

        let stdin = BufWriter::new(stdin);
        let response_map: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let agent_text: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        // Unbounded channel: reader must never block on notification dispatch.
        let (turn_completed_tx, turn_completed_rx) = mpsc::unbounded_channel::<Value>();

        // Background reader task: reads JSONL from stdout, dispatches messages.
        let reader_response_map = response_map.clone();
        let reader_agent_text = agent_text.clone();
        let reader_task = tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                // Read next line with size guard.
                let line = match read_line_bounded(&mut lines, MAX_LINE_BYTES).await {
                    Some(Ok(line)) => line,
                    Some(Err(e)) => {
                        eprintln!("[appserver-reader] read error: {e}");
                        continue;
                    }
                    None => break, // EOF
                };

                if line.is_empty() {
                    continue;
                }

                match ServerMessage::parse(&line) {
                    Ok(ServerMessage::Response(resp)) => {
                        if let Some(id) = resp.id {
                            let mut map = reader_response_map.lock().await;
                            if let Some(tx) = map.remove(&id) {
                                let _ = tx.send(resp);
                            }
                        }
                    }
                    Ok(ServerMessage::Notification { method, params }) => {
                        match method.as_str() {
                            "item/agentMessage/delta" => {
                                if let Some(delta) =
                                    params.get("delta").and_then(|d| d.as_str())
                                {
                                    let mut text = reader_agent_text.lock().await;
                                    let remaining =
                                        MAX_AGENT_TEXT_BYTES.saturating_sub(text.len());
                                    if remaining > 0 {
                                        let take =
                                            truncate_to_char_boundary(delta, remaining);
                                        text.push_str(take);
                                    }
                                }
                            }
                            "turn/completed" => {
                                let _ = turn_completed_tx.send(params);
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        let redacted = truncate_to_char_boundary(&line, MAX_LOG_LINE_BYTES);
                        eprintln!(
                            "[appserver-reader] parse error: {e} — line[..{}]: {redacted}",
                            redacted.len()
                        );
                    }
                }
            }

            // EOF: server exited. Drain all pending request senders so callers
            // get an error instead of waiting forever.
            let mut map = reader_response_map.lock().await;
            for (_id, tx) in map.drain() {
                let _ = tx.send(JsonRpcResponse {
                    id: None,
                    result: None,
                    error: Some(super::protocol::JsonRpcError {
                        code: -1,
                        message: "App server exited".to_string(),
                        data: None,
                    }),
                });
            }
        });

        Ok(Self {
            child,
            stdin,
            response_map,
            turn_completed_rx,
            agent_text,
            next_id: AtomicU64::new(1),
            _reader_task: reader_task,
        })
    }

    /// Send a JSON-RPC request and wait for the matching response (with timeout).
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.request_with_timeout(method, params, REQUEST_TIMEOUT)
            .await
    }

    /// Send a JSON-RPC request with a custom timeout.
    pub async fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest::new(id, method, params);

        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.response_map.lock().await;
            map.insert(id, tx);
        }

        // On write failure, clean up the pending sender before returning.
        if let Err(e) = self.send_line(&req).await {
            let mut map = self.response_map.lock().await;
            map.remove(&id);
            return Err(e);
        }

        // Wait with timeout to prevent deadlock if server stops responding.
        let resp = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => return Err("Response channel closed".to_string()),
            Err(_) => {
                // Timeout: deterministically clean up the stale sender.
                let mut map = self.response_map.lock().await;
                map.remove(&id);
                return Err(format!(
                    "Request '{method}' timed out after {}s",
                    timeout.as_secs()
                ));
            }
        };

        if let Some(err) = resp.error {
            return Err(err.to_string());
        }
        Ok(resp.result.unwrap_or(Value::Null))
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let notif = JsonRpcNotification::new(method, params);
        self.send_line(&notif).await
    }

    /// Serialize and write a value as a JSONL line to stdin.
    async fn send_line(&mut self, value: &impl serde::Serialize) -> Result<(), String> {
        let line = serde_json::to_string(value).map_err(|e| format!("Serialize error: {e}"))?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Write error: {e}"))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Write newline error: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("Flush error: {e}"))?;
        Ok(())
    }

    /// Wait for a `turn/completed` notification matching a specific turn,
    /// with a timeout. Stale completions for other turns are discarded.
    pub async fn wait_turn_completed(
        &mut self,
        expected_turn_id: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err("Timeout waiting for turn/completed".to_string());
            }

            let params = tokio::time::timeout(remaining, self.turn_completed_rx.recv())
                .await
                .map_err(|_| "Timeout waiting for turn/completed".to_string())?
                .ok_or_else(|| "turn/completed channel closed".to_string())?;

            // If no specific turn ID requested, accept any completion.
            let Some(expected) = expected_turn_id else {
                return Ok(params);
            };

            // Check if this completion matches the expected turn.
            let actual_id = params
                .get("turn")
                .and_then(|t| t.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if actual_id == expected {
                return Ok(params);
            }

            // Stale completion — discard and keep waiting.
            eprintln!(
                "[appserver] discarding stale turn/completed (expected={expected}, got={actual_id})"
            );
        }
    }

    /// Get the accumulated agent text from all `item/agentMessage/delta` notifications.
    pub async fn accumulated_text(&self) -> String {
        self.agent_text.lock().await.clone()
    }

    /// Clear the accumulated agent text (useful between turns).
    pub async fn clear_text(&self) {
        self.agent_text.lock().await.clear();
    }

    /// Gracefully shut down the app server. Returns status of each teardown step.
    pub async fn shutdown(mut self) -> ShutdownStatus {
        let shutdown_timeout = Duration::from_secs(5);

        let shutdown_request = self
            .request_with_timeout("shutdown", Value::Null, shutdown_timeout)
            .await
            .map(|_| ());

        let exit_notify = self.notify("exit", Value::Null).await;

        let process_exited =
            tokio::time::timeout(shutdown_timeout, self.child.wait())
                .await
                .is_ok();

        ShutdownStatus {
            shutdown_request,
            exit_notify,
            process_exited,
        }
    }
}

impl Drop for CodexAppServerClient {
    fn drop(&mut self) {
        // kill_on_drop(true) handles process cleanup.
        // Abort the reader task to prevent leaked background work.
        self._reader_task.abort();
    }
}

/// Read a line from a `Lines` stream, enforcing a maximum byte length.
/// Returns `None` on EOF, `Some(Err)` on read error, `Some(Ok(line))` on success.
/// Lines exceeding `max_bytes` are truncated at a UTF-8 boundary.
async fn read_line_bounded(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    max_bytes: usize,
) -> Option<Result<String, std::io::Error>> {
    match lines.next_line().await {
        Ok(Some(line)) => {
            if line.len() > max_bytes {
                let truncated = truncate_to_char_boundary(&line, max_bytes);
                Some(Ok(truncated.to_string()))
            } else {
                Some(Ok(line))
            }
        }
        Ok(None) => None,
        Err(e) => Some(Err(e)),
    }
}

/// Truncate a string slice to at most `max_bytes`, ending on a char boundary.
pub(crate) fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii() {
        assert_eq!(truncate_to_char_boundary("hello", 3), "hel");
        assert_eq!(truncate_to_char_boundary("hello", 10), "hello");
        assert_eq!(truncate_to_char_boundary("hello", 5), "hello");
    }

    #[test]
    fn truncate_multibyte() {
        assert_eq!(truncate_to_char_boundary("한글", 6), "한글");
        assert_eq!(truncate_to_char_boundary("한글", 5), "한");
        assert_eq!(truncate_to_char_boundary("한글", 3), "한");
        assert_eq!(truncate_to_char_boundary("한글", 2), "");
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate_to_char_boundary("", 10), "");
        assert_eq!(truncate_to_char_boundary("hello", 0), "");
    }

    #[test]
    fn truncate_mixed_ascii_multibyte() {
        // "aé" = a(1) + é(2) = 3 bytes
        assert_eq!(truncate_to_char_boundary("aé", 2), "a");
        assert_eq!(truncate_to_char_boundary("aé", 3), "aé");
    }

    #[test]
    fn truncate_emoji() {
        // 🎉 = 4 bytes
        assert_eq!(truncate_to_char_boundary("🎉x", 4), "🎉");
        assert_eq!(truncate_to_char_boundary("🎉x", 3), "");
        assert_eq!(truncate_to_char_boundary("🎉x", 5), "🎉x");
    }
}
