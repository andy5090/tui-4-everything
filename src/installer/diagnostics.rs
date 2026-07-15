use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureDiagnostics {
    pub exit_code: Option<i32>,
    pub stderr_summary: String,
    pub full_log_path: String,
}

impl FailureDiagnostics {
    pub fn from_stderr(
        exit_code: Option<i32>,
        stderr: &str,
        full_log_path: impl Into<String>,
    ) -> Self {
        let summary = stderr.lines().take(3).collect::<Vec<_>>().join(" | ");

        Self {
            exit_code,
            stderr_summary: summary,
            full_log_path: full_log_path.into(),
        }
    }
}
