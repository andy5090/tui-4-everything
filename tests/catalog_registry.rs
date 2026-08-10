use std::collections::HashMap;
use std::path::Path;

use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::catalog::models::{
    AppCategory, Audience, Capability, CatalogRegistry, Check, Exposure, InstallMethod, Installer,
    OutputFilter, Platform, RiskLevel, RunSpec, Tool, ToolCategory, VerifiedUpdate, VersionProbe,
};
use t4e::catalog::validator::{validate_catalog, validate_workspaces};

#[test]
fn registry_loads_and_validates() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    validate_catalog(&catalog).expect("catalog validates");

    assert!(
        catalog.tools.len() >= 40,
        "expected at least 40 starter tools"
    );
    assert!(catalog.packs.len() >= 6, "expected starter packs");

    let agent_tools: Vec<_> = catalog
        .tools
        .iter()
        .filter(|tool| tool.category == ToolCategory::Agents)
        .collect();
    assert_eq!(agent_tools.len(), 3, "exactly three agent tools");
    assert!(agent_tools.iter().all(|tool| {
        matches!(tool.exposure, Exposure::Starter)
            && tool.risk_level() == RiskLevel::Danger
            && tool.capabilities.contains(&Capability::Commands)
            && tool.capabilities.contains(&Capability::Autonomous)
    }));
    let missing_descriptions = catalog
        .tools
        .iter()
        .filter(|tool| {
            tool.is_launchable_app() && tool.description.as_deref().is_none_or(str::is_empty)
        })
        .map(|tool| tool.id.as_str())
        .collect::<Vec<_>>();
    assert!(
        missing_descriptions.is_empty(),
        "launchable apps need descriptions: {}",
        missing_descriptions.join(", ")
    );
    let asciiquarium = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "asciiquarium")
        .expect("asciiquarium exists");
    assert!(asciiquarium.installers.iter().any(|installer| {
        installer.platform == Platform::Linux && installer.method == InstallMethod::Snap
    }));

    let yewtube = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "yewtube")
        .expect("yewtube exists");
    assert_eq!(yewtube.run.cmd, "yt");
    assert_eq!(yewtube.run_command_for(Platform::Linux), "t4e-yewtube");
    assert!(yewtube.installers.iter().any(|installer| {
        installer.platform == Platform::Linux
            && installer.method == InstallMethod::Yewtube
            && installer.system_packages.contains(&"mpv".to_string())
    }));
    assert_eq!(yewtube.checks[0].which.as_deref(), Some("yt"));
    assert_eq!(yewtube.checks[1].which.as_deref(), Some("t4e-yewtube"));
    assert_eq!(yewtube.run_options[0].values, ["MPV", "TCT", "CACA"]);
    assert_eq!(yewtube.run_options[0].default_value.as_deref(), Some("MPV"));
    assert!(yewtube.run_options[0].default_enabled);
    assert_eq!(yewtube.run_command_for(Platform::Macos), "t4e-yewtube");
    assert!(!yewtube.key_hints.is_empty());

    let ascii_camera = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "ascii-camera")
        .expect("ASCII Camera exists");
    assert_eq!(ascii_camera.risk_level(), RiskLevel::High);
    assert!(
        ascii_camera
            .capabilities
            .contains(&Capability::CameraCapture)
    );
    assert_eq!(
        ascii_camera.run_command_for(Platform::Linux),
        "t4e-ascii-camera"
    );
    assert!(ascii_camera.installers.iter().any(|installer| {
        installer.platform == Platform::Linux
            && installer.method == InstallMethod::AsciiCamera
            && installer.system_packages == ["mpv"]
    }));
    assert!(ascii_camera.installers.iter().any(|installer| {
        installer.platform == Platform::Termux
            && installer.method == InstallMethod::AsciiCamera
            && installer.system_packages == ["termux-api", "chafa"]
    }));
    assert!(
        catalog
            .packs
            .iter()
            .find(|pack| pack.id == "video-pack")
            .expect("video pack exists")
            .tool_ids
            .contains(&ascii_camera.id)
    );

    let youtube_tui = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "youtube-tui")
        .expect("youtube-tui exists");
    assert_eq!(youtube_tui.run.cmd, "youtube-tui");
    assert_eq!(
        youtube_tui.run_command_for(Platform::Linux),
        "t4e-youtube-tui-v2"
    );
    assert_eq!(
        youtube_tui.run_command_for(Platform::Macos),
        "t4e-youtube-tui-v2"
    );
    assert_eq!(youtube_tui.run_options[0].values, ["MPV", "TCT", "CACA"]);
    assert_eq!(
        youtube_tui.run_options[0].default_value.as_deref(),
        Some("MPV")
    );
    assert!(youtube_tui.installers.iter().any(|installer| {
        installer.platform == Platform::Linux
            && installer.method == InstallMethod::YoutubeTui
            && installer
                .system_packages
                .contains(&"libmpv-dev".to_string())
            && installer
                .system_packages
                .contains(&"python3-venv".to_string())
            && !installer.system_packages.contains(&"yt-dlp".to_string())
    }));

    let tplay = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "tplay")
        .expect("tplay exists");
    assert_eq!(tplay.run.cmd, "tplay");
    assert_eq!(tplay.run_command_for(Platform::Linux), "t4e-tplay");
    assert!(tplay.launch_argument.is_some());
    assert!(tplay.installers.iter().any(|installer| {
        installer.platform == Platform::Linux
            && installer.method == InstallMethod::Tplay
            && installer
                .system_packages
                .contains(&"libavcodec-dev".to_string())
            && installer
                .system_packages
                .contains(&"python3-venv".to_string())
            && !installer.system_packages.contains(&"yt-dlp".to_string())
    }));

    let newsboat = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "newsboat")
        .expect("newsboat exists");
    assert_eq!(newsboat.run_command_for(Platform::Linux), "t4e-newsboat");
    assert!(newsboat.installers.iter().all(|installer| {
        installer.method == InstallMethod::Newsboat
            && installer.executable.as_deref() == Some("t4e-newsboat")
    }));

    let lynx = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "lynx")
        .expect("lynx exists");
    assert_eq!(lynx.run.cmd, "lynx");
    assert!(lynx.installers.iter().any(|installer| {
        installer.platform == Platform::Linux
            && installer.method == InstallMethod::Apt
            && installer.package_hints == ["lynx"]
    }));
    let btop = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "btop")
        .expect("btop exists");
    assert_eq!(btop.run.cmd, "btop");
    assert_eq!(btop.app_category(), AppCategory::System);
    assert_eq!(btop.risk_level(), RiskLevel::Danger);
    assert_eq!(btop.capabilities, [Capability::System]);
    assert!(btop.installers.iter().any(|installer| {
        installer.platform == Platform::Macos
            && installer.method == InstallMethod::Brew
            && installer.package_hints == ["btop"]
    }));
    assert!(btop.installers.iter().any(|installer| {
        installer.platform == Platform::Linux
            && installer.method == InstallMethod::Apt
            && installer.package_hints == ["btop"]
    }));
    assert!(catalog.tools.iter().all(|tool| tool.id != "neovim"));
    let lazyvim = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "lazyvim")
        .expect("lazyvim exists");
    assert_eq!(lazyvim.run.cmd, "t4e-lazyvim");
    assert!(
        lazyvim
            .installers
            .iter()
            .all(|installer| installer.method == InstallMethod::LazyVim)
    );
    let helix = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "helix")
        .expect("helix exists");
    assert_eq!(helix.category, ToolCategory::Ide);

    let termleaf = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "termleaf")
        .expect("termleaf exists");
    assert_eq!(termleaf.run.cmd, "termleaf");
    assert_eq!(termleaf.category, ToolCategory::Edit);
    assert_eq!(termleaf.risk_level(), RiskLevel::High);
    assert_eq!(
        termleaf
            .checks
            .iter()
            .filter_map(|check| check.which.as_deref())
            .collect::<Vec<_>>(),
        ["termleaf", "termleaf-update"]
    );
    assert!(termleaf.installers.iter().all(|installer| {
        installer.method == InstallMethod::Script
            && installer.requires_confirm
            && installer.install_cmd.as_deref().is_some_and(|command| {
                command.contains("andy5090/termleaf/releases/download/v0.3.5/")
                    && command.contains("termleaf-installer.sh")
                    && command.contains("TERMLEAF_VERSION=0.3.5")
            })
            && installer.verified_update.as_ref().is_some_and(|update| {
                update.version == "0.3.5"
                    && update.command.contains("/releases/download/v0.3.5/")
                    && update.command.contains("TERMLEAF_VERSION=0.3.5")
                    && update.evidence.ends_with("/releases/tag/v0.3.5")
            })
    }));
    assert!(
        catalog
            .packs
            .iter()
            .find(|pack| pack.id == "edit-pack")
            .expect("edit pack exists")
            .tool_ids
            .contains(&termleaf.id)
    );

    let pipes = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "pipes-sh")
        .expect("pipes-sh exists");
    let pipes_linux = pipes
        .installers
        .iter()
        .find(|installer| installer.platform == Platform::Linux)
        .expect("pipes-sh Linux installer exists");
    assert_eq!(pipes_linux.executable.as_deref(), Some("/usr/games/pipes"));
    assert_eq!(pipes.run_command_for(Platform::Linux), "/usr/games/pipes");
    assert_eq!(pipes.run_command_for(Platform::Macos), "pipes.sh");

    for id in ["yazi", "helix"] {
        let tool = catalog
            .tools
            .iter()
            .find(|tool| tool.id == id)
            .expect("classic snap tool exists");
        assert!(tool.installers.iter().any(|installer| {
            installer.platform == Platform::Linux && installer.method == InstallMethod::SnapClassic
        }));
    }
}

#[test]
fn every_home_application_has_one_non_empty_os_category() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let launchable_count = catalog
        .tools
        .iter()
        .filter(|tool| tool.is_launchable_app())
        .count();
    let categorized_count = AppCategory::ALL
        .iter()
        .map(|category| {
            let count = catalog
                .tools
                .iter()
                .filter(|tool| tool.is_launchable_app() && tool.app_category() == *category)
                .count();
            assert!(count > 0, "{} category is empty", category.label());
            count
        })
        .sum::<usize>();

    assert_eq!(categorized_count, launchable_count);
}

#[test]
fn launchable_apps_belong_to_exactly_one_pack() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let mut memberships = catalog
        .tools
        .iter()
        .filter(|tool| tool.is_launchable_app())
        .map(|tool| (tool.id.as_str(), Vec::<&str>::new()))
        .collect::<HashMap<_, _>>();

    for pack in &catalog.packs {
        for tool_id in &pack.tool_ids {
            let tool = catalog
                .tools
                .iter()
                .find(|tool| &tool.id == tool_id)
                .expect("pack tool exists");
            if tool.is_launchable_app() {
                memberships
                    .get_mut(tool.id.as_str())
                    .expect("launchable tool is indexed")
                    .push(pack.id.as_str());
            }
        }
    }

    let duplicates = memberships
        .iter()
        .filter(|(_, packs)| packs.len() != 1)
        .map(|(tool, packs)| format!("{tool}: {}", packs.join(", ")))
        .collect::<Vec<_>>();
    assert!(duplicates.is_empty(), "{}", duplicates.join("\n"));
}

#[test]
fn glow_and_read_only_helpers_belong_to_the_viewers_pack() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let viewers = catalog
        .packs
        .iter()
        .find(|pack| pack.id == "viewers-pack")
        .expect("viewers pack exists");
    assert_eq!(
        viewers.tool_ids,
        ["fastfetch", "btop", "glow", "bat", "less", "mediainfo"]
    );

    let podcasts = catalog
        .packs
        .iter()
        .find(|pack| pack.id == "podcasts-reading-pack")
        .expect("podcasts and news pack exists");
    assert_eq!(podcasts.title, "Podcasts & News Pack");
    assert!(!podcasts.tool_ids.contains(&"glow".to_string()));
}

#[test]
fn one_shot_fun_tools_have_visible_default_output() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let expected = [
        ("cowsay", "cowsay T4E"),
        ("fortune", "fortune"),
        ("figlet", "figlet"),
        ("fastfetch", "fastfetch"),
    ];

    for (tool_id, command) in expected {
        let tool = catalog
            .tools
            .iter()
            .find(|tool| tool.id == tool_id)
            .expect("one-shot tool exists");
        assert_eq!(tool.run.cmd, command);
        assert!(tool.run.keep_open, "{tool_id} output must remain visible");
    }

    let lolcat = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "lolcat")
        .expect("lolcat support tool exists");
    assert!(!lolcat.is_launchable_app());
    for tool_id in ["cowsay", "fortune", "figlet"] {
        let tool = catalog
            .tools
            .iter()
            .find(|tool| tool.id == tool_id)
            .expect("compatible app exists");
        assert!(tool.run_options.iter().any(|option| {
            option.id == "rainbow-output" && option.output_filter == Some(OutputFilter::Lolcat)
        }));
    }
    let figlet = catalog
        .tools
        .iter()
        .find(|tool| tool.id == "figlet")
        .expect("Figlet exists");
    assert!(figlet.launch_argument.is_some());
    assert_eq!(
        figlet.run_options[0].values,
        ["standard", "small", "slant", "big", "banner"]
    );
}

#[test]
fn workspaces_load_and_have_tmux_minimum() {
    let workspaces =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspace loads");
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    validate_workspaces(&catalog, &workspaces).expect("workspaces validate");
    let tmux_count = workspaces
        .workspaces
        .iter()
        .filter(|ws| matches!(ws.mux, t4e::mux::workspace::MuxBackend::Tmux))
        .count();

    assert!(tmux_count >= 3, "expected at least three tmux workspaces");
}

#[test]
fn workspace_validation_rejects_shell_operators_and_unapproved_executables() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let mut workspaces =
        load_workspaces(Path::new("registry/workspaces.yaml")).expect("workspace loads");
    workspaces.workspaces[0].layout.panes[0].cmd = "yewtube; touch /tmp/t4e-pwn".to_string();
    assert!(validate_workspaces(&catalog, &workspaces).is_err());

    workspaces.workspaces[0].layout.panes[0].cmd = "python malware.py".to_string();
    assert!(validate_workspaces(&catalog, &workspaces).is_err());
}

#[test]
fn catalog_validation_accepts_well_formed_verified_update_metadata() {
    let catalog = CatalogRegistry {
        packs: vec![],
        tools: vec![verified_update_tool(Some(VerifiedUpdate {
            version: "1.2.3".to_string(),
            version_probe: VersionProbe {
                executable: "ripgrep".to_string(),
                args: vec!["--version".to_string()],
            },
            command: "apt-get install -y ripgrep=1.2.3".to_string(),
            verified_at: "2026-07-30".to_string(),
            evidence: "https://example.com/ripgrep-1.2.3".to_string(),
        }))],
    };

    validate_catalog(&catalog).expect("catalog validates");
}

#[test]
fn catalog_validation_rejects_verified_update_without_exact_versioned_command() {
    let catalog = CatalogRegistry {
        packs: vec![],
        tools: vec![verified_update_tool(Some(VerifiedUpdate {
            version: "1.2.3".to_string(),
            version_probe: VersionProbe {
                executable: "ripgrep".to_string(),
                args: vec!["--version".to_string()],
            },
            command: "apt-get install -y ripgrep".to_string(),
            verified_at: "2026-07-30".to_string(),
            evidence: "https://example.com/ripgrep-1.2.3".to_string(),
        }))],
    };

    assert!(validate_catalog(&catalog).is_err());
}

#[test]
fn catalog_validation_rejects_verified_update_with_unsafe_probe_args() {
    let catalog = CatalogRegistry {
        packs: vec![],
        tools: vec![verified_update_tool(Some(VerifiedUpdate {
            version: "1.2.3".to_string(),
            version_probe: VersionProbe {
                executable: "ripgrep".to_string(),
                args: vec!["--version;rm".to_string()],
            },
            command: "apt-get install -y ripgrep=1.2.3".to_string(),
            verified_at: "2026-07-30".to_string(),
            evidence: "https://example.com/ripgrep-1.2.3".to_string(),
        }))],
    };

    assert!(validate_catalog(&catalog).is_err());
}

fn verified_update_tool(verified_update: Option<VerifiedUpdate>) -> Tool {
    Tool {
        id: "verified-ripgrep".to_string(),
        name: "Verified Ripgrep".to_string(),
        description: Some("Version-pinned ripgrep".to_string()),
        key_hints: vec![],
        install_timeout_sec: None,
        category: ToolCategory::Utility,
        tags: vec![],
        audience: Audience::Developer,
        capabilities: vec![],
        exposure: Exposure::Starter,
        run: RunSpec {
            cmd: "ripgrep".to_string(),
            keep_open: false,
        },
        launch_argument: None,
        run_options: vec![],
        installers: vec![
            Installer {
                platform: Platform::Linux,
                method: InstallMethod::Apt,
                package_hints: vec!["ripgrep".to_string()],
                system_packages: vec![],
                executable: Some("ripgrep".to_string()),
                install_cmd: None,
                requires_confirm: false,
                verified_update: verified_update.clone(),
            },
            Installer {
                platform: Platform::Macos,
                method: InstallMethod::Brew,
                package_hints: vec!["ripgrep".to_string()],
                system_packages: vec![],
                executable: Some("ripgrep".to_string()),
                install_cmd: None,
                requires_confirm: false,
                verified_update,
            },
        ],
        checks: vec![Check {
            which: Some("ripgrep".to_string()),
            version: None,
            custom: None,
        }],
        notes: None,
    }
}
