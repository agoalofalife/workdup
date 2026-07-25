mod cleanup;
mod cli;
mod config;
mod db;
mod http;
mod logging;
mod scanner;
mod temporal;
mod tokenizer;

use crate::{
    cli::{Cli, Cmd},
    config::validate,
    temporal::temporal_meter,
};
use anyhow::Result;
use clap::Parser;
use std::thread;
use tokio::runtime::Builder;
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info};

fn main() -> Result<()> {
    let db_path: &'static str = "./data/workdup.db";

    dotenvy::dotenv().ok(); // env first of all
    logging::init_logging();

    let cli = Cli::parse();
    let cfg = config::load(&cli.config)?;
    let namespaces = cfg.resolve_namespace_section();

    match cli.cmd {
        Cmd::Validate => {
            validate(&namespaces)?;
            println!("ok: {} namespace(s)", namespaces.len());
            Ok(())
        }
        Cmd::Run => {
            db::init_schema(db_path)?;

            let token = CancellationToken::new();

            // The SDK Prometheus exporter binds a listener and `tokio::spawn`s its HTTP
            // server, so it must be created inside a runtime context — and that runtime
            // must outlive the whole run to keep serving scrapes on :9464. The per-worker
            // runtimes each fully occupy their own thread with a single `block_on`, so we
            // host the exporter on this dedicated main runtime instead.
            let rt = Builder::new_current_thread().enable_all().build()?;

            let metrics_addr: std::net::SocketAddr =
                format!("0.0.0.0:{}", cfg.http.temporal_metrics_port).parse()?;

            let meter = rt.block_on(async move { temporal_meter(metrics_addr) })?;

            let mut workers = vec![];

            for ns in &namespaces {
                workers.push(spawn_worker("scanner", {
                    tracing::info_span!("scanner", %ns.name);

                    let (ns, db_path, tok, meter) =
                        (ns.clone(), db_path, token.clone(), meter.clone());
                    let span = tracing::info_span!("scanner", ns = %ns.name);

                    move || scanner::run(ns, db_path, tok, meter).instrument(span)
                }));

                workers.push(spawn_worker("cleanup", {
                    let (ns, db_path, tok, meter) =
                        (ns.clone(), db_path, token.clone(), meter.clone());
                    let span = tracing::info_span!("scanner", ns = %ns.name);

                    move || cleanup::run(ns, db_path, tok, meter).instrument(span)
                }));
            }

            let http = spawn_worker("http", {
                let (path, token, meter) = (db_path.to_string(), token.clone(), meter.clone());

                move || {
                    http::run(
                        path,
                        format!("0.0.0.0:{}", cfg.http.port).to_string(),
                        token,
                        namespaces,
                        meter,
                    )
                }
            });

            // Drives the shutdown wait and, concurrently, the exporter's server task for
            // the whole lifetime of the process.
            rt.block_on(shutdown_signal());

            token.cancel();

            for w in workers {
                w.join().ok();
            }
            http.join().ok();

            info!("all workers stopped");

            Ok(())
        }
    }
}

/// Resolves on SIGINT or SIGTERM.
///
/// Kubernetes sends SIGTERM on pod termination and only escalates to SIGKILL
/// once the grace period expires — waiting on Ctrl-C alone means the token is
/// never cancelled and every rollout ends in a hard kill.
async fn shutdown_signal() {
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            // Losing SIGTERM shouldn't stop the daemon from starting; degrade
            // to SIGINT-only rather than refusing to run.
            error!(error = %e, "could not install SIGTERM handler, watching SIGINT only");
            let _ = tokio::signal::ctrl_c().await;
            info!(signal = "SIGINT", "shutdown requested");
            return;
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!(signal = "SIGINT", "shutdown requested"),
        _ = sigterm.recv()          => info!(signal = "SIGTERM", "shutdown requested"),
    }
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
