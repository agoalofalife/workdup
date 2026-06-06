use clap::Parser;

#[derive(Parser)]
#[command(name = "workdup", about = "Temporal workflow history deduplication")]
pub struct Cli {
    /// Temporal namespace (falls back to env var)
    #[arg(long, env = "TEMPORAL_NAMESPACE")]
    pub namespace: Option<String>,
}
