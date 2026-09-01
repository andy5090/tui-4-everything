use t4e::catalog::models::{
    Audience, Capability, Check, Exposure, InstallMethod, Installer, Platform, RunSpec, Tool,
    ToolCategory, VerifiedUpdate, VersionProbe,
};
use t4e::installer::engine::{InstallPolicy, build_install_task, build_verified_update_task};
use t4e::installer::resolver::{Candidate, PackageSearch, rank_candidates, resolve_with_fallback};

fn fake_tool(capabilities: Vec<Capability>) -> Tool {
    Tool {
        id: "fake-tool".to_string(),
        name: "Fake Tool".to_string(),
        description: None,
        key_hints: vec![],
        install_timeout_sec: None,
        category: ToolCategory::Utility,
        tags: vec![],
        audience: Audience::General,
        capabilities,
        exposure: Exposure::Starter,
        run: RunSpec {
            cmd: "fake".to_string(),
            keep_open: false,
        },
        launch_argument: None,
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
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Script,
        architectures: vec![],
        package_hints: vec!["example".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: Some("curl https://example.com/install.sh | bash".to_string()),
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.requires_confirmation);
}

#[test]
fn danger_tools_require_confirmation_even_for_pkg_manager() {
    let tool = fake_tool(vec![Capability::Commands]);
    let installer = Installer {
        platform: Platform::Macos,
        method: InstallMethod::Brew,
        architectures: vec![],
        package_hints: vec!["codex".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.requires_confirmation);
    assert_eq!(task.command, "brew install codex");
}

#[test]
fn apt_command_uses_cached_sudo_noninteractively() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Apt,
        architectures: vec![],
        package_hints: vec!["ripgrep".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(
        task.command,
        "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y ripgrep"
    );
}

#[test]
fn termux_pkg_install_never_requests_sudo() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Termux,
        method: InstallMethod::Pkg,
        architectures: vec![],
        package_hints: vec!["ripgrep".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(task.command, "pkg install -y ripgrep");
    assert!(!task.requires_privileges);
}

#[test]
fn termux_pip_install_bootstraps_python_without_sudo() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Termux,
        method: InstallMethod::Pip,
        architectures: vec![],
        package_hints: vec!["yt-dlp".to_string()],
        system_packages: vec!["python".to_string()],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(
        task.command,
        "pkg install -y python && python -m pip install --upgrade yt-dlp"
    );
    assert!(!task.requires_privileges);
}

#[test]
fn verified_update_task_uses_pinned_command_and_records_expected_version() {
    let mut tool = fake_tool(vec![]);
    tool.checks = vec![Check {
        which: Some("ripgrep".to_string()),
        version: None,
        custom: None,
    }];
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Apt,
        architectures: vec![],
        package_hints: vec!["ripgrep".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: Some(VerifiedUpdate {
            version: "1.2.3".to_string(),
            version_probe: VersionProbe {
                executable: "ripgrep".to_string(),
                args: vec!["--version".to_string()],
            },
            command: "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get install -y ripgrep=1.2.3"
                .to_string(),
            verified_at: "2026-07-30".to_string(),
            evidence: "https://example.com/ripgrep-1.2.3".to_string(),
        }),
    };

    let task = build_verified_update_task(&tool, &installer, &InstallPolicy::default())
        .expect("verified task builds");

    assert_eq!(
        task.command,
        "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get install -y ripgrep=1.2.3"
    );
    assert_eq!(task.check_command.as_deref(), Some("ripgrep"));
    assert_eq!(task.expected_version.as_deref(), Some("1.2.3"));
    assert_eq!(
        task.version_probe,
        Some(VersionProbe {
            executable: "ripgrep".to_string(),
            args: vec!["--version".to_string()],
        })
    );
    assert!(task.is_verified_update());
}

#[test]
fn pipx_install_bootstraps_the_package_manager() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Pipx,
        architectures: vec![],
        package_hints: vec!["yewtube".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.command.contains("install -y pipx"));
    assert!(task.command.ends_with("pipx install yewtube"));
}

#[test]
fn yewtube_install_creates_managed_terminal_video_renderers() {
    let mut tool = fake_tool(vec![Capability::Network]);
    tool.id = "yewtube".to_string();
    tool.run.cmd = "yt".to_string();
    tool.checks = vec![
        Check {
            which: Some("yt".to_string()),
            version: None,
            custom: None,
        },
        Check {
            which: Some("t4e-yewtube".to_string()),
            version: None,
            custom: None,
        },
        Check {
            which: Some("t4e-mpv-terminal".to_string()),
            version: None,
            custom: None,
        },
    ];
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Yewtube,
        architectures: vec![],
        package_hints: vec!["yewtube".to_string()],
        system_packages: vec!["mpv".to_string(), "python3".to_string()],
        executable: Some("t4e-yewtube".to_string()),
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");

    assert!(task.command.contains("pipx install --force yewtube"));
    assert!(task.command.contains("mps-youtube"));
    assert!(task.command.contains("T4E_MPV_RENDERER"));
    assert!(task.command.contains("d[\\\"PLAYER\\\"]=sys.argv[2]"));
    assert!(task.command.contains("--vo=tct --profile=sw-fast"));
    assert!(task.command.contains("--vo=caca"));
    assert!(task.command.contains("t4e-yewtube"));
    assert_eq!(task.check_command.as_deref(), Some("yt"));
    assert_eq!(
        task.additional_check_commands,
        ["t4e-yewtube", "t4e-mpv-terminal"]
    );
    assert!(task.requires_privileges);
}

#[test]
fn ascii_camera_install_reuses_mpv_without_opencv() {
    let mut tool = fake_tool(vec![Capability::CameraCapture]);
    tool.id = "ascii-camera".to_string();
    tool.run.cmd = "t4e-ascii-camera".to_string();
    tool.checks = vec![
        Check {
            which: Some("mpv".to_string()),
            version: None,
            custom: None,
        },
        Check {
            which: Some("t4e-ascii-camera-v3".to_string()),
            version: None,
            custom: None,
        },
    ];
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::AsciiCamera,
        architectures: vec![],
        package_hints: vec!["mpv".to_string()],
        system_packages: vec!["mpv".to_string()],
        executable: Some("t4e-ascii-camera".to_string()),
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");

    assert!(task.command.contains("install -y mpv"));
    assert!(task.command.contains("t4e-ascii-camera"));
    assert!(task.command.contains("av://v4l2:/dev/video"));
    assert!(task.command.contains("renderer=tct"));
    assert!(task.command.contains("\"--vo=$renderer\""));
    assert!(!task.command.to_ascii_lowercase().contains("opencv"));
    assert_eq!(task.check_command.as_deref(), Some("mpv"));
    assert_eq!(task.additional_check_commands, ["t4e-ascii-camera-v3"]);
    assert!(task.requires_privileges);
    assert!(!task.requires_confirmation);
}

#[test]
fn cargo_install_uses_the_published_lockfile() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Cargo,
        architectures: vec![],
        package_hints: vec!["spotatui".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(task.command, "cargo install --locked spotatui");
}

#[test]
fn i686_spotatui_uses_the_lightweight_low_memory_build() {
    let mut tool = fake_tool(vec![]);
    tool.id = "spotatui".to_string();
    tool.install_timeout_sec = Some(7_200);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Spotatui,
        architectures: vec![],
        package_hints: vec!["spotatui".to_string()],
        system_packages: vec!["lld".to_string()],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");

    assert!(task.command.contains("CARGO_HTTP_TIMEOUT=600"));
    assert!(task.command.contains("CARGO_HTTP_LOW_SPEED_LIMIT=1"));
    assert!(task.command.contains("CARGO_PROFILE_RELEASE_OPT_LEVEL=0"));
    assert!(task.command.contains("link-arg=-fuse-ld=lld"));
    assert!(
        task.command
            .contains("--no-default-features --features telemetry")
    );
    assert_eq!(task.effective_timeout_sec(600), 7_200);
    assert!(task.requires_privileges);
}

#[test]
fn termleaf_builds_the_verified_tag_from_source_on_termux() {
    let mut tool = fake_tool(vec![Capability::FileWrite]);
    tool.id = "termleaf".to_string();
    tool.checks = vec![
        Check {
            which: Some("termleaf".to_string()),
            version: None,
            custom: None,
        },
        Check {
            which: Some("termleaf-update".to_string()),
            version: None,
            custom: None,
        },
    ];
    let installer = Installer {
        platform: Platform::Termux,
        method: InstallMethod::Termleaf,
        architectures: vec![],
        package_hints: vec!["termleaf".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: true,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");

    assert_eq!(
        task.command,
        "cargo install --locked --git https://github.com/andy5090/termleaf --tag v0.3.5 termleaf"
    );
    assert_eq!(task.check_command.as_deref(), Some("termleaf"));
    assert_eq!(task.additional_check_commands, ["termleaf-update"]);
    assert!(task.requires_confirmation);
    assert!(!task.requires_privileges);
    assert_eq!(task.effective_timeout_sec(1_080), 1_800);
}

#[test]
fn cargo_install_bootstraps_declared_system_dependencies_and_binaries() {
    let mut tool = fake_tool(vec![]);
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
        architectures: vec![],
        package_hints: vec!["termusic".to_string(), "termusic-server".to_string()],
        system_packages: vec![
            "protobuf-compiler".to_string(),
            "libasound2-dev".to_string(),
        ],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
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
fn tplay_install_uses_an_isolated_current_yt_dlp() {
    let mut tool = fake_tool(vec![]);
    tool.id = "tplay".to_string();
    tool.run.cmd = "tplay".to_string();
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Tplay,
        architectures: vec![],
        package_hints: vec!["tplay".to_string()],
        system_packages: vec!["python3-venv".to_string()],
        executable: Some("t4e-tplay".to_string()),
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };
    tool.installers = vec![installer.clone()];

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.command.contains("cargo install --locked tplay"));
    assert!(task.command.contains("python3 -m venv"));
    assert!(
        task.command
            .contains("pip install --upgrade 'yt-dlp[default]'")
    );
    assert!(task.command.contains("t4e-tplay"));
    assert_eq!(task.check_command.as_deref(), Some("t4e-tplay"));
    assert!(task.requires_privileges);
}

#[test]
fn youtube_tui_install_puts_current_yt_dlp_first_for_mpv() {
    let mut tool = fake_tool(vec![Capability::Network]);
    tool.id = "youtube-tui".to_string();
    tool.run.cmd = "youtube-tui".to_string();
    tool.checks = vec![
        Check {
            which: Some("youtube-tui".to_string()),
            version: None,
            custom: None,
        },
        Check {
            which: Some("t4e-youtube-tui-v2".to_string()),
            version: None,
            custom: None,
        },
        Check {
            which: Some("t4e-mpv-terminal".to_string()),
            version: None,
            custom: None,
        },
    ];
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::YoutubeTui,
        architectures: vec![],
        package_hints: vec!["youtube-tui".to_string()],
        system_packages: vec!["mpv".to_string(), "python3-venv".to_string()],
        executable: Some("t4e-youtube-tui-v2".to_string()),
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");

    assert!(task.command.contains("cargo install --locked youtube-tui"));
    assert!(task.command.contains("python3 -m venv"));
    assert!(
        task.command
            .contains("pip install --upgrade 'yt-dlp[default]'")
    );
    assert!(
        task.command
            .contains("PATH=\"$data_dir/player:$data_dir/yt-dlp/bin:$real_path\"")
    );
    assert!(task.command.contains("T4E_PLAYER_PATH"));
    assert!(task.command.contains("T4E_MPV_RENDERER"));
    assert!(task.command.contains("T4E_VIDEO_HOST_PID"));
    assert!(task.command.contains("kill -STOP"));
    assert!(task.command.contains("< /dev/tty > /dev/tty 2>&1"));
    assert!(task.command.contains("--vo=tct --profile=sw-fast"));
    assert!(task.command.contains("--vo=caca"));
    assert!(task.command.contains("t4e-youtube-tui-v2"));
    assert_eq!(task.check_command.as_deref(), Some("youtube-tui"));
    assert_eq!(
        task.additional_check_commands,
        ["t4e-youtube-tui-v2", "t4e-mpv-terminal"]
    );
    assert!(task.requires_privileges);
    assert!(!task.requires_confirmation);
}

#[test]
fn newsboat_install_creates_a_first_feed_launcher() {
    let mut tool = fake_tool(vec![]);
    tool.id = "newsboat".to_string();
    tool.run.cmd = "newsboat".to_string();
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Newsboat,
        architectures: vec![],
        package_hints: vec!["newsboat".to_string()],
        system_packages: vec![],
        executable: Some("t4e-newsboat".to_string()),
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };
    tool.installers = vec![installer.clone()];

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.command.starts_with("sudo -n snap install newsboat"));
    assert!(task.command.contains("t4e-newsboat"));
    assert!(task.command.contains("snap/newsboat/common/t4e"));
    assert!(task.command.contains("Feed URL (Ctrl+C to cancel)"));
    assert!(task.command.contains("exec newsboat -u"));
    assert_eq!(task.check_command.as_deref(), Some("t4e-newsboat"));
}

#[test]
fn snap_command_uses_cached_sudo_noninteractively() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Snap,
        architectures: vec![],
        package_hints: vec!["asciiquarium".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(task.command, "sudo -n snap install asciiquarium");
    assert!(!task.requires_confirmation);
}

#[test]
fn classic_snap_command_uses_cached_sudo_noninteractively() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::SnapClassic,
        architectures: vec![],
        package_hints: vec!["yazi".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert_eq!(task.command, "sudo -n snap install --classic yazi");
    assert!(!task.requires_confirmation);
}

#[test]
fn lazyvim_uses_an_isolated_managed_configuration() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::LazyVim,
        architectures: vec![],
        package_hints: vec!["lazyvim".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
    assert!(task.command.contains("snap install nvim --classic"));
    assert!(task.command.contains("LazyVim/starter"));
    assert!(task.command.contains("NVIM_APPNAME=t4e-lazyvim"));
    assert!(!task.command.contains("/.config/nvim"));
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
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Apt,
        architectures: vec![],
        package_hints: vec!["ripgrep; rm -rf /".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    assert!(build_install_task(&tool, &installer, &InstallPolicy::default()).is_err());
}

#[test]
fn non_script_installer_cannot_override_the_generated_command() {
    let tool = fake_tool(vec![]);
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Apt,
        architectures: vec![],
        package_hints: vec!["ripgrep".to_string()],
        system_packages: vec![],
        executable: None,
        install_cmd: Some("curl https://example.com | sh".to_string()),
        requires_confirm: false,
        verified_update: None,
    };

    assert!(build_install_task(&tool, &installer, &InstallPolicy::default()).is_err());
}

#[test]
fn fastfetch_uses_the_official_architecture_specific_deb() {
    let mut tool = fake_tool(vec![Capability::FileRead]);
    tool.id = "fastfetch".to_string();
    tool.run.cmd = "fastfetch".to_string();
    tool.checks = vec![Check {
        which: Some("fastfetch".to_string()),
        version: None,
        custom: None,
    }];
    let installer = Installer {
        platform: Platform::Linux,
        method: InstallMethod::Fastfetch,
        architectures: vec![],
        package_hints: vec!["fastfetch".to_string()],
        system_packages: vec!["curl".to_string()],
        executable: None,
        install_cmd: None,
        requires_confirm: false,
        verified_update: None,
    };

    let task =
        build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");

    assert!(task.command.contains("dpkg --print-architecture"));
    assert!(
        task.command
            .contains("fastfetch-cli/fastfetch/releases/latest/download")
    );
    assert!(task.command.contains("fastfetch-linux-${asset}.deb"));
    assert!(task.command.contains("apt-get"));
    assert_eq!(task.check_command.as_deref(), Some("fastfetch"));
    assert!(task.requires_privileges);
    assert!(!task.requires_confirmation);
}

#[test]
fn i686_wrappers_use_official_pinned_assets_and_managed_paths() {
    let cases = [
        (
            "glow",
            InstallMethod::Glow,
            vec!["curl"],
            ["glow_2.1.2_i386.deb", "apt-get"],
        ),
        (
            "yazi",
            InstallMethod::Yazi,
            vec![],
            ["yazi-fm@26.5.6", "yazi-cli@26.5.6"],
        ),
        (
            "asciiquarium",
            InstallMethod::Asciiquarium,
            vec!["curl", "perl", "cpanminus", "libcurses-perl"],
            [
                "8bdb7d441a36a5a9f64b853317a66f9d4a82f08f/asciiquarium",
                "t4e/asciiquarium",
            ],
        ),
    ];

    for (tool_id, method, system_packages, expected_fragments) in cases {
        let mut tool = fake_tool(vec![]);
        tool.id = tool_id.to_string();
        tool.run.cmd = tool_id.to_string();
        let requires_privileges = !system_packages.is_empty();
        let installer = Installer {
            platform: Platform::Linux,
            method,
            architectures: vec![],
            package_hints: vec![tool_id.to_string()],
            system_packages: system_packages.into_iter().map(str::to_string).collect(),
            executable: None,
            install_cmd: None,
            requires_confirm: false,
            verified_update: None,
        };

        let task =
            build_install_task(&tool, &installer, &InstallPolicy::default()).expect("task builds");
        for fragment in expected_fragments {
            assert!(
                task.command.contains(fragment),
                "{tool_id} command must contain {fragment}"
            );
        }
        assert_eq!(task.requires_privileges, requires_privileges);
    }
}
