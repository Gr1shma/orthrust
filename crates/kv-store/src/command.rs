use serde::{Deserialize, Serialize};

/// Each entry in the Raft log is one of these.
/// Raft replicates commands; the store executes them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    Set { key: String, value: String },
    Delete { key: String },
}
