use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Downloading,
    Verifying,
    Preparing,
    Ready,
    Waiting,
    Installing,
    Complete,
    Cancelled,
    Failed,
    RecoveryFailed,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub tag: String,
    pub downloaded: u64,
    pub total: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(default)]
    pub backup_path: Option<String>,
}
impl Progress {
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Downloading
                | Phase::Verifying
                | Phase::Preparing
                | Phase::Ready
                | Phase::Waiting
                | Phase::Installing
        )
    }
}
