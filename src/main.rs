use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use t4e::catalog::loader::{load_catalog, load_workspaces};
use t4e::catalog::validator::validate_catalog;

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
    }

    Ok(())
}
