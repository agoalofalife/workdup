use anyhow::{Result, anyhow};
use temporalio_client::{
    Client, ClientOptions, Connection, grpc::WorkflowService, tonic::IntoRequest,
};
use temporalio_common::envconfig::LoadClientConfigProfileOptions;
use temporalio_common::protos::temporal::api::common::v1::WorkflowExecution as WfExec;
use temporalio_common::protos::temporal::api::history::v1::HistoryEvent;
use temporalio_common::protos::temporal::api::workflowservice::v1::GetWorkflowExecutionHistoryRequest;

pub async fn connect(namespace: String) -> Result<Client> {
    let (conn_opts, mut client_opts) =
        ClientOptions::load_from_config(LoadClientConfigProfileOptions::default())
            .map_err(|e| anyhow!("failed to load temporal config: {e}"))?;

    client_opts.namespace = namespace;
    let connection = Connection::connect(conn_opts).await?;
    let client = Client::new(connection, client_opts.clone())?;

    Ok(client)
}

/// Fetch the *complete* event history for one workflow, following pagination.
pub async fn fetch_history(
    client: &mut Client,
    namespace: &str,
    workflow_id: &str,
    run_id: &str,
) -> anyhow::Result<Vec<HistoryEvent>> {
    let mut events = Vec::new();
    let mut next_page_token = Vec::new();

    loop {
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

        if let Some(history) = resp.history {
            events.extend(history.events);
        }

        // Empty token => no more pages.
        if resp.next_page_token.is_empty() {
            break;
        }
        next_page_token = resp.next_page_token;
    }

    Ok(events)
}
