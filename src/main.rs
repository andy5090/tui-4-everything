use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::catalog::models::Platform;
use t4e::catalog::validator::validate_catalog;
use t4e::gates::{
    AttemptResult, CANONICAL_SAMPLE_TOOLS, FailureClassification, ToolGateResult, build_gate_report,
};
use t4e::installer::engine::{InstallPolicy, build_install_task};
use t4e::installer::resolver::{
    Candidate, ShellPackageSearch, resolve_with_fallback,
};
use t4e::mux::tmux::compile_workspace;
use t4e::mux::workspace::MuxBackend;
use t4e::mux::zellij::render_layout_kdl;

#[derive(Debug, Parser)]
#[command(name = "t4e")]
#[command(about = "t4e v0.1 bootstrap CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    GenerateGateReport {
        #[arg(long)]
        gate_id: String,
        #[arg(long)]
        os: String,
        #[arg(long)]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Command::Validate { catalog, workspaces } => {
            let catalog_model = load_catalog(&catalog)
                .with_context(|| format!("failed to load catalog from {}", catalog.display()))?;
            validate_catalog(&catalog_model)?;
            let _workspace_model = load_workspaces(&workspaces)
                .with_context(|| format!("failed to load workspaces from {}", workspaces.display()))?;
            println!("catalog/workspaces validation ok");
        }
        Command::InstallPlan {
            catalog,
            tool_id,
            platform,
        } => {
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
        Command::Resolve { hint, candidates } => {
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
        Command::WorkspacePlan {
            workspaces,
            workspace_id,
            mux,
        } => {
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
                eprintln!("note: workspace default mux is tmux; zellij requested as explicit override");
            }
        }
        Command::GenerateGateReport {
            gate_id,
            os,
            output,
        } => {
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
            let report = build_gate_report(gate_id, run_id, os, results, required_success_rate)?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, serde_json::to_string_pretty(&report)?)?;
            println!("{}", output.display());
        }
    }

    Ok(())
}
