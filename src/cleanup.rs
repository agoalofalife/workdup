use crate::{db, temporal};
use anyhow::{Result, anyhow};
use rusqlite::Connection as SqliteConnection;
use std::time::Duration;
use temporalio_client::{Client, grpc::WorkflowService, tonic::IntoRequest};
use temporalio_common::protos::temporal::api::{
    common::v1::WorkflowExecution as WfExec, enums::v1::WorkflowExecutionStatus,
    workflowservice::v1::DescribeWorkflowExecutionRequest,
};
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub async fn run(namespace: String, db_path: &str, token: CancellationToken) -> Result<()> {
    let mut client = temporal::connect(namespace.clone()).await?;
    let conn = db::open(&db_path)?;

    let mut ticker = interval(Duration::from_hours(1));

    loop {
        tokio::select! {
            _ = token.cancelled() => { info!("cleanup stopping"); break; }
            _ = ticker.tick() => {
                if let Err(e) = run_once(&mut client, &namespace, &conn).await {
                    error!(error = %e, "cleanup tick failed");
                }
            }
        }
    }
    Ok(())
}

async fn run_once(client: &mut Client, namespace: &str, conn: &SqliteConnection) -> Result<()> {
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
        let status = describe_status(client, namespace, &workflow_id, &run_id).await?;

        if is_not_running(status) {
            // run_id is important, because continue_as_new feature
            // workflow_id might have few run ids
            conn.execute(
                "DELETE FROM workflows WHERE workflow_id = ?1 AND run_id = ?2",
                (&workflow_id, &run_id),
            )?;
            info!(%workflow_id, ?status, "cleanup: removed completed workflow");
        }
    }
    Ok(())
}

async fn describe_status(
    client: &mut Client,
    namespace: &str,
    workflow_id: &str,
    run_id: &str,
) -> Result<WorkflowExecutionStatus> {
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
        .ok_or_else(|| anyhow!("no execution info for {workflow_id}"))?
        .status();
    Ok(status)
}

fn is_not_running(s: WorkflowExecutionStatus) -> bool {
    use WorkflowExecutionStatus::*;

    s != Running
}
