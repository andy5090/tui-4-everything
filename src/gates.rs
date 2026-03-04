use serde::{Deserialize, Serialize};

pub const CANONICAL_SAMPLE_TOOLS: [&str; 10] = [
    "curl",
    "wget",
    "jq",
    "ripgrep",
    "fzf",
    "tmux",
    "neovim",
    "ffmpeg",
    "yt-dlp",
    "tree",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttemptResult {
    pub exit_code: i32,
    pub classification: FailureClassification,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailureClassification {
    #[serde(rename = "infra")]
    Infra,
    #[serde(rename = "product")]
    Product,
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolGateResult {
    pub tool_id: String,
    pub attempts: Vec<AttemptResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateSummary {
    pub first_attempt_successes: u32,
    pub successful_tools: u32,
    pub tools_with_retry: u32,
    pub attempts_total: u32,
    pub first_pass_rate: f64,
    pub success_rate: f64,
    pub retry_used_rate: f64,
    pub infra_failures: u32,
    pub product_failures: u32,
    pub status: String,
}

pub fn compute_gate_summary(results: &[ToolGateResult], required_success_rate: f64) -> GateSummary {
    let total_tools = results.len() as f64;
    let mut first_attempt_successes = 0_u32;
    let mut successful_tools = 0_u32;
    let mut tools_with_retry = 0_u32;
    let mut attempts_total = 0_u32;
    let mut infra_failures = 0_u32;
    let mut product_failures = 0_u32;

    for result in results {
        if result.attempts.len() > 1 {
            tools_with_retry += 1;
        }
        attempts_total += result.attempts.len() as u32;

        if let Some(first) = result.attempts.first() {
            if first.exit_code == 0 {
                first_attempt_successes += 1;
            }
        }

        let mut success = false;
        for attempt in &result.attempts {
            if attempt.exit_code == 0 {
                success = true;
            } else {
                match attempt.classification {
                    FailureClassification::Infra => infra_failures += 1,
                    FailureClassification::Product => product_failures += 1,
                    FailureClassification::None => {}
                }
            }
        }
        if success {
            successful_tools += 1;
        }
    }

    let first_pass_rate = if total_tools == 0.0 {
        0.0
    } else {
        f64::from(first_attempt_successes) / total_tools
    };
    let success_rate = if total_tools == 0.0 {
        0.0
    } else {
        f64::from(successful_tools) / total_tools
    };
    let retry_used_rate = if total_tools == 0.0 {
        0.0
    } else {
        f64::from(tools_with_retry) / total_tools
    };

    let status = if success_rate >= required_success_rate {
        "pass"
    } else {
        "fail"
    }
    .to_string();

    GateSummary {
        first_attempt_successes,
        successful_tools,
        tools_with_retry,
        attempts_total,
        first_pass_rate,
        success_rate,
        retry_used_rate,
        infra_failures,
        product_failures,
        status,
    }
}
