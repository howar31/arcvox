//! Download progress reporting. The download itself is added separately.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadProgress {
    pub id: String,
    pub received: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}
