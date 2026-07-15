use crate::catalog::models::{InstallMethod, Risk, Tool};

pub fn requires_explicit_confirmation(tool: &Tool) -> bool {
    if matches!(tool.risk, Risk::Admin | Risk::High) {
        return true;
    }

    tool.installers.iter().any(|installer| {
        matches!(installer.method, InstallMethod::Script) || installer.requires_confirm
    })
}

pub fn is_search_only_agent(tool: &Tool) -> bool {
    matches!(tool.risk, Risk::High) && tool.category == crate::catalog::models::ToolCategory::Agents
}
