use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const CANONICAL_SAMPLE_TOOLS: [&str; 10] = [
    "curl", "wget", "jq", "ripgrep", "fzf", "tmux", "neovim", "ffmpeg", "yt-dlp", "tree",
];

pub const CANONICAL_SAMPLE_SIZE: usize = CANONICAL_SAMPLE_TOOLS.len();
pub const MAX_ATTEMPTS_PER_TOOL: usize = 2;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SampleSet {
    pub version: String,
    pub size: usize,
    pub tool_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GatePolicy {
    pub max_attempts_per_tool: usize,
    pub per_attempt_timeout_sec: u32,
    pub gate_timeout_sec: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GateReport {
    pub gate_id: String,
    pub run_id: String,
    pub os: String,
    pub sample_set: SampleSet,
    pub policy: GatePolicy,
    pub summary: GateSummary,
    pub tool_results: Vec<ToolGateResult>,
}

pub fn validate_gate_input(results: &[ToolGateResult]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for result in results {
        if !seen.insert(result.tool_id.clone()) {
            bail!("duplicate tool result for {}", result.tool_id);
        }

        if !CANONICAL_SAMPLE_TOOLS.contains(&result.tool_id.as_str()) {
            bail!("tool {} is outside canonical sample set", result.tool_id);
        }

        if result.attempts.len() > MAX_ATTEMPTS_PER_TOOL {
            bail!(
                "tool {} exceeds attempt budget {}",
                result.tool_id,
                MAX_ATTEMPTS_PER_TOOL
            );
        }
    }

    Ok(())
}

pub fn compute_gate_summary(results: &[ToolGateResult], required_success_rate: f64) -> GateSummary {
    let index: BTreeMap<&str, &ToolGateResult> =
        results.iter().map(|item| (item.tool_id.as_str(), item)).collect();

    let total_tools = CANONICAL_SAMPLE_SIZE as f64;
    let mut first_attempt_successes = 0_u32;
    let mut successful_tools = 0_u32;
    let mut tools_with_retry = 0_u32;
    let mut attempts_total = 0_u32;
    let mut infra_failures = 0_u32;
    let mut product_failures = 0_u32;

    for tool_id in CANONICAL_SAMPLE_TOOLS {
        if let Some(result) = index.get(tool_id) {
            if result.attempts.len() > 1 {
                tools_with_retry += 1;
            }
            attempts_total += result.attempts.len() as u32;

            if let Some(first) = result.attempts.first()
                && first.exit_code == 0
            {
                first_attempt_successes += 1;
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
    }

    let first_pass_rate = f64::from(first_attempt_successes) / total_tools;
    let success_rate = f64::from(successful_tools) / total_tools;
    let retry_used_rate = f64::from(tools_with_retry) / total_tools;

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

pub fn build_gate_report(
    gate_id: impl Into<String>,
    run_id: impl Into<String>,
    os: impl Into<String>,
    results: Vec<ToolGateResult>,
    required_success_rate: f64,
) -> Result<GateReport> {
    validate_gate_input(&results)?;
    let summary = compute_gate_summary(&results, required_success_rate);

    Ok(GateReport {
        gate_id: gate_id.into(),
        run_id: run_id.into(),
        os: os.into(),
        sample_set: SampleSet {
            version: "v0.1".to_string(),
            size: CANONICAL_SAMPLE_SIZE,
            tool_ids: CANONICAL_SAMPLE_TOOLS
                .iter()
                .map(ToString::to_string)
                .collect(),
        },
        policy: GatePolicy {
            max_attempts_per_tool: MAX_ATTEMPTS_PER_TOOL,
            per_attempt_timeout_sec: 600,
            gate_timeout_sec: 2700,
        },
        summary,
        tool_results: results,
    })
}
