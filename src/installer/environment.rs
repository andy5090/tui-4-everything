use std::collections::BTreeSet;
use std::env;
use std::path::PathBuf;

use crate::catalog::models::{Architecture, InstallMethod, Installer, Platform, Tool};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallEnvironment {
    pub platform: Platform,
    pub architecture: String,
    commands: BTreeSet<String>,
}

impl InstallEnvironment {
    pub fn detect() -> Self {
        let commands = [
            "awk",
            "apt-get",
            "brew",
            "cargo",
            "curl",
            "dnf",
            "go",
            "ldd",
            "npm",
            "pacman",
            "pipx",
            "python",
            "pkg",
            "sha256sum",
            "shasum",
            "snap",
            "tar",
            "uname",
            "xbps-install",
        ]
        .into_iter()
        .filter(|command| command_exists(command))
        .map(str::to_string)
        .collect();
        Self {
            platform: Platform::current(),
            architecture: env::consts::ARCH.to_string(),
            commands,
        }
    }

    pub fn with_commands<I, S>(
        platform: Platform,
        architecture: impl Into<String>,
        commands: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            platform,
            architecture: architecture.into(),
            commands: commands.into_iter().map(Into::into).collect(),
        }
    }

    pub fn supports(&self, tool: &Tool) -> bool {
        if !self.is_runtime_compatible(tool) {
            return false;
        }
        let Some(installer) =
            tool.installer_for_target(self.platform, Architecture::from_name(&self.architecture))
        else {
            return false;
        };
        if installer.method == InstallMethod::Builtin {
            return true;
        }
        match self.platform {
            Platform::Macos => self.supports_macos(installer),
            Platform::Termux => self.supports_termux(installer),
            Platform::Linux if self.has("apt-get") => self.supports_linux_method(tool, installer),
            Platform::Linux if self.has("xbps-install") => self.supports_xbps(tool, installer),
            Platform::Linux => self.supports_linux_method(tool, installer),
        }
    }

    pub fn installer_for(&self, tool: &Tool) -> Option<Installer> {
        if !self.is_runtime_compatible(tool) {
            return None;
        }
        let installer = tool
            .installer_for_target(self.platform, Architecture::from_name(&self.architecture))?
            .clone();
        if installer.method == InstallMethod::Builtin {
            return Some(installer);
        }

        match self.platform {
            Platform::Macos => self.supports_macos(&installer).then_some(installer),
            Platform::Termux => self.supports_termux(&installer).then_some(installer),
            Platform::Linux if self.has("apt-get") => self
                .supports_linux_method(tool, &installer)
                .then_some(installer),
            Platform::Linux if self.has("xbps-install") => self.xbps_installer(tool, installer),
            Platform::Linux => self
                .supports_linux_method(tool, &installer)
                .then_some(installer),
        }
    }

    fn xbps_installer(&self, tool: &Tool, mut installer: Installer) -> Option<Installer> {
        if tool.id == "ascii-camera" && installer.method == InstallMethod::AsciiCamera {
            // The composite installer still needs to create T4E's launcher after
            // XBPS installs its runtime dependencies, so keep the specialized
            // method. An empty
            // system_packages list distinguishes this normalized XBPS recipe
            // from the catalog's APT recipe.
            installer.package_hints = vec!["mpv".into(), "ffmpeg".into(), "libcaca".into()];
            installer.system_packages.clear();
            installer.install_cmd = None;
            installer.verified_update = None;
            return Some(installer);
        }

        if let Some(port) = xbps_port(&tool.id) {
            installer.method = InstallMethod::Xbps;
            installer.package_hints = port.packages.iter().map(|value| (*value).into()).collect();
            installer.system_packages.clear();
            installer.install_cmd = None;
            installer.verified_update = None;
            if let Some(executable) = port.executable {
                installer.executable = Some(executable.into());
            }
            return Some(installer);
        }

        self.supports_xbps(tool, &installer).then_some(installer)
    }

    fn supports_xbps(&self, tool: &Tool, installer: &Installer) -> bool {
        if tool.id == "ascii-camera" && installer.method == InstallMethod::AsciiCamera {
            return true;
        }
        if xbps_port(&tool.id).is_some() {
            return true;
        }
        match installer.method {
            InstallMethod::Cargo => installer.system_packages.is_empty() && self.has("cargo"),
            InstallMethod::Script => self.supports_linux_script(tool),
            InstallMethod::NpmGlobal => !is_32_bit_x86(&self.architecture) && self.has("npm"),
            _ => false,
        }
    }

    fn supports_linux_method(&self, tool: &Tool, installer: &Installer) -> bool {
        if !installer.system_packages.is_empty() && !self.has("apt-get") {
            return false;
        }
        match installer.method {
            InstallMethod::Apt => self.has("apt-get"),
            InstallMethod::Pkg => false,
            InstallMethod::Dnf => self.has("dnf"),
            InstallMethod::Pacman => self.has("pacman"),
            InstallMethod::Xbps => self.has("xbps-install"),
            InstallMethod::Snap | InstallMethod::SnapClassic => self.has("snap"),
            InstallMethod::Cargo
            | InstallMethod::Termleaf
            | InstallMethod::Yazi
            | InstallMethod::Spotatui => self.has("cargo"),
            InstallMethod::NpmGlobal => !is_32_bit_x86(&self.architecture) && self.has("npm"),
            InstallMethod::Pipx => self.has("pipx") || self.has("apt-get"),
            InstallMethod::Pip => false,
            InstallMethod::Go => self.has("go"),
            InstallMethod::Script => self.supports_linux_script(tool),
            InstallMethod::LazyVim => self.has("apt-get") && self.has("snap"),
            InstallMethod::Tplay | InstallMethod::YoutubeTui => {
                self.has("apt-get") && self.has("cargo")
            }
            InstallMethod::Yewtube | InstallMethod::AsciiCamera => self.has("apt-get"),
            InstallMethod::Newsboat => self.has("snap"),
            InstallMethod::Fastfetch => {
                self.has("apt-get")
                    && self.has("curl")
                    && matches!(self.architecture.as_str(), "x86_64" | "aarch64")
            }
            InstallMethod::Glow | InstallMethod::Asciiquarium => {
                self.has("apt-get") && self.has("curl")
            }
            InstallMethod::Builtin => true,
            InstallMethod::Brew | InstallMethod::BrewCask | InstallMethod::Other => false,
        }
    }

    fn supports_macos(&self, installer: &Installer) -> bool {
        match installer.method {
            InstallMethod::Brew | InstallMethod::BrewCask => self.has("brew"),
            InstallMethod::Cargo
            | InstallMethod::Termleaf
            | InstallMethod::Yazi
            | InstallMethod::Spotatui => self.has("cargo"),
            InstallMethod::NpmGlobal => self.has("npm"),
            InstallMethod::Go => self.has("go"),
            InstallMethod::Script => self.has("curl"),
            InstallMethod::Builtin => true,
            _ => true,
        }
    }

    fn supports_termux(&self, installer: &Installer) -> bool {
        if installer.platform != Platform::Termux && installer.method != InstallMethod::Builtin {
            return false;
        }
        match installer.method {
            InstallMethod::Pkg
            | InstallMethod::Pip
            | InstallMethod::AsciiCamera
            | InstallMethod::Newsboat
            | InstallMethod::Fastfetch
            | InstallMethod::LazyVim => self.has("pkg"),
            InstallMethod::Cargo | InstallMethod::Termleaf => self.has("cargo"),
            InstallMethod::NpmGlobal => self.has("npm"),
            InstallMethod::Go => self.has("go"),
            InstallMethod::Builtin => true,
            _ => false,
        }
    }

    fn has(&self, command: &str) -> bool {
        self.commands.contains(command)
    }

    pub fn is_runtime_compatible(&self, tool: &Tool) -> bool {
        !(self.platform == Platform::Termux && tool.id == "btop")
    }

    fn supports_linux_script(&self, tool: &Tool) -> bool {
        if tool.id == "termleaf" {
            return ["awk", "curl", "ldd", "tar", "uname"]
                .into_iter()
                .all(|command| self.has(command))
                && (self.has("sha256sum") || self.has("shasum"));
        }
        !is_32_bit_x86(&self.architecture) && self.has("curl")
    }
}

struct XbpsPort {
    packages: &'static [&'static str],
    executable: Option<&'static str>,
}

fn xbps_port(tool_id: &str) -> Option<XbpsPort> {
    let (packages, executable): (&[&str], Option<&str>) = match tool_id {
        "asciiquarium" => (&["asciiquarium"], None),
        "bastet" => (&["bastet"], None),
        "bat" => (&["bat"], Some("bat")),
        "btop" => (&["btop"], None),
        "cava" => (&["cava"], None),
        "cmatrix" => (&["cmatrix"], None),
        "cowsay" => (&["cowsay"], None),
        "dust" => (&["dust"], None),
        "fastfetch" => (&["fastfetch"], None),
        "fd" => (&["fd"], Some("fd")),
        "figlet" => (&["figlet"], None),
        "fortune" => (&["fortune-mod"], None),
        "fzf" => (&["fzf"], None),
        "glow" => (&["glow"], None),
        "helix" => (&["helix"], None),
        "lynx" => (&["lynx"], None),
        "lolcat" => (&["lolcat-c"], Some("lolcat")),
        "micro" => (&["micro"], None),
        "mpv" => (&["mpv"], None),
        "ncdu" => (&["ncdu"], None),
        "newsboat" => (&["newsboat"], Some("newsboat")),
        "nudoku" => (&["nudoku"], None),
        "ripgrep" => (&["ripgrep"], None),
        "sl" => (&["sl"], None),
        "tty-clock" => (&["tty-clock"], None),
        "visidata" => (&["visidata"], None),
        "yazi" => (&["yazi"], None),
        "zoxide" => (&["zoxide"], None),
        _ => return None,
    };
    Some(XbpsPort {
        packages,
        executable,
    })
}

fn is_32_bit_x86(architecture: &str) -> bool {
    matches!(architecture, "x86" | "i386" | "i486" | "i586" | "i686")
}

fn command_exists(command: &str) -> bool {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|path| env::split_paths(&path).collect::<Vec<PathBuf>>())
        .map(|directory| directory.join(command))
        .any(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::InstallEnvironment;
    use crate::catalog::loader::load_catalog;
    use crate::catalog::models::{InstallMethod, Platform};
    use crate::installer::engine::{InstallPolicy, build_install_task};
    use std::path::Path;

    #[test]
    fn void_uses_verified_xbps_ports_and_hides_ubuntu_only_apps() {
        let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
        let environment =
            InstallEnvironment::with_commands(Platform::Linux, "i686", ["cargo", "xbps-install"]);

        let cmatrix = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "cmatrix")
            .expect("cmatrix");
        let installer = environment.installer_for(cmatrix).expect("xbps port");
        assert_eq!(installer.method, InstallMethod::Xbps);
        assert_eq!(installer.package_hints, ["cmatrix"]);
        let task =
            build_install_task(cmatrix, &installer, &InstallPolicy::default()).expect("xbps task");
        assert_eq!(task.command, "sudo -n xbps-install -Sy cmatrix");

        let lolcat = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "lolcat")
            .expect("lolcat");
        let installer = environment.installer_for(lolcat).expect("lolcat XBPS port");
        assert_eq!(installer.method, InstallMethod::Xbps);
        assert_eq!(installer.package_hints, ["lolcat-c"]);
        assert_eq!(installer.executable.as_deref(), Some("lolcat"));
        let task =
            build_install_task(lolcat, &installer, &InstallPolicy::default()).expect("lolcat task");
        assert_eq!(task.command, "sudo -n xbps-install -Sy lolcat-c");
        assert_eq!(task.check_command.as_deref(), Some("lolcat"));

        let ascii_camera = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "ascii-camera")
            .expect("ascii-camera");
        let installer = environment
            .installer_for(ascii_camera)
            .expect("ascii-camera XBPS port");
        assert_eq!(installer.method, InstallMethod::AsciiCamera);
        assert!(installer.system_packages.is_empty());
        let task = build_install_task(ascii_camera, &installer, &InstallPolicy::default())
            .expect("ascii-camera task");
        assert!(
            task.command
                .starts_with("missing_packages=; for package in mpv ffmpeg libcaca; do ")
        );
        assert!(task.command.contains("xbps-query \"$package\""));
        assert!(task.command.contains(
            "if [ -n \"$missing_packages\" ]; then sudo -n xbps-install -Sy $missing_packages"
        ));
        assert!(task.command.contains("t4e-ascii-camera"));
        assert!(task.requires_privileges);

        let youtube = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "youtube-tui")
            .expect("youtube-tui");
        assert!(!environment.supports(youtube));
    }

    #[test]
    fn termux_uses_only_explicit_native_installers_without_sudo() {
        let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
        let environment =
            InstallEnvironment::with_commands(Platform::Termux, "aarch64", ["cargo", "npm", "pkg"]);
        let supported = catalog
            .tools
            .iter()
            .filter_map(|tool| {
                environment
                    .installer_for(tool)
                    .map(|installer| (tool, installer))
            })
            .collect::<Vec<_>>();

        assert_eq!(supported.len(), 46);
        for (tool, installer) in supported {
            assert!(
                installer.platform == Platform::Termux
                    || installer.method == InstallMethod::Builtin,
                "{} fell back to a non-Termux installer",
                tool.id
            );
            let task = build_install_task(tool, &installer, &InstallPolicy::default())
                .expect("Termux task builds");
            assert!(!task.command.contains("sudo"), "{} uses sudo", tool.id);
            assert!(
                !task.command.contains("apt-get") && !task.command.contains("snap install"),
                "{} uses a Linux-only package manager",
                tool.id
            );
            assert!(!task.requires_privileges, "{} requests sudo", tool.id);
        }

        let spotatui = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "spotatui")
            .expect("spotatui");
        assert!(!environment.supports(spotatui));

        let btop = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "btop")
            .expect("btop");
        assert!(!environment.is_runtime_compatible(btop));
        assert!(environment.installer_for(btop).is_none());

        let yt_dlp = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "yt-dlp")
            .expect("yt-dlp");
        let installer = environment.installer_for(yt_dlp).expect("yt-dlp installer");
        assert_eq!(installer.method, InstallMethod::Pip);
        let task = build_install_task(yt_dlp, &installer, &InstallPolicy::default())
            .expect("yt-dlp task builds");
        assert!(task.command.contains("pkg install -y python"));
        assert!(
            task.command
                .contains("python -m pip install --upgrade yt-dlp")
        );

        let termleaf = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "termleaf")
            .expect("termleaf");
        let installer = environment
            .installer_for(termleaf)
            .expect("Termleaf is supported on Termux");
        assert_eq!(installer.method, InstallMethod::Termleaf);
        let task = build_install_task(termleaf, &installer, &InstallPolicy::default())
            .expect("Termleaf task builds");
        assert_eq!(
            task.command,
            "cargo install --locked --git https://github.com/andy5090/termleaf --tag v0.3.5 termleaf"
        );
        assert!(task.requires_confirmation);
        assert!(!task.requires_privileges);
    }

    #[test]
    fn i686_hides_unverified_script_installers() {
        let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
        let environment = InstallEnvironment::with_commands(
            Platform::Linux,
            "x86",
            [
                "awk",
                "curl",
                "ldd",
                "sha256sum",
                "tar",
                "uname",
                "xbps-install",
            ],
        );
        let claude = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "claude-code")
            .expect("claude");
        assert!(!environment.supports(claude));

        let termleaf = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "termleaf")
            .expect("termleaf");
        assert!(environment.supports(termleaf));
    }
}
