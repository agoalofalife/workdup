use anyhow::Result;
use futures_util::StreamExt;
use rusqlite::{Connection, OptionalExtension};
use temporalio_client::{Client, WorkflowListOptions};
use temporalio_common::telemetry::metrics::TemporalMeter;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{config::ResolvedNamespace, db, temporal, tokenizer};

pub async fn run(
    cfg: ResolvedNamespace,
    db_path: &str,
    token: CancellationToken,
    meter: TemporalMeter,
) -> Result<()> {
    let mut temp_client = temporal::connect(&cfg, meter).await?;
    let db_conn = db::open(db_path)?;

    let mut ticker = interval(cfg.scan_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay); // will rescheduled when finished + timeout time

    loop {
        tokio::select! {
            _ = token.cancelled() => { info!("scanner stopping"); break; }
            _ = ticker.tick() => {
                let start_time = std::time::Instant::now();
                let result = scan(&mut temp_client, &cfg.name, &db_conn, &token, &cfg.query).await;

                metrics::gauge!("scan_tick_duration_seconds", "namespace" => cfg.name.clone())
                    .set(start_time.elapsed().as_secs_f64());

                metrics::counter!(
                    "scan_ticks_total",
                    "namespace" => cfg.name.clone(),
                    "result" => if result.is_ok() { "ok" } else { "error" },
                )
                .increment(1);

                if let Err(e) = result {
                    error!(error = %e, "scan tick failed");
                }
            }
        }
    }
    Ok(())
}

async fn scan(
    temp_client: &mut Client,
    namespace: &str,
    db_conn: &Connection,
    token: &CancellationToken,
    query: &str,
) -> Result<()> {
    info!("Start scannig");
    let mut stream = temp_client.list_workflows(query, WorkflowListOptions::builder().build());

    // per-tick counters → emitted as gauges at the end (exact per-scan totals, no window aliasing)
    let (mut listed, mut processed, mut updated, mut skipped) = (0u64, 0u64, 0u64, 0u64);

    while let Some(workflow) = stream.next().await {
        if token.is_cancelled() {
            info!("cancellation requested - stopping scanning at safe point");
            break;
        }
        metrics::counter!("workflows_listed_total", "namespace" => namespace.to_string())
            .increment(1);
        listed += 1;

        let wf = workflow?;
        let new_history_length = wf.history_length();

        let prev_history_length: Option<i64> = db_conn
            .query_row(
                "SELECT history_length FROM workflows WHERE namespace =?1 AND workflow_id = ?2 AND run_id = ?3",
                (namespace, wf.id(), wf.run_id()),
                |r| r.get(0),
            )
            .optional()?;

        if prev_history_length == Some(new_history_length) {
            metrics::counter!("workflows_skipped_unchanged_total", "namespace" => namespace.to_string())
                 .increment(1);
            skipped += 1;
            continue; // workflow was  unchanged
        }

        let events = temporal::fetch_history(temp_client, namespace, wf.id(), wf.run_id()).await?;
        // let tokens: Vec<String> = events.iter().filter_map(tokenizer::event_token).collect();

        let tokens: Vec<String> = match events
            .iter()
            .map(tokenizer::event_token)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(tokens) => tokens.into_iter().flatten().collect(),
            Err(e) => {
                metrics::counter!(
                    "workflows_dropped_total",
                    "namespace" => namespace.to_string(),
                    "reason" => e.to_string(),
                )
                .increment(1);

                error!(
                    workflow_id = %wf.id(),
                    run_id = %wf.run_id(),
                    error = %e,
                    "skipping workflow because semantic hash would be incomplete"
                );
                continue;
            }
        };

        let hash = tokenizer::semantic_hash(&tokens.join("\n"));

        info!(workflow_id = %wf.id(), wf_type = %wf.workflow_type(), "scanned");

        metrics::counter!("workflows_processed_total", "namespace" => namespace.to_string())
            .increment(1);
        processed += 1;

        let start = std::time::Instant::now();

        let result = db_conn.execute(
            "INSERT OR REPLACE INTO workflows
                      (namespace, workflow_id, run_id, workflow_type, history_length, semantic_hash)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                namespace,
                wf.id(),
                wf.run_id(),
                wf.workflow_type(),
                wf.history_length(),
                &hash,
            ),
        );

        metrics::counter!("scan_workflows_updated_total", "namespace" => namespace.to_string())
            .increment(1);
        updated += 1;

        metrics::histogram!("db_write_duration_seconds", "op" => "upsert")
            .record(start.elapsed().as_secs_f64());

        metrics::counter!(
            "db_writes_total",
            "op" => "upsert",
            "result" => if result.is_ok() { "ok" } else { "error" },
        )
        .increment(1);

        let affected = result?;

        info!(workflow_id = %wf.id(), affected, "workflow updated in db");
    }

    // per-tick throughput: exact totals for THIS scan cycle (gauge = last tick's value, held until next tick)
    metrics::gauge!("scan_workflows_listed", "namespace" => namespace.to_string()).set(listed as f64);
    metrics::gauge!("scan_workflows_processed", "namespace" => namespace.to_string()).set(processed as f64);
    metrics::gauge!("scan_workflows_updated", "namespace" => namespace.to_string()).set(updated as f64);
    metrics::gauge!("scan_workflows_skipped", "namespace" => namespace.to_string()).set(skipped as f64);

    Ok(())
}
