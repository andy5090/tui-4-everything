use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::catalog::models::{InstallMethod, Installer, RiskLevel, Tool};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallTask {
    pub tool_id: String,
    pub method: InstallMethod,
    pub command: String,
    #[serde(default)]
    pub check_command: Option<String>,
    #[serde(default)]
    pub additional_check_commands: Vec<String>,
    #[serde(default)]
    pub install_timeout_sec: Option<u64>,
    #[serde(default)]
    pub requires_privileges: bool,
    pub requires_confirmation: bool,
    pub queued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallPolicy {
    pub enforce_script_confirmation: bool,
}

impl InstallTask {
    pub fn effective_timeout_sec(&self, configured_timeout_sec: u64) -> u64 {
        let method_default = if self.method == InstallMethod::Cargo {
            configured_timeout_sec.max(1_800)
        } else {
            configured_timeout_sec
        };
        self.install_timeout_sec
            .map_or(method_default, |timeout| method_default.max(timeout))
    }

    pub fn check_commands(&self) -> impl Iterator<Item = &str> {
        self.check_command
            .iter()
            .map(String::as_str)
            .chain(self.additional_check_commands.iter().map(String::as_str))
    }
}

impl Default for InstallPolicy {
    fn default() -> Self {
        Self {
            enforce_script_confirmation: true,
        }
    }
}

pub fn build_install_task(
    tool: &Tool,
    installer: &Installer,
    policy: &InstallPolicy,
) -> Result<InstallTask> {
    let command = materialize_command(installer)?;

    let requires_confirmation = match installer.method {
        InstallMethod::Script => {
            let _ = policy.enforce_script_confirmation;
            true
        }
        _ => installer.requires_confirm || tool.risk_level() == RiskLevel::Danger,
    };

    if matches!(installer.method, InstallMethod::Script) && !requires_confirmation {
        bail!("script installer for {} must require confirmation", tool.id);
    }

    let mut check_commands = tool.install_check_commands(installer.platform.clone());
    let check_command = (!check_commands.is_empty()).then(|| check_commands.remove(0));
    Ok(InstallTask {
        tool_id: tool.id.clone(),
        method: installer.method.clone(),
        command,
        check_command,
        additional_check_commands: check_commands,
        install_timeout_sec: tool.install_timeout_sec,
        requires_privileges: !installer.system_packages.is_empty(),
        requires_confirmation,
        queued_at: Utc::now(),
    })
}

fn materialize_command(installer: &Installer) -> Result<String> {
    if matches!(installer.method, InstallMethod::Script) {
        return installer
            .install_cmd
            .clone()
            .ok_or_else(|| anyhow::anyhow!("script installer requires explicit install_cmd"));
    }
    if installer.install_cmd.is_some() {
        bail!("install_cmd is only allowed for script installers");
    }

    if installer.package_hints.is_empty() {
        bail!("installer has no package hint");
    }
    for hint in &installer.package_hints {
        validate_package_hint(hint)?;
    }
    for package in &installer.system_packages {
        validate_package_hint(package)?;
    }
    let hint = &installer.package_hints[0];

    let command = match installer.method {
        InstallMethod::Brew => format!("brew install {}", hint),
        InstallMethod::BrewCask => format!("brew install --cask {}", hint),
        InstallMethod::Apt => format!(
            "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y {}",
            hint
        ),
        InstallMethod::Dnf => format!("sudo -n dnf install -y {}", hint),
        InstallMethod::Pacman => format!("sudo -n pacman -S --noconfirm {}", hint),
        InstallMethod::Snap => format!("sudo -n snap install {}", hint),
        InstallMethod::SnapClassic => format!("sudo -n snap install --classic {}", hint),
        InstallMethod::Pipx => format!(
            "command -v pipx >/dev/null 2>&1 || sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y pipx; pipx install {}",
            hint
        ),
        InstallMethod::NpmGlobal => format!("npm install -g {}", hint),
        InstallMethod::Cargo => format!(
            "cargo install --locked {}",
            installer.package_hints.join(" ")
        ),
        InstallMethod::Go => format!("go install {}", hint),
        InstallMethod::LazyVim => materialize_lazyvim_command(&installer.platform),
        InstallMethod::Tplay => materialize_tplay_command(&installer.platform),
        InstallMethod::YoutubeTui => materialize_youtube_tui_command(&installer.platform),
        InstallMethod::Yewtube => materialize_yewtube_command(&installer.platform),
        InstallMethod::AsciiCamera => materialize_ascii_camera_command(&installer.platform),
        InstallMethod::Newsboat => materialize_newsboat_command(&installer.platform),
        InstallMethod::Fastfetch => materialize_fastfetch_command(&installer.platform),
        InstallMethod::Script => unreachable!("script installers return before package handling"),
        InstallMethod::Other => {
            return Err(anyhow::anyhow!("unsupported install method"));
        }
    };

    if installer.system_packages.is_empty() {
        Ok(command)
    } else if installer.platform == crate::catalog::models::Platform::Linux {
        Ok(format!(
            "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y {} && {}",
            installer.system_packages.join(" "),
            command
        ))
    } else {
        bail!("system_packages are only supported by Linux installers")
    }
}

fn materialize_fastfetch_command(platform: &crate::catalog::models::Platform) -> String {
    match platform {
        crate::catalog::models::Platform::Macos => "brew install fastfetch".to_string(),
        crate::catalog::models::Platform::Linux => concat!(
            "case \"$(dpkg --print-architecture)\" in ",
            "amd64) asset=amd64 ;; arm64) asset=aarch64 ;; ",
            "*) echo 'fastfetch supports amd64 and arm64 in T4E' >&2; exit 1 ;; esac && ",
            "package=\"$(mktemp --suffix=.deb)\" && trap 'rm -f \"$package\"' EXIT && ",
            "curl -fL --retry 3 -o \"$package\" ",
            "\"https://github.com/fastfetch-cli/fastfetch/releases/latest/download/",
            "fastfetch-linux-${asset}.deb\" && ",
            "sudo -n env DEBIAN_FRONTEND=noninteractive apt-get ",
            "-o DPkg::Lock::Timeout=300 install -y \"$package\""
        )
        .to_string(),
    }
}

fn materialize_newsboat_command(platform: &crate::catalog::models::Platform) -> String {
    let (install, data_dir) = match platform {
        crate::catalog::models::Platform::Linux => (
            "sudo -n snap install newsboat",
            "$HOME/snap/newsboat/common/t4e",
        ),
        crate::catalog::models::Platform::Macos => (
            "brew install newsboat",
            "${XDG_DATA_HOME:-$HOME/.local/share}/t4e/newsboat",
        ),
    };
    format!(
        "{install} && mkdir -p \"$HOME/.local/bin\" && printf '%s\\n' '#!/bin/sh' 'data_dir=\"{data_dir}\"' 'urls=\"$data_dir/urls\"' 'mkdir -p \"$data_dir\"' 'while [ ! -s \"$urls\" ]; do' '  printf \"Newsboat needs at least one RSS or Atom feed.\\nFeed URL (Ctrl+C to cancel): \"' '  IFS= read -r feed || exit 0' '  [ -n \"$feed\" ] || continue' '  printf \"%s\\n\" \"$feed\" > \"$urls\"' 'done' 'exec newsboat -u \"$urls\" \"$@\"' > \"$HOME/.local/bin/t4e-newsboat\" && chmod +x \"$HOME/.local/bin/t4e-newsboat\""
    )
}

fn materialize_tplay_command(platform: &crate::catalog::models::Platform) -> String {
    match platform {
        crate::catalog::models::Platform::Linux => {
            "cargo install --locked tplay && data_dir=\"${XDG_DATA_HOME:-$HOME/.local/share}/t4e/tplay\" && python3 -m venv \"$data_dir/yt-dlp\" && \"$data_dir/yt-dlp/bin/python\" -m pip install --upgrade 'yt-dlp[default]' && mkdir -p \"$HOME/.local/bin\" && printf '%s\\n' '#!/bin/sh' 'data_dir=\"${XDG_DATA_HOME:-$HOME/.local/share}/t4e/tplay\"' 'exec env PATH=\"$data_dir/yt-dlp/bin:$PATH\" tplay \"$@\"' > \"$HOME/.local/bin/t4e-tplay\" && chmod +x \"$HOME/.local/bin/t4e-tplay\"".to_string()
        }
        crate::catalog::models::Platform::Macos => "cargo install --locked tplay".to_string(),
    }
}

fn materialize_youtube_tui_command(platform: &crate::catalog::models::Platform) -> String {
    let install = match platform {
        crate::catalog::models::Platform::Linux => concat!(
            "cargo install --locked youtube-tui && ",
            "data_dir=\"${XDG_DATA_HOME:-$HOME/.local/share}/t4e/youtube-tui\" && ",
            "python3 -m venv \"$data_dir/yt-dlp\" && ",
            "\"$data_dir/yt-dlp/bin/python\" -m pip install --upgrade 'yt-dlp[default]'"
        ),
        crate::catalog::models::Platform::Macos => {
            "brew install mpv yt-dlp && cargo install --locked youtube-tui"
        }
    };
    let launcher = concat!(
        "data_dir=\"${XDG_DATA_HOME:-$HOME/.local/share}/t4e/youtube-tui\" && ",
        "player_dir=\"$data_dir/player\" && mkdir -p \"$player_dir\" \"$HOME/.local/bin\" && ",
        "printf '%s\\n' '#!/bin/sh' ",
        "'exec env PATH=\"$T4E_PLAYER_PATH\" \"$HOME/.local/bin/t4e-mpv-terminal\" \"$@\"' ",
        "> \"$player_dir/mpv\" && chmod +x \"$player_dir/mpv\" && ",
        "printf '%s\\n' '#!/bin/sh' ",
        "'renderer=mpv' ",
        "'while [ \"$#\" -gt 0 ]; do' ",
        "'  case \"$1\" in' ",
        "'    --renderer) renderer=\"${2:-}\"; shift 2 ;;' ",
        "'    --renderer=*) renderer=\"${1#*=}\"; shift ;;' ",
        "'    *) break ;;' ",
        "'  esac' ",
        "'done' ",
        "'renderer=\"$(printf \"%s\" \"$renderer\" | tr \"[:upper:]\" \"[:lower:]\")\"' ",
        "'case \"$renderer\" in mpv|tct|caca) ;; *) printf \"Unsupported video renderer: %s\\n\" \"$renderer\" >&2; exit 2 ;; esac' ",
        "'data_dir=\"${XDG_DATA_HOME:-$HOME/.local/share}/t4e/youtube-tui\"' ",
        "'real_path=\"$PATH\"' ",
        "'host_pid=\"$$\"' ",
        "'exec env T4E_MPV_RENDERER=\"$renderer\" T4E_PLAYER_PATH=\"$data_dir/yt-dlp/bin:$real_path\" T4E_VIDEO_HOST=youtube-tui T4E_VIDEO_HOST_PID=\"$host_pid\" PATH=\"$data_dir/player:$data_dir/yt-dlp/bin:$real_path\" youtube-tui \"$@\"' ",
        "> \"$HOME/.local/bin/t4e-youtube-tui-v2\" && ",
        "chmod +x \"$HOME/.local/bin/t4e-youtube-tui-v2\" && ",
        "ln -sf t4e-youtube-tui-v2 \"$HOME/.local/bin/t4e-youtube-tui\""
    );
    [
        install,
        " && ",
        managed_mpv_player_install_command(),
        " && ",
        launcher,
    ]
    .concat()
}

fn materialize_yewtube_command(platform: &crate::catalog::models::Platform) -> String {
    let install = match platform {
        crate::catalog::models::Platform::Linux => concat!(
            "command -v pipx >/dev/null 2>&1 || sudo -n env DEBIAN_FRONTEND=noninteractive ",
            "apt-get -o DPkg::Lock::Timeout=300 install -y pipx; ",
            "pipx install --force yewtube"
        ),
        crate::catalog::models::Platform::Macos => "brew install yewtube mpv",
    };
    let launcher = concat!(
        "mkdir -p \"$HOME/.local/bin\" && ",
        "printf '%s\\n' '#!/bin/sh' ",
        "'renderer=mpv' ",
        "'while [ \"$#\" -gt 0 ]; do' ",
        "'  case \"$1\" in' ",
        "'    --renderer) renderer=\"${2:-}\"; shift 2 ;;' ",
        "'    --renderer=*) renderer=\"${1#*=}\"; shift ;;' ",
        "'    *) break ;;' ",
        "'  esac' ",
        "'done' ",
        "'renderer=\"$(printf \"%s\" \"$renderer\" | tr \"[:upper:]\" \"[:lower:]\")\"' ",
        "'case \"$renderer\" in mpv|tct|caca) ;; *) printf \"Unsupported video renderer: %s\\n\" \"$renderer\" >&2; exit 2 ;; esac' ",
        "'config_dir=\"${XDG_CONFIG_HOME:-$HOME/.config}/mps-youtube\"' ",
        "'config=\"$config_dir/config.json\"' ",
        "'player=\"$HOME/.local/bin/t4e-mpv-terminal\"' ",
        "'mkdir -p \"$config_dir\"' ",
        "'python3 -c \"import json,os,sys; p=sys.argv[1]; d=json.load(open(p)) if os.path.exists(p) else {}; d[\\\"PLAYER\\\"]=sys.argv[2]; q=p+\\\".tmp\\\"; json.dump(d,open(q,\\\"w\\\"),indent=2); os.replace(q,p)\" \"$config\" \"$player\"' ",
        "'exec env T4E_MPV_RENDERER=\"$renderer\" yt \"$@\"' ",
        "> \"$HOME/.local/bin/t4e-yewtube\" && ",
        "chmod +x \"$HOME/.local/bin/t4e-yewtube\""
    );
    [
        install,
        " && ",
        managed_mpv_player_install_command(),
        " && ",
        launcher,
    ]
    .concat()
}

fn managed_mpv_player_install_command() -> &'static str {
    concat!(
        "mkdir -p \"$HOME/.local/bin\" && ",
        "printf '%s\\n' '#!/bin/sh' ",
        "'renderer=\"${T4E_MPV_RENDERER:-mpv}\"' ",
        "'play_terminal() {' ",
        "'  host_pid=\"${T4E_VIDEO_HOST_PID:-}\"' ",
        "'  case \"$host_pid\" in \"\"|*[!0-9]*) host_pid= ;; esac' ",
        "'  if [ \"${T4E_VIDEO_HOST:-}\" = youtube-tui ] && [ -n \"$host_pid\" ] && [ \"$host_pid\" != \"$$\" ] && [ -w /dev/tty ]; then' ",
        "'    cleanup() { kill -CONT \"$host_pid\" 2>/dev/null || true; printf \"\\033[2J\\033[H\" > /dev/tty; }' ",
        "'    kill -STOP \"$host_pid\" 2>/dev/null || true' ",
        "'    trap cleanup EXIT HUP INT TERM' ",
        "'    mpv \"$@\" < /dev/tty > /dev/tty 2>&1' ",
        "'    status=$?' ",
        "'    trap - EXIT HUP INT TERM' ",
        "'    cleanup' ",
        "'    exit \"$status\"' ",
        "'  fi' ",
        "'  exec mpv \"$@\"' ",
        "'}' ",
        "'supports_renderer() { mpv --vo=help 2>/dev/null | grep -Eq \"^[[:space:]]*$1[[:space:]]\"; }' ",
        "'case \"$renderer\" in' ",
        "'  mpv) exec mpv \"$@\" ;;' ",
        "'  tct) if supports_renderer tct; then play_terminal --vo=tct --profile=sw-fast --really-quiet \"$@\"; else exec mpv \"$@\"; fi ;;' ",
        "'  caca) if supports_renderer caca; then play_terminal --vo=caca --really-quiet \"$@\"; else exec mpv \"$@\"; fi ;;' ",
        "'  *) printf \"Unsupported video renderer: %s\\n\" \"$renderer\" >&2; exit 2 ;;' ",
        "'esac' ",
        "> \"$HOME/.local/bin/t4e-mpv-terminal\" && ",
        "chmod +x \"$HOME/.local/bin/t4e-mpv-terminal\""
    )
}

fn materialize_ascii_camera_command(platform: &crate::catalog::models::Platform) -> String {
    let install = match platform {
        crate::catalog::models::Platform::Linux => "",
        crate::catalog::models::Platform::Macos => "brew install mpv && ",
    };
    format!(
        "{install}mkdir -p \"$HOME/.local/bin\" && printf '%s\\n' \
         '#!/bin/sh' \
         'device=0' \
         'renderer=tct' \
         'mirror=0' \
         'lavf_options=' \
         'while [ \"$#\" -gt 0 ]; do' \
         '  case \"$1\" in' \
         '    --device) device=\"${{2:-0}}\"; shift 2 ;;' \
         '    --vo) renderer=\"${{2:-tct}}\"; shift 2 ;;' \
         '    --vf=hflip) mirror=1; shift ;;' \
         '    --demuxer-lavf-o) lavf_options=\"${{2:-}}\"; shift 2 ;;' \
         '    *) printf \"Unknown ASCII Camera option: %s\\n\" \"$1\" >&2; exit 2 ;;' \
         '  esac' \
         'done' \
         'case \"$(uname -s)\" in' \
         '  Darwin) input=\"av://avfoundation:${{device}}:none\" ;;' \
         '  *) input=\"av://v4l2:/dev/video${{device}}\" ;;' \
         'esac' \
         'set --' \
         'if [ \"$mirror\" -eq 1 ]; then set -- \"$@\" \"--vf=hflip\"; fi' \
         'if [ -n \"$lavf_options\" ]; then set -- \"$@\" \"--demuxer-lavf-o=$lavf_options\"; fi' \
         'exec mpv --profile=low-latency --no-audio --untimed \"--vo=$renderer\" \"$@\" \"$input\"' \
         > \"$HOME/.local/bin/t4e-ascii-camera\" && \
         chmod +x \"$HOME/.local/bin/t4e-ascii-camera\" && \
         ln -sf t4e-ascii-camera \"$HOME/.local/bin/t4e-ascii-camera-v2\""
    )
}

fn materialize_lazyvim_command(platform: &crate::catalog::models::Platform) -> String {
    let dependencies = match platform {
        crate::catalog::models::Platform::Linux => {
            "sudo -n snap install nvim --classic && sudo -n env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y git ripgrep fd-find gcc"
        }
        crate::catalog::models::Platform::Macos => "brew install neovim git ripgrep fd",
    };
    format!(
        "{dependencies} && config_dir=\"${{XDG_CONFIG_HOME:-$HOME/.config}}/t4e-lazyvim\" && if [ ! -f \"$config_dir/init.lua\" ]; then if [ -e \"$config_dir\" ]; then echo 'T4E LazyVim config path already exists' >&2; exit 1; fi; git clone --filter=blob:none https://github.com/LazyVim/starter \"$config_dir\" && rm -rf \"$config_dir/.git\"; fi && mkdir -p \"$HOME/.local/bin\" && printf '%s\\n' '#!/bin/sh' 'exec env NVIM_APPNAME=t4e-lazyvim nvim \"$@\"' > \"$HOME/.local/bin/t4e-lazyvim\" && chmod +x \"$HOME/.local/bin/t4e-lazyvim\""
    )
}

fn validate_package_hint(hint: &str) -> Result<()> {
    let is_valid = hint
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-._@+/".contains(ch));
    if !is_valid {
        bail!("unsafe package hint: {}", hint);
    }
    Ok(())
}
