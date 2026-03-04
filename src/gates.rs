use serde::{Deserialize, Serialize};

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
