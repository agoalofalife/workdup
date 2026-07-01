use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "workdup", about = "Temporal workflow history deduplication")]
pub struct Cli {
    #[arg(long, env = "WORKDUP_CONFIG", default_value = "workdup.toml")]
    pub config: PathBuf,
    #[arg(long)] // временный override, выигрывает над файлом
    pub log_level: Option<String>,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// run demon
    Run,
    /// Check config file (CI / init-container)
    Validate,
}
