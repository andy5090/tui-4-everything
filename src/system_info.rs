use std::fs;
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemOverview {
    pub source: &'static str,
    pub logo: Vec<String>,
    pub lines: Vec<String>,
}

pub fn cached_system_overview() -> SystemOverview {
    static OVERVIEW: OnceLock<SystemOverview> = OnceLock::new();
    OVERVIEW.get_or_init(detect_system_overview).clone()
}

fn detect_system_overview() -> SystemOverview {
    let logo = fastfetch_lines(
        &["--logo", "small", "--structure", "none", "--pipe", "false"],
        false,
    )
    .unwrap_or_default();
    if let Some(lines) = fastfetch_lines(
        &[
            "--logo",
            "none",
            "--structure",
            "OS:Host:Kernel:Uptime:CPU:Memory",
            "--pipe",
            "false",
        ],
        true,
    ) {
        return SystemOverview {
            source: "fastfetch",
            logo,
            lines,
        };
    }

    SystemOverview {
        source: "system",
        logo: Vec::new(),
        lines: fallback_lines(),
    }
}

fn fastfetch_lines(args: &[&str], trim: bool) -> Option<Vec<String>> {
    let output = Command::new("fastfetch").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            if trim {
                line.trim().to_string()
            } else {
                line.trim_end().to_string()
            }
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    (!lines.is_empty()).then_some(lines)
}

fn fallback_lines() -> Vec<String> {
    vec![
        format!("OS: {}", os_name()),
        format!(
            "Host: {}",
            command_output("hostname", &[]).unwrap_or_else(|| "Unknown".into())
        ),
        format!(
            "Kernel: {}",
            command_output("uname", &["-sr"]).unwrap_or_else(|| "Unknown".into())
        ),
        format!("Uptime: {}", uptime()),
        format!("CPU: {}", cpu()),
        format!("Memory: {}", memory()),
    ]
}

fn os_name() -> String {
    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("PRETTY_NAME=")
                    .map(|value| value.trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn uptime() -> String {
    let seconds = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|contents| contents.split_whitespace().next()?.parse::<f64>().ok())
        .map_or(0, |seconds| seconds as u64);
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else {
        format!("{hours}h {minutes}m")
    }
}

fn cpu() -> String {
    let model = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.trim() == "model name")
                    .map(|(_, value)| value.trim().to_string())
            })
        })
        .unwrap_or_else(|| std::env::consts::ARCH.to_string());
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    format!("{model} ({cores})")
}

fn memory() -> String {
    let contents = match fs::read_to_string("/proc/meminfo") {
        Ok(contents) => contents,
        Err(_) => return "Unknown".to_string(),
    };
    let value = |key: &str| {
        contents.lines().find_map(|line| {
            line.strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
    };
    let Some(total_kib) = value("MemTotal:") else {
        return "Unknown".to_string();
    };
    let available_kib = value("MemAvailable:").unwrap_or_default();
    let used_kib = total_kib.saturating_sub(available_kib);
    format!(
        "{:.1} / {:.1} GiB",
        used_kib as f64 / 1_048_576.0,
        total_kib as f64 / 1_048_576.0
    )
}

#[cfg(test)]
mod tests {
    use super::{cpu, fallback_lines, memory, uptime};

    #[test]
    fn fallback_has_the_same_compact_information_shape_as_fastfetch() {
        let lines = fallback_lines();
        assert_eq!(lines.len(), 6);
        for label in ["OS:", "Host:", "Kernel:", "Uptime:", "CPU:", "Memory:"] {
            assert!(lines.iter().any(|line| line.starts_with(label)));
        }
        assert!(!uptime().is_empty());
        assert!(!cpu().is_empty());
        assert!(!memory().is_empty());
    }
}
