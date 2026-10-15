//! Native upstream adapters. Chat Completions keeps its original byte path.
//!
//! The conversion boundary is deliberately independent of HTTP and SQLite;
//! relay integration owns transport, cancellation and the existing stats guard.
pub mod config;
pub mod encode;
pub mod replay;
pub mod request;
pub mod sse;

/// Errors contain static descriptions and structural paths, never input values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub param: Option<String>,
    pub status: u16,
}

impl Error {
    pub fn input(code: &'static str, path: impl Into<String>) -> Self {
        Self {
            code,
            param: Some(path.into()),
            status: 400,
        }
    }

    pub fn upstream(code: &'static str) -> Self {
        Self {
            code,
            param: None,
            status: 502,
        }
    }

    pub fn envelope(&self) -> serde_json::Value {
        serde_json::json!({"error":{
            "message": match self.status {
                409 => "Conversation state expired; start a new conversation",
                400 => "Request cannot be represented by the selected upstream protocol",
                _ => "Upstream protocol response was invalid or incomplete",
            },
            "type": if self.status == 400 { "invalid_request_error" } else { "upstream_error" },
            "param":self.param,"code":self.code
        }})
    }
}
