use crate::catalog::models::{InstallMethod, Risk, Tool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

pub fn classify(tool: &Tool) -> RiskLevel {
    if tool.category == crate::catalog::models::ToolCategory::Agents || matches!(tool.risk, Risk::High) {
        return RiskLevel::High;
    }

    if tool
        .installers
        .iter()
        .any(|installer| matches!(installer.method, InstallMethod::Script))
    {
        return RiskLevel::High;
    }

    if tool
        .tags
        .iter()
        .any(|tag| matches!(tag.as_str(), "system" | "privileged"))
    {
        return RiskLevel::Medium;
    }

    RiskLevel::Low
}
