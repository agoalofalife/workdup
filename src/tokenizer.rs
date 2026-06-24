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

pub fn event_token(event: &HistoryEvent) -> Result<Option<String>, String> {
    match event.event_type() {
        EventType::WorkflowExecutionStarted => {
            if let Some(Attributes::WorkflowExecutionStartedEventAttributes(attrs)) =
                &event.attributes
            {
                let name = attrs
                    .workflow_type
                    .as_ref()
                    .map(|t| t.name.as_str())
                    .ok_or_else(|| {
                        format!(
                            "'WorkflowExecutionStarted' event {} is missing 'workflowExecutionStartedEventAttributes'",
                            event.event_id
                        )
                    })?;

                Ok(Some(format!("WS:{name}")))
            } else {
                Err(format!(
                    "'WorkflowExecutionStarted' event {} is missing event attributes and could not find 'workflowExecutionStartedEventAttributes'",
                    event.event_id
                ))
            }
        }
        EventType::ActivityTaskScheduled => Ok(Some(format!("A:{:?}", event.event_type()))),
        EventType::ActivityTaskCompleted => Ok(Some("AC".into())),
        EventType::ActivityTaskFailed => Ok(Some(format!("AF:{:?}", event.event_type()))),
        EventType::ActivityTaskTimedOut => Ok(Some("ATO".into())),
        EventType::TimerStarted => {
            if let Some(Attributes::TimerStartedEventAttributes(attrs)) = &event.attributes {
                let timeout = attrs
                    .start_to_fire_timeout
                    .as_ref()
                    .map(|d| d.seconds)
                    .ok_or_else(|| {
                        format!(
                            "TimerStarted event {} has incorrect 'timerStartedEventAttributes.startToFireTimeout' attribute {:?}",
                            event.event_id, attrs.start_to_fire_timeout,
                        )
                    })?;

                Ok(Some(format!("T:{:?}", bucket(timeout))))
            } else {
                Err(format!(
                    "TimerStarted event {} is missing event attributes and could not find 'timerStartedEventAttributes' attribute",
                    event.event_id
                ))
            }
        }
        EventType::TimerFired => Ok(Some("TF".into())),
        EventType::TimerCanceled => Ok(Some("TX".into())),
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

                        if let (Some(id), Some(v)) = (change_id.as_ref(), version) {
                            Ok(Some(format!("V:\"{}\":{}", id, v))) // V:"my-change":2
                        } else {
                            Err(format!(
                                "MarkerRecorded event {} has invalid attribute 'markerRecordedEventAttributes' with change_id {:?} or version {:?}",
                                event.event_id, change_id, version
                            ))
                        }
                    }
                    "SideEffect" => Ok(Some("SE".into())),
                    undefined => Err(format!(
                        "MarkerRecorded event {} has undefined markerName value: {}, please pay attention and handle it..",
                        event.event_id, undefined
                    )),
                }
            } else {
                Err(format!(
                    "MarkerRecorded event {} is missing event attributes and could not find 'markerRecordedEventAttributes' attribute",
                    event.event_id
                ))
            }
        }
        EventType::StartChildWorkflowExecutionInitiated => {
            Ok(Some(format!("C:{:?}", event.event_type())))
        }
        EventType::ChildWorkflowExecutionCompleted => Ok(Some("CC".into())),
        EventType::ChildWorkflowExecutionFailed => Ok(Some(format!("CF:{:?}", event.event_type()))),
        EventType::ChildWorkflowExecutionTimedOut => Ok(Some("CTO".into())),
        EventType::ChildWorkflowExecutionCanceled => Ok(Some("CCx".into())),
        EventType::WorkflowExecutionCancelRequested => Ok(Some("CR".into())),
        EventType::WorkflowExecutionSignaled => Ok(Some(format!("S:{:?}", event.event_type()))),
        EventType::WorkflowExecutionCanceled => Ok(Some("DONE:canceled".into())),
        EventType::WorkflowExecutionCompleted => Ok(Some("DONE:success".into())),
        EventType::WorkflowExecutionFailed => {
            if let Some(Attributes::WorkflowExecutionFailedEventAttributes(a)) = &event.attributes {
                let failure = a.failure.as_ref().ok_or_else(|| {
                    format!(
                        "WorkflowExecutionFailed event {} is missing 'failure' attribute",
                        event.event_id
                    )
                })?;

                let info = match &failure.failure_info {
                    Some(FailureInfo::ApplicationFailureInfo(app)) => app.r#type.clone(), // e.g. "MyError"
                    Some(FailureInfo::TimeoutFailureInfo(_)) => "Timeout".to_string(),
                    Some(FailureInfo::CanceledFailureInfo(_)) => "Canceled".to_string(),
                    Some(other) => {
                        return Err(format!(
                            "WorkflowExecutionFailed event {} has an unhandled 'failure_info' variant: {:?}, please handle it",
                            event.event_id, other
                        ));
                    }
                    None => {
                        return Err(format!(
                            "WorkflowExecutionFailed event {} has no failure_info set",
                            event.event_id
                        ));
                    }
                };

                Ok(Some(format!("DONE:failure:{}", info)))
            } else {
                Err(format!(
                    "WorkflowExecutionFailed event {} is missing event attributes and could not find 'workflowExecutionFailedEventAttributes' attribute",
                    event.event_id
                ))
            }
        }
        EventType::WorkflowExecutionTerminated => Ok(Some("DONE:terminated".into())),
        EventType::WorkflowExecutionContinuedAsNew => Ok(Some("DONE:continue-as-new".into())),
        EventType::WorkflowTaskScheduled
        | EventType::WorkflowTaskStarted
        | EventType::WorkflowTaskCompleted
        | EventType::ActivityTaskStarted
        | EventType::ChildWorkflowExecutionStarted
        | EventType::UpsertWorkflowSearchAttributes
        | EventType::WorkflowTaskFailed
        | EventType::WorkflowTaskTimedOut => Ok(None),
        other => Err(format!(
            "Undefined type while trying to make hash string: {:?}",
            other
        )),
    }
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
