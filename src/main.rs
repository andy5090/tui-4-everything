use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use t4e::app::state::AppState;
use t4e::app::terminal::run as run_terminal_app;
use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::catalog::models::{Exposure, Platform};
use t4e::catalog::validator::{validate_catalog, validate_workspaces};
use t4e::gates::{
    AttemptResult, CANONICAL_SAMPLE_TOOLS, EvidenceKind, FailureClassification, GateProvenance,
    RuntimeCheckEvidence, ToolGateResult, build_gate_report, build_runtime_gate_report,
    runtime_gate_check_ids,
};
use t4e::installer::engine::{InstallPolicy, build_install_task};
use t4e::installer::execution::{
    CommandRunner, ExecutionPolicy, InstallExecutor, InstallJob, SystemCommandRunner,
};
use t4e::installer::resolver::{Candidate, ShellPackageSearch, resolve_with_fallback};
use t4e::mux::tmux::compile_workspace;
use t4e::mux::workspace::MuxBackend;
use t4e::mux::zellij::render_layout_kdl;
use t4e::storage::{default_state_path, log_dir_for_state};

#[derive(Debug, Parser)]
#[command(name = "t4e")]
#[command(about = "Curated terminal apps and workspace dashboard")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Tui {
        #[arg(long, default_value = "registry/catalog.yaml")]
        catalog: PathBuf,
        #[arg(long, default_value = "registry/workspaces.yaml")]
        workspaces: PathBuf,
    },
    Validate {
        #[arg(long, default_value = "registry/catalog.yaml")]
        catalog: PathBuf,
        #[arg(long, default_value = "registry/workspaces.yaml")]
        workspaces: PathBuf,
    },
    InstallPlan {
        #[arg(long, default_value = "registry/catalog.yaml")]
        catalog: PathBuf,
        #[arg(long)]
        tool_id: String,
        #[arg(long, default_value = "macos")]
        platform: String,
    },
    CatalogPlans {
        #[arg(long, default_value = "registry/catalog.yaml")]
        catalog: PathBuf,
        #[arg(long, default_value = "linux")]
        platform: String,
        #[arg(long, default_value = "all", value_parser = ["all", "starter", "labs"])]
        exposure: String,
    },
    Install {
        #[arg(long, default_value = "registry/catalog.yaml")]
        catalog: PathBuf,
        #[arg(long)]
        tool_id: String,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long, default_value_t = 600)]
        timeout_sec: u64,
        #[arg(long, default_value_t = 2)]
        attempts: u32,
    },
    Resolve {
        #[arg(long)]
        hint: String,
        #[arg(long)]
        candidates: Vec<String>,
    },
    WorkspacePlan {
        #[arg(long, default_value = "registry/workspaces.yaml")]
        workspaces: PathBuf,
        #[arg(long)]
        workspace_id: String,
        #[arg(long, default_value = "tmux")]
        mux: String,
    },
    GenerateContractGateReport {
        #[arg(long)]
        gate_id: String,
        #[arg(long)]
        os: String,
        #[arg(long)]
        output: PathBuf,
    },
    BuildRealGateReport {
        #[arg(long)]
        gate_id: String,
        #[arg(long)]
        os: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    BuildRuntimeGateReport {
        #[arg(long, value_parser = ["gate3", "gate4", "gate5"])]
        gate_id: String,
        #[arg(long)]
        os: String,
        #[arg(long, value_name = "CHECK_ID=PATH")]
        evidence: Vec<String>,
        #[arg(long)]
        output: PathBuf,
    },
    McpServer {
        #[arg(long, default_value = "registry/catalog.yaml")]
        catalog: PathBuf,
        #[arg(long, default_value = "registry/workspaces.yaml")]
        workspaces: PathBuf,
    },
    CollectRealGateResults {
        #[arg(long, value_parser = ["brew", "apt"])]
        manager: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        None => run_tui(
            PathBuf::from("registry/catalog.yaml"),
            PathBuf::from("registry/workspaces.yaml"),
        )?,
        Some(Command::Tui {
            catalog,
            workspaces,
        }) => run_tui(catalog, workspaces)?,
        Some(Command::Validate {
            catalog,
            workspaces,
        }) => {
            let catalog_model = load_catalog(&catalog)
                .with_context(|| format!("failed to load catalog from {}", catalog.display()))?;
            validate_catalog(&catalog_model)?;
            let workspace_model = load_workspaces(&workspaces).with_context(|| {
                format!("failed to load workspaces from {}", workspaces.display())
            })?;
            validate_workspaces(&catalog_model, &workspace_model)?;
            println!("catalog/workspaces validation ok");
        }
        Some(Command::InstallPlan {
            catalog,
            tool_id,
            platform,
        }) => {
            let catalog_model = load_catalog(&catalog)
                .with_context(|| format!("failed to load catalog from {}", catalog.display()))?;
            validate_catalog(&catalog_model)?;

            let target_platform = match platform.as_str() {
                "macos" => Platform::Macos,
                "linux" => Platform::Linux,
                other => anyhow::bail!("unsupported platform: {}", other),
            };

            let tool = catalog_model
                .tools
                .iter()
                .find(|tool| tool.id == tool_id)
                .with_context(|| format!("tool not found: {}", tool_id))?;

            let installer = tool
                .installers
                .iter()
                .find(|installer| installer.platform == target_platform)
                .with_context(|| format!("installer not found for platform {}", platform))?;

            let task = build_install_task(tool, installer, &InstallPolicy::default())?;
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
        Some(Command::CatalogPlans {
            catalog,
            platform,
            exposure,
        }) => {
            let catalog_model = load_catalog(&catalog)
                .with_context(|| format!("failed to load catalog from {}", catalog.display()))?;
            validate_catalog(&catalog_model)?;
            let target_platform = parse_platform(&platform)?;
            let mut plans = Vec::new();
            for tool in &catalog_model.tools {
                let exposure_name = match tool.exposure {
                    Exposure::Starter => "starter",
                    Exposure::Labs => "labs",
                };
                if exposure != "all" && exposure != exposure_name {
                    continue;
                }
                let Some(installer) = tool
                    .installers
                    .iter()
                    .find(|installer| installer.platform == target_platform)
                else {
                    plans.push(serde_json::json!({
                        "tool_id": tool.id,
                        "name": tool.name,
                        "exposure": exposure_name,
                        "launchable_app": tool.is_launchable_app(),
                        "supported": false,
                        "error": format!("no {platform} installer")
                    }));
                    continue;
                };
                match build_install_task(tool, installer, &InstallPolicy::default()) {
                    Ok(task) => plans.push(serde_json::json!({
                        "tool_id": tool.id,
                        "name": tool.name,
                        "exposure": exposure_name,
                        "launchable_app": tool.is_launchable_app(),
                        "supported": true,
                        "package_hint": installer.package_hints.first(),
                        "method": task.method,
                        "command": task.command,
                        "check_command": task.check_command,
                        "requires_confirmation": task.requires_confirmation
                    })),
                    Err(error) => plans.push(serde_json::json!({
                        "tool_id": tool.id,
                        "name": tool.name,
                        "exposure": exposure_name,
                        "launchable_app": tool.is_launchable_app(),
                        "supported": false,
                        "error": error.to_string()
                    })),
                }
            }
            println!("{}", serde_json::to_string_pretty(&plans)?);
        }
        Some(Command::Install {
            catalog,
            tool_id,
            platform,
            yes,
            timeout_sec,
            attempts,
        }) => {
            let catalog_model = load_catalog(&catalog)
                .with_context(|| format!("failed to load catalog from {}", catalog.display()))?;
            validate_catalog(&catalog_model)?;
            let platform_name = platform.unwrap_or_else(current_platform_name);
            let target_platform = parse_platform(&platform_name)?;
            let tool = catalog_model
                .tools
                .iter()
                .find(|tool| tool.id == tool_id)
                .with_context(|| format!("tool not found: {}", tool_id))?;
            let installer = tool
                .installers
                .iter()
                .find(|installer| installer.platform == target_platform)
                .with_context(|| format!("installer not found for platform {}", platform_name))?;
            let task = build_install_task(tool, installer, &InstallPolicy::default())?;
            if !yes {
                anyhow::bail!(
                    "refusing to execute without --yes; planned command: {}",
                    task.command
                );
            }

            let state_path = default_state_path();
            let policy = ExecutionPolicy {
                timeout: Duration::from_secs(timeout_sec.max(1)),
                max_attempts: attempts.max(1),
                log_dir: log_dir_for_state(&state_path),
            };
            let channel = task.method.channel_name().to_string();
            let executor = InstallExecutor::new(SystemCommandRunner, policy);
            let completed = executor.execute(
                InstallJob::new(task, channel),
                Arc::new(AtomicBool::new(false)),
                |chunk| eprint!("{}", chunk.text),
            );
            println!("{}", serde_json::to_string_pretty(&completed)?);
            if completed.item.state != t4e::installer::queue::QueueState::Success {
                anyhow::bail!("installation failed for {}", completed.item.tool_id);
            }
        }
        Some(Command::Resolve { hint, candidates }) => {
            let method = t4e::catalog::models::InstallMethod::Apt;
            let candidates = candidates
                .into_iter()
                .map(|package| Candidate {
                    package,
                    method: method.clone(),
                })
                .collect::<Vec<_>>();
            let ranked = resolve_with_fallback(&hint, method, &candidates, &ShellPackageSearch)?;
            println!("{}", serde_json::to_string_pretty(&ranked)?);
        }
        Some(Command::WorkspacePlan {
            workspaces,
            workspace_id,
            mux,
        }) => {
            let workspaces_model = load_workspaces(&workspaces)?;
            let workspace = workspaces_model
                .workspaces
                .iter()
                .find(|workspace| workspace.id == workspace_id)
                .with_context(|| format!("workspace not found: {}", workspace_id))?;

            match mux.as_str() {
                "tmux" => {
                    let session_name = workspace
                        .session_name
                        .clone()
                        .unwrap_or_else(|| format!("t4e-{}", workspace.id));
                    let output = compile_workspace(workspace, &session_name, "main")?;
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                "zellij" => {
                    let layout = render_layout_kdl(workspace)?;
                    println!("{}", layout);
                }
                other => anyhow::bail!("unsupported mux target {}", other),
            }

            if matches!(workspace.mux, MuxBackend::Tmux) && mux == "zellij" {
                eprintln!(
                    "note: workspace default mux is tmux; zellij requested as explicit override"
                );
            }
        }
        Some(Command::GenerateContractGateReport {
            gate_id,
            os,
            output,
        }) => {
            let required_success_rate = match gate_id.as_str() {
                "gate1" => 0.90,
                "gate2" => 0.60,
                other => anyhow::bail!("unsupported gate_id {}", other),
            };

            let result_budget = if gate_id == "gate1" { 9 } else { 6 };
            let results = CANONICAL_SAMPLE_TOOLS
                .iter()
                .enumerate()
                .map(|(idx, tool)| {
                    let attempts = if idx < result_budget {
                        vec![AttemptResult {
                            attempt: 1,
                            exit_code: 0,
                            duration_ms: 1000,
                            stderr_summary: String::new(),
                            classification: FailureClassification::None,
                        }]
                    } else {
                        vec![AttemptResult {
                            attempt: 1,
                            exit_code: 1,
                            duration_ms: 1000,
                            stderr_summary: "mock failure".to_string(),
                            classification: FailureClassification::Product,
                        }]
                    };
                    ToolGateResult {
                        tool_id: (*tool).to_string(),
                        manager: if os.contains("macos") {
                            "brew".to_string()
                        } else {
                            "apt".to_string()
                        },
                        attempt_count: attempts.len() as u8,
                        final_status: if idx < result_budget {
                            "success".to_string()
                        } else {
                            "failed".to_string()
                        },
                        failure_classification: if idx < result_budget {
                            FailureClassification::None
                        } else {
                            FailureClassification::Product
                        },
                        attempts,
                    }
                })
                .collect::<Vec<_>>();

            let run_id = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let report = build_gate_report(
                gate_id,
                run_id,
                os,
                results,
                required_success_rate,
                EvidenceKind::Contract,
                GateProvenance {
                    result_source: "synthetic-contract-fixture".to_string(),
                    result_sha256: "not-applicable".to_string(),
                    generated_at: chrono::Utc::now().to_rfc3339(),
                },
            )?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, serde_json::to_string_pretty(&report)?)?;
            println!("{}", output.display());
        }
        Some(Command::BuildRealGateReport {
            gate_id,
            os,
            input,
            output,
        }) => {
            let required_success_rate = gate_threshold(&gate_id)?;
            let bytes = fs::read(&input)
                .with_context(|| format!("failed to read real gate input {}", input.display()))?;
            let results = serde_json::from_slice::<Vec<ToolGateResult>>(&bytes)
                .with_context(|| format!("invalid real gate input {}", input.display()))?;
            use sha2::{Digest, Sha256};
            let input_hash = format!("{:x}", Sha256::digest(&bytes));
            let report = build_gate_report(
                gate_id.clone(),
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
                os,
                results,
                required_success_rate,
                EvidenceKind::Real,
                GateProvenance {
                    result_source: input.display().to_string(),
                    result_sha256: input_hash,
                    generated_at: chrono::Utc::now().to_rfc3339(),
                },
            )?;
            if report.summary.status != "pass" {
                anyhow::bail!("{} real gate did not meet its success threshold", gate_id);
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, serde_json::to_string_pretty(&report)?)?;
            println!("{}", output.display());
        }
        Some(Command::BuildRuntimeGateReport {
            gate_id,
            os,
            evidence,
            output,
        }) => {
            use sha2::{Digest, Sha256};

            let expected = runtime_gate_check_ids(&gate_id)?;
            let mut checks = Vec::new();
            let mut combined = Sha256::new();
            for value in evidence {
                let (check_id, path) = value.split_once('=').with_context(|| {
                    format!("invalid evidence {value:?}; expected CHECK_ID=PATH")
                })?;
                if !expected.contains(&check_id) {
                    anyhow::bail!("unexpected check {check_id} for {gate_id}");
                }
                let path = PathBuf::from(path);
                let bytes = fs::read(&path)
                    .with_context(|| format!("failed to read evidence {}", path.display()))?;
                if bytes.is_empty() {
                    anyhow::bail!("evidence is empty: {}", path.display());
                }
                let test_log = String::from_utf8_lossy(&bytes);
                if !test_log.contains("test result: ok.")
                    || test_log.contains("test result: FAILED")
                {
                    anyhow::bail!(
                        "evidence does not contain a successful Cargo test result: {}",
                        path.display()
                    );
                }
                let digest = format!("{:x}", Sha256::digest(&bytes));
                combined.update(check_id.as_bytes());
                combined.update(digest.as_bytes());
                checks.push(RuntimeCheckEvidence {
                    check_id: check_id.to_string(),
                    command: runtime_gate_command(&gate_id, check_id)?.to_string(),
                    status: "pass".to_string(),
                    result_source: path.display().to_string(),
                    result_sha256: digest,
                });
            }
            let report = build_runtime_gate_report(
                &gate_id,
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
                os,
                checks,
                GateProvenance {
                    result_source: "direct-runtime-check-logs".to_string(),
                    result_sha256: format!("{:x}", combined.finalize()),
                    generated_at: chrono::Utc::now().to_rfc3339(),
                },
            )?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
            println!("{}", output.display());
        }
        Some(Command::McpServer {
            catalog,
            workspaces,
        }) => {
            let catalog_model = load_catalog(&catalog)?;
            validate_catalog(&catalog_model)?;
            let workspace_model = load_workspaces(&workspaces)?;
            validate_workspaces(&catalog_model, &workspace_model)?;
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            t4e::mcp::run_server(
                std::io::BufReader::new(stdin.lock()),
                stdout.lock(),
                &catalog_model,
                &workspace_model,
            )?;
        }
        Some(Command::CollectRealGateResults {
            manager,
            output,
            yes,
        }) => {
            if !yes {
                anyhow::bail!("real installation collection requires --yes");
            }
            if manager == "apt" {
                let status = std::process::Command::new("sudo")
                    .args(["apt-get", "update"])
                    .status()
                    .context("failed to run apt-get update")?;
                if !status.success() {
                    anyhow::bail!("apt-get update failed with {status}");
                }
            }
            let cancel = AtomicBool::new(false);
            let runner = SystemCommandRunner;
            let mut results = Vec::new();
            for tool_id in CANONICAL_SAMPLE_TOOLS {
                let package = gate_package(&manager, tool_id);
                let command = if manager == "brew" {
                    format!("brew install {package}")
                } else {
                    format!(
                        "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=300 install -y {package}"
                    )
                };
                let mut attempts = Vec::new();
                for attempt in 1..=2_u8 {
                    let output =
                        runner.run(&command, Duration::from_secs(600), &cancel, &mut |_| {})?;
                    let classification = classify_gate_failure(&output);
                    attempts.push(AttemptResult {
                        attempt,
                        exit_code: output.exit_code.unwrap_or(-1),
                        duration_ms: output.duration_ms,
                        stderr_summary: output.stderr.chars().take(1000).collect(),
                        classification,
                    });
                    if output.exit_code == Some(0) && !output.timed_out {
                        break;
                    }
                }
                let succeeded = attempts
                    .last()
                    .is_some_and(|attempt| attempt.exit_code == 0);
                let failure_classification = if succeeded {
                    FailureClassification::None
                } else {
                    attempts
                        .last()
                        .map(|attempt| attempt.classification.clone())
                        .unwrap_or(FailureClassification::Product)
                };
                results.push(ToolGateResult {
                    tool_id: tool_id.to_string(),
                    manager: manager.clone(),
                    attempt_count: attempts.len() as u8,
                    final_status: if succeeded { "success" } else { "failed" }.to_string(),
                    failure_classification,
                    attempts,
                });
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, serde_json::to_vec_pretty(&results)?)?;
            println!("{}", output.display());
        }
    }

    Ok(())
}

fn run_tui(catalog: PathBuf, workspaces: PathBuf) -> Result<()> {
    let catalog_model = load_catalog(&catalog)
        .with_context(|| format!("failed to load catalog from {}", catalog.display()))?;
    validate_catalog(&catalog_model)?;
    let workspace_model = load_workspaces(&workspaces)
        .with_context(|| format!("failed to load workspaces from {}", workspaces.display()))?;
    validate_workspaces(&catalog_model, &workspace_model)?;
    let app = AppState::persistent(catalog_model, workspace_model, default_state_path())?;
    run_terminal_app(app)
}

fn current_platform_name() -> String {
    if cfg!(target_os = "macos") {
        "macos".to_string()
    } else {
        "linux".to_string()
    }
}

fn parse_platform(value: &str) -> Result<Platform> {
    match value {
        "macos" => Ok(Platform::Macos),
        "linux" => Ok(Platform::Linux),
        other => anyhow::bail!("unsupported platform: {}", other),
    }
}

fn gate_threshold(gate_id: &str) -> Result<f64> {
    match gate_id {
        "gate1" => Ok(0.90),
        "gate2" => Ok(0.60),
        other => anyhow::bail!("unsupported gate_id {}", other),
    }
}

fn runtime_gate_command(gate_id: &str, check_id: &str) -> Result<&'static str> {
    match (gate_id, check_id) {
        ("gate3", "tmux-live-repro") => Ok(
            "cargo test --test tmux_runtime three_registry_tmux_layouts_relaunch_with_matching_live_snapshots -- --exact --nocapture",
        ),
        ("gate3", "workspace-canonical-hash") => Ok("cargo test --test workspace_repro"),
        ("gate4", "installer-execution") => Ok("cargo test --test install_execution"),
        ("gate4", "queue-retry-state") => Ok("cargo test --test queue_state"),
        ("gate5", "agent-policy") => Ok("cargo test --test contracts"),
        ("gate5", "install-confirmation") => Ok("cargo test --test installer_logic"),
        _ => anyhow::bail!("unknown runtime check {gate_id}/{check_id}"),
    }
}

fn gate_package<'a>(manager: &str, tool_id: &'a str) -> &'a str {
    match (manager, tool_id) {
        ("apt", "yt-dlp") => "yt-dlp",
        _ => tool_id,
    }
}

fn classify_gate_failure(
    output: &t4e::installer::execution::CommandOutput,
) -> FailureClassification {
    if output.exit_code == Some(0) {
        return FailureClassification::None;
    }
    let stderr = output.stderr.to_ascii_lowercase();
    if output.timed_out
        || [
            "network",
            "temporary failure",
            "could not resolve",
            "connection reset",
        ]
        .iter()
        .any(|needle| stderr.contains(needle))
    {
        FailureClassification::Infra
    } else {
        FailureClassification::Product
    }
}
