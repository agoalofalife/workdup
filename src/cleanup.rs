use crate::{db, temporal};
use anyhow::Result;
use rusqlite::Connection as SqliteConnection;
use std::time::Duration;
use temporalio_client::{
    Client,
    grpc::WorkflowService,
    tonic::{Code, IntoRequest, Status},
};
use temporalio_common::protos::temporal::api::{
    common::v1::WorkflowExecution as WfExec, enums::v1::WorkflowExecutionStatus,
    workflowservice::v1::DescribeWorkflowExecutionRequest,
};
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub async fn run(
    namespace: String,
    db_path: &str,
    token: CancellationToken,
    tick_interval: Duration,
) -> Result<()> {
    let mut client = temporal::connect(namespace.clone()).await?;
    let conn = db::open(db_path)?;

    let mut ticker = interval(tick_interval);

    loop {
        tokio::select! {
            _ = token.cancelled() => { info!("cleanup stopping"); break; }
            _ = ticker.tick() => {
                if let Err(e) = run_once(&mut client, &namespace, &conn, &token).await {
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
             WHERE last_checked <= datetime('now', '-1 days');",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };

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
                    delete_workflow(conn, &workflow_id, &run_id)?;
                    info!(%workflow_id, ?status, "cleanup: removed completed workflow in temporal");
                }
            }

            Err(status) => match status.code() {
                Code::NotFound => {
                    // gone from temporal server, might be retention period for example if in temporal cloud
                    delete_workflow(conn, &workflow_id, &run_id)?;
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

fn delete_workflow(conn: &SqliteConnection, workflow_id: &str, run_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM workflows WHERE workflow_id = ?1 AND run_id = ?2",
        (&workflow_id, &run_id),
    )?;
    Ok(())
}
