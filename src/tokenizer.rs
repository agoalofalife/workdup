use sha2::{Digest, Sha256};
use temporalio_common::protos::temporal::api::{
    enums::v1::EventType,
    failure::v1::failure::FailureInfo,
    history::v1::{HistoryEvent, history_event::Attributes},
};

pub fn semantic_hash(tokens: &str) -> String {
    Sha256::digest(tokens.as_bytes())
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

use tracing::{error, warn};

pub fn event_token(event: &HistoryEvent) -> Option<String> {
    match event.event_type() {
        EventType::WorkflowExecutionStarted => Some(format!("WS:{:?}", event.event_type())),
        EventType::ActivityTaskScheduled => Some(format!("A:{:?}", event.event_type())),
        EventType::ActivityTaskCompleted => Some("AC".into()),
        EventType::ActivityTaskFailed => Some(format!("AF:{:?}", event.event_type())),
        EventType::ActivityTaskTimedOut => Some("ATO".into()),
        EventType::TimerStarted => {
            if let Some(Attributes::TimerStartedEventAttributes(attrs)) = &event.attributes {
                let timeout = attrs
                    .start_to_fire_timeout
                    .as_ref()
                    .map(|d| d.seconds)
                    .unwrap_or(0);

                Some(format!("T:{:?}", bucket(timeout)))
            } else {
                None
            }
        }
        EventType::TimerFired => Some("TF".into()),
        EventType::TimerCanceled => Some("TX".into()),
        EventType::MarkerRecorded => {
            if let Some(Attributes::MarkerRecordedEventAttributes(attrs)) = &event.attributes {
                match attrs.marker_name.as_str() {
                    "Version" => {
                        let change_id: Option<String> = attrs
                            .details
                            .get("changeId")
                            .and_then(|p| p.payloads.first())
                            .and_then(|p| serde_json::from_slice(&p.data).ok());

                        let version: Option<i64> = attrs
                            .details
                            .get("version")
                            .and_then(|p| p.payloads.first())
                            .and_then(|p| serde_json::from_slice(&p.data).ok());

                        if let (Some(id), Some(v)) = (change_id, version) {
                            Some(format!("V:\"{}\":{}", id, v)) // V:"my-change":2
                        } else {
                            None
                        }
                    }
                    "SideEffect" => Some("SE".into()),
                    undefined => {
                        warn!("new type {} of MarkerRecorded", undefined);
                        None
                    }
                }
            } else {
                None
            }
        }
        EventType::StartChildWorkflowExecutionInitiated => {
            Some(format!("C:{:?}", event.event_type()))
        }
        EventType::ChildWorkflowExecutionCompleted => Some("CC".into()),
        EventType::ChildWorkflowExecutionFailed => Some(format!("CF:{:?}", event.event_type())),
        EventType::ChildWorkflowExecutionTimedOut => Some("CTO".into()),
        EventType::ChildWorkflowExecutionCanceled => Some("CCx".into()),
        EventType::WorkflowExecutionCancelRequested => Some("CR".into()),
        EventType::WorkflowExecutionSignaled => Some(format!("S:{:?}", event.event_type())),
        EventType::WorkflowExecutionCanceled => Some("DONE:canceled".into()),
        EventType::WorkflowExecutionCompleted => Some("DONE:success".into()),
        EventType::WorkflowExecutionFailed => {
            if let Some(Attributes::WorkflowExecutionFailedEventAttributes(a)) = &event.attributes {
                let info = a
                    .failure
                    .as_ref()
                    .map(|f| match &f.failure_info {
                        Some(FailureInfo::ApplicationFailureInfo(app)) => app.r#type.clone(), // e.g. "MyError"
                        Some(FailureInfo::TimeoutFailureInfo(_)) => "Timeout".to_string(),
                        Some(FailureInfo::CanceledFailureInfo(_)) => "Canceled".to_string(),
                        _ => f.message.clone(), // fall back to message if no typed info
                    })
                    .unwrap_or_default();
                Some(format!("DONE:failure:{}", info))
            } else {
                error!("Could not define type of WorkflowExecutionFailed");
                None
            }
        }
        EventType::WorkflowExecutionTerminated => Some("DONE:terminated".into()),
        EventType::WorkflowExecutionContinuedAsNew => Some("DONE:continue-as-new".into()),
        EventType::WorkflowTaskScheduled
        | EventType::WorkflowTaskStarted
        | EventType::WorkflowTaskCompleted
        | EventType::ActivityTaskStarted
        | EventType::ChildWorkflowExecutionStarted
        | EventType::UpsertWorkflowSearchAttributes
        | EventType::WorkflowTaskFailed
        | EventType::WorkflowTaskTimedOut => None,
        other => {
            warn!(
                "Undefined type while trying to make hash string: {:?}",
                other
            );
            None
        }
    }
}

fn bucket(seconds: i64) -> i64 {
    match seconds {
        ..=60 => 60,
        ..=600 => 600,         // 10 minutes
        ..=3600 => 3600,       // 1 hour
        ..=86400 => 86400,     // 1 day (24 hours)
        ..=604800 => 604800,   // 7 days
        ..=2592000 => 2592000, // 30 days
        ..=7776000 => 7776000, // 90 days
        _ => 7776000,          // 90 days for others
    }
}
