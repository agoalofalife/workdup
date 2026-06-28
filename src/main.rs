mod cleanup;
mod cli;
mod db;
mod http;
mod logging;
mod scanner;
mod temporal;
mod tokenizer;

use crate::cli::Cli;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use std::thread;
use tokio::runtime::Builder;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

fn main() -> Result<()> {
    let db_path: &'static str = "./data/workdup.db";

    logging::init_logging();
    dotenvy::dotenv().ok();

    db::init_schema(db_path)?;

    let cli = Cli::parse();
    let namespace = cli.namespace.context("TEMPORAL_NAMESPACE was not set")?;
    let token = CancellationToken::new();

    let scanner = spawn_worker("scanner", {
        let (ns, path, tok, query) = (namespace.clone(), db_path, token.clone(), cli.query.clone());
        move || scanner::run(ns, path, tok, cli.scan_interval, query)
    });

    let cleanup = spawn_worker("cleanup", {
        let (ns, path, tok) = (namespace.clone(), db_path, token.clone());
        move || cleanup::run(ns, path, tok, cli.cleanup_interval)
    });

    let http = spawn_worker("http", {
        let (path, token) = (db_path.to_string(), token.clone());

        move || http::run(path, format!("0.0.0.0:{}", cli.port).to_string(), token)
    });

    Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let _ = tokio::signal::ctrl_c().await;
            info!("shutdown requested");
        });

    token.cancel();

    scanner.join().ok();
    cleanup.join().ok();
    http.join().ok();

    info!("all workers stopped");

    Ok(())
}

/// Spawn an OS thread that runs `fut_fn` to completion on its own current-thread runtime
fn spawn_worker<F, Fut>(name: &'static str, fut_fn: F) -> thread::JoinHandle<()>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<()>>,
{
    thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let rt = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("start async worker runtime");

            if let Err(e) = rt.block_on(fut_fn()) {
                error!(worker = name, error = %e, "worker exited with error");
            }
        })
        .expect("spawn worker thread")
}
