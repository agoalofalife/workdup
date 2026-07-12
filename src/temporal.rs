use temporalio_client::{
    Client, ClientOptions, ClientTlsOptions, Connection, ConnectionOptions, TlsOptions,
    grpc::WorkflowService, tonic::IntoRequest,
};
use temporalio_common::protos::temporal::api::common::v1::WorkflowExecution as WfExec;
use temporalio_common::protos::temporal::api::history::v1::HistoryEvent;
use temporalio_common::protos::temporal::api::workflowservice::v1::GetWorkflowExecutionHistoryRequest;
use tracing::{debug, instrument};

use crate::config::{ResolvedNamespace, Tls};
use anyhow::{Context, Result};
use std::fs;
use url::Url;

use temporalio_common::telemetry::{
    PrometheusExporterOptions, TaskQueueLabelStrategy,
    metrics::{NewAttributes, TemporalMeter},
    start_prometheus_metric_exporter,
};

pub async fn connect(ns: &ResolvedNamespace, meter: TemporalMeter) -> Result<Client> {
    let use_tls = ns.tls.is_some() || ns.api_key.is_some();
    let target = parse_target(&ns.host, use_tls)?;

    let tls_options = match &ns.tls {
        Some(tls) => Some(build_tls(tls)?),
        None if ns.api_key.is_some() => Some(TlsOptions::default()),
        None => None,
    };
    let conn_opts = ConnectionOptions::new(target)
        .maybe_api_key(ns.api_key.clone())
        .maybe_tls_options(tls_options)
        .metrics_meter(meter.clone())
        .build();

    let client_opts = ClientOptions::new(ns.name.clone()).build();

    let connection = Connection::connect(conn_opts).await?;

    Ok(Client::new(connection, client_opts)?)
}

/// `host` may be `host:port` (no scheme) or a full URL. Mirror the SDK: try as-is,
/// otherwise prepend https/http depending on whether TLS is on.
fn parse_target(host: &str, use_tls: bool) -> Result<Url> {
    if let Ok(url) = Url::parse(host)
        && url.has_host()
    {
        return Ok(url);
    }
    let scheme = if use_tls { "https" } else { "http" };
    Url::parse(&format!("{scheme}://{host}")).with_context(|| format!("invalid host '{host}'"))
}

fn build_tls(tls: &Tls) -> Result<TlsOptions> {
    let client_cert = fs::read(&tls.cert_path)
        .with_context(|| format!("read cert {}", tls.cert_path.display()))?;

    let client_private_key =
        fs::read(&tls.key_path).with_context(|| format!("read key {}", tls.key_path.display()))?;

    let server_root_ca_cert = tls
        .ca_path
        .as_ref()
        .map(|p| fs::read(p).with_context(|| format!("read ca {}", p.display())))
        .transpose()?;

    Ok(TlsOptions {
        server_root_ca_cert,
        domain: None, // set if your cert's SNI differs from the host
        client_tls_options: Some(ClientTlsOptions {
            client_cert,
            client_private_key,
        }),
    })
}

/// Fetch the *complete* event history for one workflow, following pagination.
#[instrument(skip(client))]
pub async fn fetch_history(
    client: &mut Client,
    namespace: &str,
    workflow_id: &str,
    run_id: &str,
) -> anyhow::Result<Vec<HistoryEvent>> {
    let mut events = Vec::new();
    let mut next_page_token = Vec::new();
    let mut page_num = 1;
    let start = std::time::Instant::now();

    debug!("Start fetching history from workflow: {workflow_id}");

    loop {
        debug!("Featch history of workflow on page:{page_num}");

        let resp = client
            .get_workflow_execution_history(
                GetWorkflowExecutionHistoryRequest {
                    namespace: namespace.to_string(),
                    execution: Some(WfExec {
                        workflow_id: workflow_id.to_string(),
                        run_id: run_id.to_string(),
                    }),
                    maximum_page_size: 0, // 0 = server default page size
                    next_page_token: next_page_token.clone(),
                    wait_new_event: false,
                    history_event_filter_type: 0, // 0 = ALL_EVENT
                    skip_archival: false,
                }
                .into_request(),
            )
            .await?
            .into_inner();

        metrics::counter!("history_pages_fetched_total", "namespace" => namespace.to_string())
            .increment(1);

        if let Some(history) = resp.history {
            metrics::counter!("history_events_fetched_total", "namespace" => namespace.to_string())
                .increment(history.events.len() as u64);
            events.extend(history.events);
        }

        // Empty token => no more pages.
        if resp.next_page_token.is_empty() {
            break;
        }
        next_page_token = resp.next_page_token;
        page_num += 1;
    }

    metrics::histogram!("history_fetch_duration_seconds", "namespace" => namespace.to_string())
        .record(start.elapsed().as_secs_f64());

    Ok(events)
}

pub fn temporal_meter(addr: std::net::SocketAddr) -> anyhow::Result<TemporalMeter> {
    let opts = PrometheusExporterOptions::builder()
        .socket_addr(addr)
        .counters_total_suffix(true)
        .use_seconds_for_durations(true)
        .build();

    let started = start_prometheus_metric_exporter(opts)?;

    Ok(TemporalMeter::new(
        started.meter,
        NewAttributes::new(vec![]),
        TaskQueueLabelStrategy::UseNormal,
    ))
}
