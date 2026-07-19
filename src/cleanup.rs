use crate::{
    config::ResolvedNamespace,
    db::{self, refresh_db_gauges},
    temporal,
};
use anyhow::Result;
use rusqlite::Connection as SqliteConnection;
use temporalio_client::{
    Client,
    grpc::WorkflowService,
    tonic::{Code, IntoRequest, Status},
};
use temporalio_common::{
    protos::temporal::api::{
        common::v1::WorkflowExecution as WfExec, enums::v1::WorkflowExecutionStatus,
        workflowservice::v1::DescribeWorkflowExecutionRequest,
    },
    telemetry::metrics::TemporalMeter,
};
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub async fn run(
    cfg: ResolvedNamespace,
    db_path: &str,
    token: CancellationToken,
    meter: TemporalMeter,
) -> Result<()> {
    let mut client = temporal::connect(&cfg, meter).await?;
    let conn = db::open(db_path)?;

    let mut ticker = interval(cfg.cleanup_interval);

    loop {
        tokio::select! {
            _ = token.cancelled() => { info!("cleanup stopping"); break; }
            _ = ticker.tick() => {
                let start = std::time::Instant::now();

                let result = run_once(&mut client, &cfg.name, &conn, &token).await;

                metrics::gauge!("cleanup_duration_seconds", "namespace" => cfg.name.clone())
                    .set(start.elapsed().as_secs_f64());

                metrics::counter!(
                    "cleanup_runs_total",
                    "namespace" => cfg.name.clone(),
                    "result" => if result.is_ok() { "ok" } else { "error" },
                ).increment(1);

                refresh_db_gauges(&conn, db_path)?;

                if let Err(e) = result {
                    error!(error = %e, "cleanup tick failed");
                }

            }
        }
    }
    Ok(())
}

async fn run_once(
    client: &mut Client,
    namespace: &str,
    conn: &SqliteConnection,
    token: &CancellationToken,
) -> Result<()> {
    info!("Start clean database");

    // Oldest-checked records first, in small batches.
    let candidates: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT workflow_id, run_id FROM workflows
             WHERE namespace = ?1 AND last_checked <= datetime('now', '-1 days');",
        )?;
        let rows = stmt.query_map([namespace], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    // exact rows deleted in THIS run → emitted as a gauge at the end
    let mut deleted_this_run: u64 = 0;

    for (workflow_id, run_id) in candidates {
        if token.is_cancelled() {
            info!("cancellation requested - stopping cleanup at safe point");
            break;
        }

        match get_workflow_status(client, namespace, &workflow_id, &run_id).await {
            Ok(status) => {
                if status != WorkflowExecutionStatus::Running {
                    // run_id is important, because continue_as_new feature
                    // workflow_id might have few run ids
                    deleted_this_run +=
                        delete_workflow(conn, namespace, &workflow_id, &run_id)? as u64;
                    info!(%workflow_id, ?status, "cleanup: removed completed workflow in temporal");
                }
            }

            Err(status) => match status.code() {
                Code::NotFound => {
                    // gone from temporal server, might be retention period for example if in temporal cloud
                    deleted_this_run +=
                        delete_workflow(conn, namespace, &workflow_id, &run_id)? as u64;
                    info!(%workflow_id, ?status, "cleanup: not found in temporal, removed state record");
                }
                Code::Unavailable | Code::DeadlineExceeded => {
                    warn!(%workflow_id, "temporal unavailable, retrying next tick");
                }
                other => {
                    error!(%workflow_id, ?other, msg = %status.message(), "unexpected gRPC status")
                }
            },
        }
    }

    // exact per-run total (gauge = last run's deletions, held until next run)
    metrics::gauge!("cleanup_rows_deleted", "namespace" => namespace.to_string())
        .set(deleted_this_run as f64);

    Ok(())
}

async fn get_workflow_status(
    client: &mut Client,
    namespace: &str,
    workflow_id: &str,
    run_id: &str,
) -> Result<WorkflowExecutionStatus, Status> {
    let resp = client
        .describe_workflow_execution(
            DescribeWorkflowExecutionRequest {
                namespace: namespace.to_string(),
                execution: Some(WfExec {
                    workflow_id: workflow_id.to_string(),
                    run_id: run_id.to_string(),
                }),
            }
            .into_request(),
        )
        .await?
        .into_inner();

    let status = resp
        .workflow_execution_info
        .ok_or_else(|| Status::internal(format!("no execution info for {workflow_id}")))?
        .status();
    Ok(status)
}

fn delete_workflow(
    conn: &SqliteConnection,
    namespace: &str,
    workflow_id: &str,
    run_id: &str,
) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM workflows WHERE namespace =?1 AND workflow_id = ?2 AND run_id = ?3",
        (namespace, &workflow_id, &run_id),
    )?;
    Ok(deleted)
}
