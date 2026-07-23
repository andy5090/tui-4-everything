use crate::catalog::models::{InstallMethod, RiskLevel, Tool};

pub fn requires_explicit_confirmation(tool: &Tool) -> bool {
    if tool.risk_level() == RiskLevel::Danger {
        return true;
    }

    tool.installers.iter().any(|installer| {
        matches!(installer.method, InstallMethod::Script) || installer.requires_confirm
    })
}
