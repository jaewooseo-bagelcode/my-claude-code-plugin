//! Codex App Server client module.
//!
//! Provides a JSON-RPC 2.0 client for communicating with `codex app-server`
//! over stdio, plus protocol types and review output structures.

pub mod client;
pub mod protocol;

pub use client::{CodexAppServerClient, ShutdownStatus};
pub use protocol::{
    review_output_schema, Dimension, Finding, JsonRpcError, ReviewOutput, Severity,
};
