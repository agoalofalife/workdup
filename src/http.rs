use axum::{
    Json, Router,
    extract::MatchedPath,
    extract::State,
    http::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use metrics_exporter_prometheus::PrometheusBuilder;
use temporalio_common::telemetry::metrics::TemporalMeter;

use crate::{config::ResolvedNamespace, db, temporal::connect};
use axum::extract::Query;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Deserialize)]
struct NsQuery {
    namespace: String,
}

#[derive(Clone)]
struct AppState {
    db_path: String,
    namespaces: Vec<ResolvedNamespace>,
    meter: TemporalMeter,
}

#[derive(Serialize)]
pub struct UniqueWorkflow {
    pub workflow_id: String,
    pub run_id: String,
    pub workflow_type: String,
}

fn router(state: AppState) -> Router {
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install recoreder");

    let collector = std::sync::Arc::new(metrics_process::Collector::default());
    collector.describe();

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(readyz))
        .route("/unique-workflows", get(list_unique_workflows))
        .route("/stats", get(stats))
        .route(
            "/metrics",
            get({
                let collector = collector.clone();
                move || async move {
                    collector.collect(); // refresh process_* metric before rendering
                    handle.render()
                }
            }),
        )
        .route_layer(axum::middleware::from_fn(track_metrcis))
        .with_state(state)
}

pub async fn run(
    db_path: String,
    addr: String,
    token: CancellationToken,
    namespaces: Vec<ResolvedNamespace>,
    meter: TemporalMeter,
) -> anyhow::Result<()> {
    info!("binding http to {addr:?}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("http server listening on {addr}");

    axum::serve(
        listener,
        router(AppState {
            db_path,
            namespaces,
            meter,
        }),
    )
    .with_graceful_shutdown(async move { token.cancelled().await })
    .await?;
    Ok(())
}

#[derive(Serialize)]
pub struct Stats {
    pub workflows_count: i64,
    pub unique_workflows_count: i64,
}

/// GET /stats?namespace=foo
async fn stats(
    State(state): State<AppState>,
    Query(q): Query<NsQuery>,
) -> Result<Json<Vec<Stats>>, AppError> {
    let rows = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Stats>> {
        let conn = db::open(&state.db_path)?; // <-- a FRESH connection, every request

        let mut stmt = conn.prepare(
            "
            SELECT count(*) as workflows_count, (SELECT
            COUNT(distinct semantic_hash)
            FROM workflows) as unique_workflows_count from workflows
            WHERE namespace = ?1",
        )?;

        let items = stmt
            .query_map([&q.namespace], |r| {
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
    State(state): State<AppState>,
    Query(q): Query<NsQuery>,
) -> Result<Json<Vec<UniqueWorkflow>>, AppError> {
    // rusqlite is blocking + !Sync, so open + query on tokio's blocking pool,
    // not on the async thread.
    let rows = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<UniqueWorkflow>> {
        let conn = db::open(&state.db_path)?; // <-- a FRESH connection, every request

        let mut stmt = conn.prepare(
            "SELECT workflow_id, run_id, workflow_type
             FROM workflows
             WHERE namespace = ?1
             GROUP BY semantic_hash",
        )?;

        let items = stmt
            .query_map([&q.namespace], |r| {
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

async fn readyz(State(state): State<AppState>) -> Response {
    match check_ready(state).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, format!("not ready: {e}")).into_response(),
    }
}

async fn check_ready(state: AppState) -> anyhow::Result<()> {
    let db_path = state.db_path.clone();

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = db::open(&db_path)?;
        conn.query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    })
    .await??;

    for ns in &state.namespaces {
        connect(ns, state.meter.clone()).await?;
    }

    Ok(())
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

async fn track_metrcis(req: Request<axum::body::Body>, next: Next) -> Response {
    let method = req.method().clone();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());

    let start = std::time::Instant::now();
    let response = next.run(req).await;
    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    metrics::histogram!("http_request_duration_seconds", "route" => route.clone()).record(latency);
    metrics::counter!(
        "http_requests_total",
        "route" => route,
        "method" => method.to_string(),
        "status" => status,
    )
    .increment(1);

    response
}
