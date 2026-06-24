use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};

use serde::Serialize;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::db;

#[derive(Serialize)]
pub struct UniqueWorkflow {
    pub workflow_id: String,
    pub run_id: String,
    pub workflow_type: String,
}

fn router(db_path: String) -> Router {
    Router::new()
        .route("/unique-workflows", get(list_unique_workflows))
        .route("/stats", get(stats))
        .with_state(db_path)
}

pub async fn run(db_path: String, addr: String, token: CancellationToken) -> anyhow::Result<()> {
    info!("binding http to {addr:?}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("http server listening on {addr}");

    axum::serve(listener, router(db_path))
        .with_graceful_shutdown(async move { token.cancelled().await })
        .await?;
    Ok(())
}

#[derive(Serialize)]
pub struct Stats {
    pub workflows_count: i64,
    pub unique_workflows_count: i64,
}

async fn stats(State(db_path): State<String>) -> Result<Json<Vec<Stats>>, AppError> {
    let rows = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Stats>> {
        let conn = db::open(&db_path)?; // <-- a FRESH connection, every request

        let mut stmt = conn.prepare(
            "
            SELECT count(*) as workflows_count, (SELECT
            COUNT(distinct semantic_hash)
            FROM workflows) as unique_workflows_count from workflows",
        )?;

        let items = stmt
            .query_map([], |r| {
                Ok(Stats {
                    workflows_count: r.get("workflows_count")?,
                    unique_workflows_count: r.get("unique_workflows_count")?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(items)
    })
    .await??; // 1st ? = blocking task join, 2nd ? = the query result

    Ok(Json(rows))
}

async fn list_unique_workflows(
    State(db_path): State<String>,
) -> Result<Json<Vec<UniqueWorkflow>>, AppError> {
    // rusqlite is blocking + !Sync, so open + query on tokio's blocking pool,
    // not on the async thread.
    let rows = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<UniqueWorkflow>> {
        let conn = db::open(&db_path)?; // <-- a FRESH connection, every request

        let mut stmt = conn.prepare(
            "SELECT workflow_id, run_id, workflow_type
             FROM workflows
             GROUP BY semantic_hash",
        )?;

        let items = stmt
            .query_map([], |r| {
                Ok(UniqueWorkflow {
                    workflow_id: r.get("workflow_id")?,
                    run_id: r.get("run_id")?,
                    workflow_type: r.get("workflow_type")?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items)
    })
    .await??; // 1st ? = blocking task join, 2nd ? = the query result

    Ok(Json(rows))
}

pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("error: {}", self.0),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
