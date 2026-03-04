use t4e::catalog::models::{
    Audience, Exposure, InstallMethod, Installer, Platform, Risk, RunSpec, Tool, ToolCategory,
};
use t4e::installer::engine::{InstallPolicy, build_install_task};
use t4e::installer::resolver::{Candidate, rank_candidates};

fn fake_tool(risk: Risk) -> Tool {
    Tool {
        id: "fake-tool".to_string(),
        name: "Fake Tool".to_string(),
        category: ToolCategory::Utility,
        tags: vec![],
        audience: Audience::General,
        risk,
        exposure: Exposure::Starter,
        run: RunSpec {
            cmd: "fake".to_string(),
        },
        installers: vec![],
        checks: vec![],
        notes: None,
    }
}

#[test]
fn resolver_prefers_exact_then_prefix_then_contains() {
    let candidates = vec![
        Candidate {
            package: "ripgrep-all".to_string(),
            method: InstallMethod::Apt,
        },
        Candidate {
            package: "my-ripgrep".to_string(),
            method: InstallMethod::Apt,
        },
        Candidate {
            package: "ripgrep".to_string(),
            method: InstallMethod::Apt,
        },
    ];

    let ranked = rank_candidates("ripgrep", &candidates);
    assert_eq!(ranked.exact.len(), 1);
    assert_eq!(ranked.exact[0].package, "ripgrep");
    assert_eq!(ranked.auto_candidate().map(|c| c.package.as_str()), Some("ripgrep"));
}

#[test]
fn script_installers_always_require_confirmation() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Script,
        package_hints: vec!["example".to_string()],
        install_cmd: Some("curl https://example.com/install.sh | bash".to_string()),
        requires_confirm: false,
    };

    let task = build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.requires_confirmation);
}

#[test]
fn high_risk_tools_require_confirmation_even_for_pkg_manager() {
    let tool = fake_tool(Risk::High);
    let installer = Installer {
        platform: Platform::Macos,
        method: InstallMethod::Brew,
        package_hints: vec!["codex".to_string()],
        install_cmd: None,
        requires_confirm: false,
    };

    let task = build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.requires_confirmation);
    assert_eq!(task.command, "brew install codex");
}
