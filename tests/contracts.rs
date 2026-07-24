use t4e::agents::risk::classify;
use t4e::app::events::{EventAction, Screen, map_key};
use t4e::catalog::models::{
    Audience, Capability, Exposure, InstallMethod, Installer, RiskLevel, RunSpec, Tool,
    ToolCategory,
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
fn runtime_risk_is_derived_only_from_capabilities() {
    let agent = Tool {
        id: "codex-cli".to_string(),
        name: "Codex CLI".to_string(),
        description: None,
        key_hints: vec![],
        install_timeout_sec: None,
        category: ToolCategory::Agents,
        tags: vec![],
        audience: Audience::Developer,
        capabilities: vec![
            Capability::FileRead,
            Capability::FileWrite,
            Capability::Commands,
            Capability::Autonomous,
        ],
        exposure: Exposure::Starter,
        run: RunSpec {
            cmd: "codex".to_string(),
            keep_open: false,
        },
        launch_argument: None,
        run_options: Vec::new(),
        installers: vec![],
        checks: vec![],
        notes: None,
    };
    assert_eq!(classify(&agent), RiskLevel::Danger);

    let script_tool = Tool {
        id: "custom".to_string(),
        name: "Custom".to_string(),
        description: None,
        key_hints: vec![],
        install_timeout_sec: None,
        category: ToolCategory::Utility,
        tags: vec![],
        audience: Audience::General,
        capabilities: vec![],
        exposure: Exposure::Starter,
        run: RunSpec {
            cmd: "custom".to_string(),
            keep_open: false,
        },
        launch_argument: None,
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
    assert_eq!(classify(&script_tool), RiskLevel::Safe);
}

#[test]
fn risk_level_uses_the_highest_declared_capability() {
    let mut tool = Tool {
        id: "capability-test".to_string(),
        name: "Capability Test".to_string(),
        description: None,
        key_hints: vec![],
        install_timeout_sec: None,
        category: ToolCategory::Utility,
        tags: vec![],
        audience: Audience::General,
        capabilities: vec![],
        exposure: Exposure::Starter,
        run: RunSpec {
            cmd: "capability-test".to_string(),
            keep_open: false,
        },
        launch_argument: None,
        run_options: Vec::new(),
        installers: vec![],
        checks: vec![],
        notes: None,
    };

    assert_eq!(tool.risk_level(), RiskLevel::Safe);
    tool.capabilities.push(Capability::Network);
    assert_eq!(tool.risk_level(), RiskLevel::Low);
    tool.capabilities.push(Capability::CameraCapture);
    assert_eq!(tool.risk_level(), RiskLevel::High);
    tool.capabilities.push(Capability::Delete);
    assert_eq!(tool.risk_level(), RiskLevel::High);
    tool.capabilities.push(Capability::Autonomous);
    assert_eq!(tool.risk_level(), RiskLevel::Danger);
}
