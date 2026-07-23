use crate::catalog::models::{RiskLevel, Tool};

pub fn classify(tool: &Tool) -> RiskLevel {
    tool.risk_level()
}
