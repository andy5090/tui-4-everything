use std::path::Path;
use std::process::Command;

use t4e::catalog::loader::load_catalog;
use t4e::catalog::models::{InstallMethod, Platform};

#[test]
#[ignore = "live Ubuntu package-index gate; run with --ignored"]
fn ubuntu_install_sources_resolve_every_catalog_package() {
    let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
    let mut failures = Vec::new();

    for tool in &catalog.tools {
        let installer = tool
            .installers
            .iter()
            .find(|installer| installer.platform == Platform::Linux)
            .expect("Linux installer is required by catalog validation");
        for package in &installer.package_hints {
            let probe = match installer.method {
                InstallMethod::Apt => apt_has_candidate(package),
                InstallMethod::Snap | InstallMethod::SnapClassic => {
                    command_succeeds("snap", &["info", package])
                }
                InstallMethod::Cargo => cargo_package_exists(package),
                InstallMethod::Pipx => {
                    command_succeeds("python3", &["-m", "pip", "index", "versions", package])
                }
                InstallMethod::NpmGlobal => command_succeeds("npm", &["view", package, "version"]),
                InstallMethod::Script => installer
                    .install_cmd
                    .as_deref()
                    .is_some_and(|command| command.starts_with("curl -fsSL https://")),
                _ => true,
            };
            if !probe {
                failures.push(format!(
                    "{}: {:?} package {} did not resolve",
                    tool.id, installer.method, package
                ));
            }
        }
        for package in &installer.system_packages {
            if !apt_has_candidate(package) {
                failures.push(format!(
                    "{}: system package {} has no Ubuntu candidate",
                    tool.id, package
                ));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn apt_has_candidate(package: &str) -> bool {
    Command::new("apt-cache")
        .args(["policy", package])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    line.trim_start()
                        .strip_prefix("Candidate:")
                        .is_some_and(|value| value.trim() != "(none)")
                })
        })
}

fn cargo_package_exists(package: &str) -> bool {
    Command::new("cargo")
        .args(["search", "--limit", "1", package])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line.starts_with(&format!("{package} =")))
        })
}
