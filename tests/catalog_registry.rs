use std::path::Path;

use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::catalog::models::{Exposure, InstallMethod, Platform, Risk};
use t4e::catalog::validator::{validate_catalog, validate_workspaces};

#[test]
fn registry_loads_and_validates() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    validate_catalog(&catalog).expect("catalog validates");

    assert!(
        catalog.tools.len() >= 40,
        "expected at least 40 starter tools"
    );
    assert!(
        catalog.packs.len() >= 6,
        "expected starter packs and optional packs"
    );

    let agent_tools: Vec<_> = catalog
        .tools
        .iter()
        .filter(|tool| matches!(tool.risk, Risk::High))
        .collect();
    assert_eq!(agent_tools.len(), 3, "exactly three high-risk agent tools");
    assert!(
        agent_tools
            .iter()
            .all(|tool| matches!(tool.exposure, Exposure::SearchOnly))
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
    assert!(yewtube.installers.iter().any(|installer| {
        installer.platform == Platform::Linux && installer.method == InstallMethod::Pipx
    }));
    assert_eq!(yewtube.checks[0].which.as_deref(), Some("yt"));

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
