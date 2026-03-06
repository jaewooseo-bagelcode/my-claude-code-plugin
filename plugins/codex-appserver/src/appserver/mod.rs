//! Codex App Server client module.
//!
//! Provides a JSON-RPC 2.0 client for communicating with `codex app-server`
//! over stdio, plus protocol types and review output structures.

pub mod client;
pub mod protocol;

pub use client::{CodexAppServerClient, ShutdownStatus};
pub use protocol::{
    coder_output_schema, review_output_schema, CoderOutput, CoderStatus, Dimension, FileAction,
    FileChange, Finding, JsonRpcError, ReviewOutput, Severity,
};
