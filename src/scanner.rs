use anyhow::Result;
use futures_util::StreamExt;
use rusqlite::{Connection, OptionalExtension};
use temporalio_client::{Client, WorkflowListOptions};
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{config::ResolvedNamespace, db, temporal, tokenizer};

pub async fn run(cfg: ResolvedNamespace, db_path: &str, token: CancellationToken) -> Result<()> {
    let mut temp_client = temporal::connect(&cfg).await?;
    let db_conn = db::open(db_path)?;

    let mut ticker = interval(cfg.scan_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay); // will rescheduled when finished + timeout time

    loop {
        tokio::select! {
            _ = token.cancelled() => { info!("scanner stopping"); break; }
            _ = ticker.tick() => {
                if let Err(e) = scan(&mut temp_client, &cfg.name, &db_conn, &token, &cfg.query).await {
                    error!(error = %e, "scan tick failed"); // log, keep looping
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

    while let Some(workflow) = stream.next().await {
        if token.is_cancelled() {
            info!("cancellation requested - stopping scanning at safe point");
            break;
        }
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

        db_conn.execute(
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
        )?;

        info!("workflow {} id updated in db", wf.id());
    }
    Ok(())
}
