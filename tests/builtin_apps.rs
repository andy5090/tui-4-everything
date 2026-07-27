use std::path::Path;

use t4e::catalog::loader::load_catalog;
use t4e::catalog::models::{Exposure, InstallMethod, Platform, RiskLevel};
use t4e::catalog::validator::validate_catalog;
use t4e::installer::checks::{InstallChecker, SystemInstallChecker};
use t4e::installer::engine::{InstallPolicy, build_install_task};

fn big_clock() -> t4e::catalog::models::Tool {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    validate_catalog(&catalog).expect("catalog validates");
    catalog
        .tools
        .iter()
        .find(|tool| tool.id == "big-clock")
        .expect("big-clock exists")
        .clone()
}

#[test]
fn big_clock_is_a_safe_launchable_builtin_app() {
    let tool = big_clock();
    assert!(tool.is_launchable_app());
    assert!(tool.is_builtin());
    assert_eq!(tool.risk_level(), RiskLevel::Safe);
    assert!(matches!(tool.exposure, Exposure::Labs));
    assert!(
        tool.installers
            .iter()
            .all(|installer| installer.method == InstallMethod::Builtin)
    );
    assert!(tool.run_options.iter().any(|option| option.flag == "-C"));
}

#[test]
fn builtin_run_command_targets_the_current_executable() {
    let tool = big_clock();
    for platform in [Platform::Linux, Platform::Macos] {
        let command = tool.run_command_for(platform);
        assert!(command.ends_with(" builtin big-clock"), "{command}");
        let executable = command.split_whitespace().next().expect("executable");
        let check = SystemInstallChecker.check(executable).expect("check runs");
        assert!(
            check.installed,
            "builtin executable must resolve: {executable}"
        );
    }
}

#[test]
fn builtin_install_plan_is_a_confirmed_no_op() {
    let tool = big_clock();
    for platform in [Platform::Linux, Platform::Macos] {
        let installer = tool
            .installers
            .iter()
            .find(|installer| installer.platform == platform)
            .expect("installer exists");
        let task = build_install_task(&tool, installer, &InstallPolicy::default())
            .expect("install task builds");
        assert_eq!(task.method, InstallMethod::Builtin);
        assert_eq!(task.command, "true");
        assert!(!task.requires_confirmation);
        assert!(!task.requires_privileges);
        let check_command = task.check_command.expect("check command exists");
        let check = SystemInstallChecker
            .check(&check_command)
            .expect("check runs");
        assert!(check.installed, "builtin app is always installed");
    }
}
