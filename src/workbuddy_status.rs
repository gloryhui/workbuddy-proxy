//! Credential-free result of checking WorkBuddy's saved proxy endpoints.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EndpointCheck {
    pub expected_address: String,
    pub checked_count: usize,
    pub mismatch_count: usize,
    /// Unique origins only; never credentials, paths, queries or fragments.
    pub actual_addresses: Vec<String>,
}
