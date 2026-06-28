use sha2::{Digest, Sha256};
use temporalio_common::protos::temporal::api::{
    common::v1::Payload,
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

macro_rules! expect_attrs {
    ($event:expr, $variant:ident) => {
        match &$event.attributes {
            Some(Attributes::$variant(a)) => a,
            _ => {
                return Err(format!(
                    "{:?} event {} is missing expected '{}'",
                    $event.event_type(),
                    $event.event_id,
                    stringify!($variant)
                ));
            }
        }
    };
}

pub fn event_token(event: &HistoryEvent) -> Result<Option<String>, String> {
    let event_type = event.event_type();
    match event_type {
        EventType::WorkflowExecutionStarted => {
            let attrs = expect_attrs!(event, WorkflowExecutionStartedEventAttributes);

            let name = attrs
                .workflow_type
                .as_ref()
                .map(|t| t.name.as_str())
                .ok_or_else(|| {
                    format!(
                        "'{event_type:?}' event {} is missing 'workflowExecutionStartedEventAttributes'",
                        event.event_id
                    )
                })?;

            Ok(Some(format!("WS:{name}")))
        }
        EventType::ActivityTaskScheduled => {
            let attrs = expect_attrs!(event, ActivityTaskScheduledEventAttributes);

            let name = attrs
                .activity_type
                .as_ref()
                .map(|t| t.name.as_str())
                .ok_or_else(|| {
                    format!(
                        "'{event_type:?}' event {} is missing 'ActivityTaskScheduledEventAttributes'",
                        event.event_id
                    )
                })?;

            Ok(Some(format!("A:{name}")))
        }
        EventType::ActivityTaskCompleted => Ok(Some("AC".into())),
        EventType::ActivityTaskFailed => {
            let attrs = expect_attrs!(event, ActivityTaskFailedEventAttributes);

            let failure = attrs.failure.as_ref().ok_or_else(|| {
                format!(
                    "{event_type:?} event {} is missing 'failure' attribute",
                    event.event_id
                )
            })?;

            let info = match &failure.failure_info {
                Some(FailureInfo::ApplicationFailureInfo(app)) => app.r#type.clone(), // e.g. "MyError"
                Some(FailureInfo::TimeoutFailureInfo(_)) => "Timeout".to_string(),
                Some(FailureInfo::CanceledFailureInfo(_)) => "Canceled".to_string(),
                Some(other) => {
                    return Err(format!(
                        "{event_type:?} event {} has an unhandled 'failure_info' variant: {:?}, please handle it",
                        event.event_id, other
                    ));
                }
                None => {
                    return Err(format!(
                        "{event_type:?} event {} has no failure_info set",
                        event.event_id
                    ));
                }
            };

            Ok(Some(format!("AF:{info}")))
        }
        EventType::ActivityTaskCanceled => Ok(Some("ACx".into())),
        EventType::ActivityTaskTimedOut => Ok(Some("ATO".into())),
        EventType::TimerStarted => {
            let attrs = expect_attrs!(event, TimerStartedEventAttributes);

            let timeout = attrs
                .start_to_fire_timeout
                .as_ref()
                .map(|d| d.seconds)
                .ok_or_else(|| {
                    format!(
                        "{event_type:?} event {} has incorrect 'timerStartedEventAttributes.startToFireTimeout' attribute {:?}",
                        event.event_id, attrs.start_to_fire_timeout,
                    )
                })?;

            Ok(Some(format!("T:{}", bucket(timeout))))
        }
        EventType::TimerFired => Ok(Some("TF".into())),
        EventType::TimerCanceled => Ok(Some("TX".into())),
        EventType::MarkerRecorded => {
            let attrs = expect_attrs!(event, MarkerRecordedEventAttributes);

            match attrs.marker_name.as_str() {
                "Version" => {
                    // Detail key differs by SDK: Go writes "changeId",
                    // Java writes "change-id".
                    let change_id = attrs
                        .details
                        .get("changeId")
                        .or_else(|| attrs.details.get("change-id"))
                        .and_then(|p| p.payloads.first())
                        .and_then(marker_payload_string);

                    let version: Option<i64> = attrs
                        .details
                        .get("version")
                        .and_then(|p| p.payloads.first())
                        .and_then(|p| serde_json::from_slice(&p.data).ok());

                    match (change_id, version) {
                        (Some(id), Some(v)) => Ok(Some(format!("V:{id:?}:{v}"))), // V:"my-change":2
                        // changeId could not be recovered, but we still have a
                        // version: keep the workflow scannable with a degraded
                        // token rather than dropping the whole history.
                        (None, Some(v)) => Ok(Some(format!("V:?:{v}"))),
                        (id, v) => Err(format!(
                            "{event_type:?} event {} has an unparseable 'Version' marker (changeId={id:?}, version={v:?})",
                            event.event_id
                        )),
                    }
                }
                "SideEffect" => Ok(Some("SE".into())),
                undefined => Err(format!(
                    "{:?} event {} has undefined markerName value: {}, please pay attention and handle it..",
                    event_type, event.event_id, undefined
                )),
            }
        }
        EventType::StartChildWorkflowExecutionInitiated => {
            let attrs = expect_attrs!(event, StartChildWorkflowExecutionInitiatedEventAttributes);

            let name = attrs
                .workflow_type
                .as_ref()
                .map(|t| t.name.as_str())
                .ok_or_else(|| {
                    format!(
                        "'{event_type:?}' event {} is missing 'StartChildWorkflowExecutionInitiatedEventAttributes'",
                        event.event_id
                    )
                })?;

            Ok(Some(format!("C:{name}")))
        }
        EventType::ChildWorkflowExecutionCompleted => Ok(Some("CC".into())),
        EventType::ChildWorkflowExecutionTerminated => Ok(Some("CTx".into())),
        EventType::ChildWorkflowExecutionFailed => {
            let attrs = expect_attrs!(event, ChildWorkflowExecutionFailedEventAttributes);

            let failure = attrs.failure.as_ref().ok_or_else(|| {
                format!(
                    "{event_type:?} event {} is missing 'failure' attribute",
                    event.event_id
                )
            })?;

            let info = match &failure.failure_info {
                Some(FailureInfo::ApplicationFailureInfo(app)) => app.r#type.clone(), // e.g. "MyError"
                Some(FailureInfo::TimeoutFailureInfo(_)) => "Timeout".to_string(),
                Some(FailureInfo::CanceledFailureInfo(_)) => "Canceled".to_string(),
                Some(other) => {
                    return Err(format!(
                        "{:?} event {} has an unhandled 'failure_info' variant: {:?}, please handle it",
                        event_type, event.event_id, other
                    ));
                }
                None => {
                    return Err(format!(
                        "{:?} event {} has no failure_info set",
                        event_type, event.event_id
                    ));
                }
            };

            Ok(Some(format!("CF:{info}")))
        }
        EventType::ChildWorkflowExecutionTimedOut => Ok(Some("CTO".into())),
        EventType::ChildWorkflowExecutionCanceled => Ok(Some("CCx".into())),
        EventType::WorkflowExecutionCancelRequested => Ok(Some("CR".into())),
        EventType::WorkflowExecutionSignaled => {
            let attrs = expect_attrs!(event, WorkflowExecutionSignaledEventAttributes);

            Ok(Some(format!("S:{}", attrs.signal_name)))
        }
        EventType::WorkflowExecutionCanceled => Ok(Some("DONE:canceled".into())),
        EventType::WorkflowExecutionCompleted => Ok(Some("DONE:success".into())),
        EventType::WorkflowExecutionTimedOut => Ok(Some("DONE:timedout".into())),
        EventType::WorkflowExecutionFailed => {
            let attrs = expect_attrs!(event, WorkflowExecutionFailedEventAttributes);

            let failure = attrs.failure.as_ref().ok_or_else(|| {
                format!(
                    "{event_type:?} event {} is missing 'failure' attribute",
                    event.event_id
                )
            })?;

            let info = match &failure.failure_info {
                Some(FailureInfo::ApplicationFailureInfo(app)) => app.r#type.clone(), // e.g. "MyError"
                Some(FailureInfo::TimeoutFailureInfo(_)) => "Timeout".to_string(),
                Some(FailureInfo::CanceledFailureInfo(_)) => "Canceled".to_string(),
                Some(other) => {
                    return Err(format!(
                        "{event_type:?} event {} has an unhandled 'failure_info' variant: {:?}, please handle it",
                        event.event_id, other
                    ));
                }
                None => {
                    return Err(format!(
                        "{event_type:?} event {} has no failure_info set",
                        event.event_id
                    ));
                }
            };

            Ok(Some(format!("DONE:failure:{info}")))
        }
        EventType::WorkflowExecutionTerminated => Ok(Some("DONE:terminated".into())),
        EventType::WorkflowExecutionContinuedAsNew => Ok(Some("DONE:continue-as-new".into())),
        EventType::RequestCancelExternalWorkflowExecutionInitiated => Ok(Some("RCE".into())),
        EventType::RequestCancelExternalWorkflowExecutionFailed => Ok(Some("RCEF".into())),
        EventType::SignalExternalWorkflowExecutionFailed => Ok(Some("SIGF".into())),
        EventType::WorkflowExecutionUpdateCompleted => Ok(Some("UC".into())),
        EventType::SignalExternalWorkflowExecutionInitiated => {
            let attrs = expect_attrs!(
                event,
                SignalExternalWorkflowExecutionInitiatedEventAttributes
            );

            Ok(Some(format!("SIG:{}", attrs.signal_name)))
        }
        EventType::StartChildWorkflowExecutionFailed => {
            let attrs = expect_attrs!(event, StartChildWorkflowExecutionFailedEventAttributes);

            Ok(Some(format!("CSF:{:?}", attrs.cause())))
        }
        EventType::WorkflowExecutionUpdateAccepted => {
            let attrs = expect_attrs!(event, WorkflowExecutionUpdateAcceptedEventAttributes);

            let name = attrs
                                .accepted_request
                                .as_ref()
                                .and_then(|r| r.input.as_ref())
                                .map(|i| i.name.as_str())
                                .ok_or_else(|| {
                                    format!(
                                        "{event_type:?} event {} has incorrect 'workflowExecutionUpdateAcceptedEventAttributes.accepted_request' attribute {:?}",
                                        event.event_id, attrs.accepted_request,
                                    )
                                })?;

            Ok(Some(format!("U:{name}")))
        }
        EventType::WorkflowExecutionUpdateRejected => Err(format!(
            "{:?} event {} does not expect to be in history",
            event_type, event.event_id
        )),
        EventType::NexusOperationScheduled
        | EventType::NexusOperationCompleted
        | EventType::NexusOperationCanceled
        | EventType::NexusOperationTimedOut
        | EventType::NexusOperationCancelRequested
        | EventType::NexusOperationStarted
        | EventType::NexusOperationCancelRequestCompleted
        | EventType::NexusOperationCancelRequestFailed
        | EventType::NexusOperationFailed => Err(format!(
            "{event_type:?} event {} is not supported yet",
            event.event_id
        )),
        EventType::WorkflowTaskScheduled
        | EventType::WorkflowTaskStarted
        | EventType::WorkflowTaskCompleted
        | EventType::WorkflowExecutionUnpaused
        | EventType::WorkflowExecutionPaused
        | EventType::ActivityTaskStarted
        | EventType::WorkflowExecutionOptionsUpdated
        | EventType::WorkflowPropertiesModifiedExternally
        | EventType::WorkflowPropertiesModified
        | EventType::WorkflowExecutionUpdateAdmitted
        | EventType::ActivityPropertiesModifiedExternally
        | EventType::ChildWorkflowExecutionStarted
        | EventType::WorkflowExecutionTimeSkippingTransitioned
        | EventType::ExternalWorkflowExecutionSignaled
        | EventType::UpsertWorkflowSearchAttributes
        | EventType::WorkflowTaskFailed
        | EventType::ExternalWorkflowExecutionCancelRequested
        | EventType::ActivityTaskCancelRequested
        | EventType::WorkflowTaskTimedOut => Ok(None),
        other => Err(format!(
            "Undefined type while trying to make hash string: {:?}",
            other
        )),
    }
}

/// Read a marker-detail payload as a string.
///
/// Temporal's Go SDK writes the `changeId` as a JSON string (`json/plain` ->
/// `"my-change"`), but some SDKs/versions store it as raw bytes
/// (`binary/plain` -> `my-change`), which `serde_json::from_slice::<String>`
/// rejects. Try JSON first, then fall back to the raw UTF-8 bytes so a single
/// odd encoding never costs us the whole workflow.
fn marker_payload_string(payload: &Payload) -> Option<String> {
    if let Ok(s) = serde_json::from_slice::<String>(&payload.data) {
        return Some(s);
    }

    let raw = String::from_utf8_lossy(&payload.data);
    let trimmed = raw.trim().trim_matches('"');
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn bucket(seconds: i64) -> i64 {
    match seconds {
        0..=60 => 60,
        61..=600 => 600,             // 10 minutes
        601..=3600 => 3600,          // 1 hour
        3601..=86400 => 86400,       // 1 day (24 hours)
        86401..=604800 => 604800,    // 7 days
        604801..=2592000 => 2592000, // 30 days
        ..=7776000 => 7776000,       // 90 days
        _ => 7776000,                // 90 days for others
    }
}
