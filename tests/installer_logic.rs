use t4e::catalog::models::{
    Audience, Check, Exposure, InstallMethod, Installer, Platform, Risk, RunSpec, Tool,
    ToolCategory,
};
use t4e::installer::engine::{InstallPolicy, build_install_task};
use t4e::installer::resolver::{Candidate, PackageSearch, rank_candidates, resolve_with_fallback};

fn fake_tool(risk: Risk) -> Tool {
    Tool {
        id: "fake-tool".to_string(),
        name: "Fake Tool".to_string(),
        description: None,
        install_timeout_sec: None,
        category: ToolCategory::Utility,
        tags: vec![],
        audience: Audience::General,
        risk,
        exposure: Exposure::Starter,
        run: RunSpec {
            cmd: "fake".to_string(),
        },
        run_options: Vec::new(),
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
    assert_eq!(
        ranked.auto_candidate().map(|c| c.package.as_str()),
        Some("ripgrep")
    );
}

#[test]
fn script_installers_always_require_confirmation() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Script,
        package_hints: vec!["example".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: Some("curl https://example.com/install.sh | bash".to_string()),
        requires_confirm: false,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.requires_confirmation);
}

#[test]
fn high_risk_tools_require_confirmation_even_for_pkg_manager() {
    let tool = fake_tool(Risk::High);
    let installer = Installer {
        platform: Platform::Macos,
        method: InstallMethod::Brew,
        package_hints: vec!["codex".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.requires_confirmation);
    assert_eq!(task.command, "brew install codex");
}

#[test]
fn apt_command_uses_cached_sudo_noninteractively() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Apt,
        package_hints: vec!["ripgrep".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(
        task.command,
        "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y ripgrep"
    );
}

#[test]
fn pipx_install_bootstraps_the_package_manager() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Pipx,
        package_hints: vec!["yewtube".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.command.contains("install -y pipx"));
    assert!(task.command.ends_with("pipx install yewtube"));
}

#[test]
fn cargo_install_uses_the_published_lockfile() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Cargo,
        package_hints: vec!["spotatui".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(task.command, "cargo install --locked spotatui");
}

#[test]
fn cargo_install_bootstraps_declared_system_dependencies_and_binaries() {
    let mut tool = fake_tool(Risk::Safe);
    tool.install_timeout_sec = Some(3_600);
    tool.checks = vec![
        Check {
            which: Some("termusic".to_string()),
            version: None,
            custom: None,
        },
        Check {
            which: Some("termusic-server".to_string()),
            version: None,
            custom: None,
        },
    ];
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Cargo,
        package_hints: vec!["termusic".to_string(), "termusic-server".to_string()],
        system_packages: vec![
            "protobuf-compiler".to_string(),
            "libasound2-dev".to_string(),
        ],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.requires_privileges);
    assert_eq!(task.check_command.as_deref(), Some("termusic"));
    assert_eq!(task.additional_check_commands, ["termusic-server"]);
    assert_eq!(task.effective_timeout_sec(1_080), 3_600);
    assert_eq!(
        task.command,
        "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y protobuf-compiler libasound2-dev && cargo install --locked termusic termusic-server"
    );
}

#[test]
fn snap_command_uses_cached_sudo_noninteractively() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Snap,
        package_hints: vec!["asciiquarium".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(task.command, "sudo -n snap install asciiquarium");
    assert!(!task.requires_confirmation);
}

#[test]
fn classic_snap_command_uses_cached_sudo_noninteractively() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::SnapClassic,
        package_hints: vec!["yazi".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(task.command, "sudo -n snap install --classic yazi");
    assert!(!task.requires_confirmation);
}

#[derive(Default)]
struct MockSearch {
    values: Vec<String>,
}

impl PackageSearch for MockSearch {
    fn search(&self, _hint: &str, _method: &InstallMethod) -> anyhow::Result<Vec<String>> {
        Ok(self.values.clone())
    }
}

#[test]
fn resolver_uses_search_fallback_when_no_exact_match() {
    let initial = vec![Candidate {
        package: "rg".to_string(),
        method: InstallMethod::Apt,
    }];
    let search = MockSearch {
        values: vec!["ripgrep".to_string(), "ripgrep-all".to_string()],
    };

    let decision = resolve_with_fallback("ripgrep", InstallMethod::Apt, &initial, &search)
        .expect("resolution succeeds");
    assert_eq!(decision.exact.len(), 1);
    assert_eq!(decision.exact[0].package, "ripgrep");
}

#[test]
fn resolver_keeps_local_candidates_when_search_returns_empty() {
    let initial = vec![Candidate {
        package: "ripgrep-all".to_string(),
        method: InstallMethod::Apt,
    }];
    let search = MockSearch { values: vec![] };

    let decision =
        resolve_with_fallback("rip", InstallMethod::Apt, &initial, &search).expect("resolve ok");
    assert_eq!(decision.prefix.len(), 1);
    assert_eq!(decision.prefix[0].package, "ripgrep-all");
}

#[test]
fn unsafe_package_hint_is_rejected() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Apt,
        package_hints: vec!["ripgrep; rm -rf /".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
    };

    assert!(build_install_task(&tool, &installer, &InstallPolicy::default()).is_err());
}

#[test]
fn non_script_installer_cannot_override_the_generated_command() {
    let tool = fake_tool(Risk::Safe);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Apt,
        package_hints: vec!["ripgrep".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: Some("curl https://example.com | sh".to_string()),
        requires_confirm: false,
    };

    assert!(build_install_task(&tool, &installer, &InstallPolicy::default()).is_err());
}
