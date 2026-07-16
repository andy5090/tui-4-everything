use t4e::agents::risk::{RiskLevel, classify};
use t4e::app::events::{EventAction, Screen, map_key};
use t4e::catalog::models::{
    Audience, Exposure, InstallMethod, Installer, Risk, RunSpec, Tool, ToolCategory,
};

#[test]
fn event_map_matches_spec_subset() {
    assert_eq!(map_key(Screen::Home, 'c'), EventAction::GoCatalog);
    assert_eq!(map_key(Screen::Catalog, 'j'), EventAction::MoveDown);
    assert_eq!(map_key(Screen::Catalog, '\n'), EventAction::OpenDetail);
    assert_eq!(
        map_key(Screen::Workspace, '\n'),
        EventAction::LaunchWorkspace
    );
    assert_eq!(map_key(Screen::Install, 'r'), EventAction::Retry);
}

#[test]
fn agents_and_script_installs_are_high_risk() {
    let agent = Tool {
        id: "codex-cli".to_string(),
        name: "Codex CLI".to_string(),
        category: ToolCategory::Agents,
        tags: vec![],
        audience: Audience::Developer,
        risk: Risk::High,
        exposure: Exposure::SearchOnly,
        run: RunSpec {
            cmd: "codex".to_string(),
        },
        run_options: Vec::new(),
        installers: vec![],
        checks: vec![],
        notes: None,
    };
    assert_eq!(classify(&agent), RiskLevel::High);

    let script_tool = Tool {
        id: "custom".to_string(),
        name: "Custom".to_string(),
        category: ToolCategory::Utility,
        tags: vec![],
        audience: Audience::General,
        risk: Risk::Safe,
        exposure: Exposure::Starter,
        run: RunSpec {
            cmd: "custom".to_string(),
        },
        run_options: Vec::new(),
        installers: vec![Installer {
            platform: t4e::catalog::models::Platform::Linux,
            method: InstallMethod::Script,
            package_hints: vec!["custom".to_string()],
            system_packages: vec![],
            executable: None,
            install_cmd: Some("curl x | bash".to_string()),
            requires_confirm: true,
        }],
        checks: vec![],
        notes: None,
    };
    assert_eq!(classify(&script_tool), RiskLevel::High);
}
