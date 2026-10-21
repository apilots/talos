//! Typed Browser projection; caller/node identity stays in the existing envelope.
use crate::{environment_pb::invalid, error_pb};
use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::*;

macro_rules! enumeration {
    ($encode:ident, $decode:ident, $kind:ident, $($variant:ident),+) => {
        fn $encode(value: $kind) -> i32 { match value { $($kind::$variant => pb::$kind::$variant as i32,)+ } }
        fn $decode(value: i32) -> TalosResult<$kind> {
            match pb::$kind::try_from(value).map_err(|_| invalid())? {
                $(pb::$kind::$variant => Ok($kind::$variant),)+ _ => Err(invalid()),
            }
        }
    };
}
enumeration!(
    host_to,
    host_from,
    BrowserHostKind,
    Docker,
    Desktop,
    Native,
    Server
);
enumeration!(
    target_to,
    target_from,
    BrowserTargetKind,
    Managed,
    Connected
);
enumeration!(
    display_reason_to,
    display_reason_from,
    BrowserDisplayUnavailableReason,
    NativeHost,
    NotBundled,
    HostPolicy,
    UnsupportedVersion
);
enumeration!(
    local_to,
    local_from,
    BrowserLocalOpenCapability,
    DesktopOwned,
    ExternalUserWindow,
    Unavailable
);
enumeration!(
    suspension_to,
    suspension_from,
    BrowserSuspensionReason,
    UserPaused,
    ViewerDisconnected,
    WorkerRestarted,
    TakeoverFailed,
    PluginDisabled
);

pub fn encode_browser_params(value: BrowserRequest) -> TalosResult<pb::BrowserParams> {
    use pb::browser_params::Operation as Wire;
    Ok(pb::BrowserParams {
        operation: Some(match value.operation {
            BrowserOperation::Read => Wire::Read(pb::Empty {}),
            BrowserOperation::SelectTarget {
                target_id,
                expected_revision,
            } => Wire::SelectTarget(pb::BrowserSelectTarget {
                target_id,
                expected_revision,
            }),
            BrowserOperation::Connect { expected_revision } => {
                Wire::Connect(pb::BrowserExpectedRevision { expected_revision })
            }
            BrowserOperation::Disconnect { expected_revision } => {
                Wire::Disconnect(pb::BrowserExpectedRevision { expected_revision })
            }
            BrowserOperation::Pause { expected_revision } => {
                Wire::Pause(pb::BrowserExpectedRevision { expected_revision })
            }
            BrowserOperation::Resume { expected_revision } => {
                Wire::Resume(pb::BrowserExpectedRevision { expected_revision })
            }
            BrowserOperation::TakeControl { expected_revision } => {
                Wire::TakeControl(pb::BrowserExpectedRevision { expected_revision })
            }
            BrowserOperation::ReleaseControl {
                controller_id,
                expected_revision,
            } => Wire::ReleaseControl(pb::BrowserReleaseControl {
                controller_id,
                expected_revision,
            }),
            BrowserOperation::OpenLocal => Wire::OpenLocal(pb::Empty {}),
        }),
    })
}
pub fn decode_browser_params(value: Option<pb::BrowserParams>) -> TalosResult<BrowserRequest> {
    use pb::browser_params::Operation as Wire;
    let operation = match value.and_then(|v| v.operation).ok_or_else(invalid)? {
        Wire::Read(_) => BrowserOperation::Read,
        Wire::SelectTarget(v) => BrowserOperation::SelectTarget {
            target_id: v.target_id,
            expected_revision: checked(v.expected_revision)?,
        },
        Wire::Connect(v) => BrowserOperation::Connect {
            expected_revision: checked(v.expected_revision)?,
        },
        Wire::Disconnect(v) => BrowserOperation::Disconnect {
            expected_revision: checked(v.expected_revision)?,
        },
        Wire::Pause(v) => BrowserOperation::Pause {
            expected_revision: checked(v.expected_revision)?,
        },
        Wire::Resume(v) => BrowserOperation::Resume {
            expected_revision: checked(v.expected_revision)?,
        },
        Wire::TakeControl(v) => BrowserOperation::TakeControl {
            expected_revision: checked(v.expected_revision)?,
        },
        Wire::ReleaseControl(v) => BrowserOperation::ReleaseControl {
            controller_id: v.controller_id,
            expected_revision: checked(v.expected_revision)?,
        },
        Wire::OpenLocal(_) => BrowserOperation::OpenLocal,
    };
    Ok(BrowserRequest {
        node_id: None,
        operation,
    })
}
fn checked(value: u64) -> TalosResult<u64> {
    if value > 9_007_199_254_740_991 {
        Err(invalid())
    } else {
        Ok(value)
    }
}

pub fn encode_browser_result(value: BrowserSnapshot) -> TalosResult<pb::BrowserSnapshot> {
    use pb::browser_control_owner::Value as Owner;
    use pb::browser_display_capability::Value as Display;
    use pb::browser_readiness::Value as Ready;
    Ok(pb::BrowserSnapshot {
        enabled: value.enabled,
        host: host_to(value.host),
        selected_target_id: value.selected_target_id,
        targets: value
            .targets
            .into_iter()
            .map(|v| pb::BrowserTarget {
                target_id: v.target_id,
                display_name: v.display_name,
                kind: target_to(v.kind),
                local_open: local_to(v.local_open),
                readiness: Some(pb::BrowserReadiness {
                    value: Some(match v.readiness {
                        BrowserReadiness::Stopped => Ready::Stopped(pb::Empty {}),
                        BrowserReadiness::Starting => Ready::Starting(pb::Empty {}),
                        BrowserReadiness::Ready => Ready::Ready(pb::Empty {}),
                        BrowserReadiness::NeedsConnection => Ready::NeedsConnection(pb::Empty {}),
                        BrowserReadiness::Unavailable { error } => {
                            Ready::Unavailable(error_pb::encode_api_error(error))
                        }
                    }),
                }),
            })
            .collect(),
        display: Some(pb::BrowserDisplayCapability {
            value: Some(match value.display {
                BrowserDisplayCapability::DockerDesktop { target_id } => {
                    Display::DockerDesktop(target_id)
                }
                BrowserDisplayCapability::Unavailable { reason } => {
                    Display::Unavailable(display_reason_to(reason))
                }
            }),
        }),
        control: Some(pb::BrowserControlState {
            generation: value.control.generation,
            revision: value.control.revision,
            epoch: value.control.epoch,
            owner: Some(pb::BrowserControlOwner {
                value: Some(match value.control.owner {
                    BrowserControlOwner::Idle => Owner::Idle(pb::Empty {}),
                    BrowserControlOwner::Agent {
                        session_id,
                        turn_id,
                    } => Owner::Agent(pb::BrowserAgentOwner {
                        session_id,
                        turn_id,
                    }),
                    BrowserControlOwner::HandingOver { controller_id } => {
                        Owner::HandingOver(controller_id)
                    }
                    BrowserControlOwner::Human { controller_id } => Owner::Human(controller_id),
                    BrowserControlOwner::Suspended { reason } => {
                        Owner::Suspended(suspension_to(reason))
                    }
                }),
            }),
        }),
    })
}
pub fn decode_browser_result(value: pb::BrowserSnapshot) -> TalosResult<BrowserSnapshot> {
    use pb::browser_control_owner::Value as Owner;
    use pb::browser_display_capability::Value as Display;
    use pb::browser_readiness::Value as Ready;
    let control = value.control.ok_or_else(invalid)?;
    Ok(BrowserSnapshot {
        enabled: value.enabled,
        host: host_from(value.host)?,
        selected_target_id: value.selected_target_id,
        targets: value
            .targets
            .into_iter()
            .map(|v| {
                Ok(BrowserTarget {
                    target_id: v.target_id,
                    display_name: v.display_name,
                    kind: target_from(v.kind)?,
                    local_open: local_from(v.local_open)?,
                    readiness: match v.readiness.and_then(|v| v.value).ok_or_else(invalid)? {
                        Ready::Stopped(_) => BrowserReadiness::Stopped,
                        Ready::Starting(_) => BrowserReadiness::Starting,
                        Ready::Ready(_) => BrowserReadiness::Ready,
                        Ready::NeedsConnection(_) => BrowserReadiness::NeedsConnection,
                        Ready::Unavailable(error) => BrowserReadiness::Unavailable {
                            error: error_pb::decode_api_error(error)?,
                        },
                    },
                })
            })
            .collect::<TalosResult<Vec<_>>>()?,
        display: match value.display.and_then(|v| v.value).ok_or_else(invalid)? {
            Display::DockerDesktop(target_id) => {
                BrowserDisplayCapability::DockerDesktop { target_id }
            }
            Display::Unavailable(reason) => BrowserDisplayCapability::Unavailable {
                reason: display_reason_from(reason)?,
            },
        },
        control: BrowserControlState {
            generation: control.generation,
            revision: checked(control.revision)?,
            epoch: checked(control.epoch)?,
            owner: match control.owner.and_then(|v| v.value).ok_or_else(invalid)? {
                Owner::Idle(_) => BrowserControlOwner::Idle,
                Owner::Agent(v) => BrowserControlOwner::Agent {
                    session_id: v.session_id,
                    turn_id: v.turn_id,
                },
                Owner::HandingOver(controller_id) => {
                    BrowserControlOwner::HandingOver { controller_id }
                }
                Owner::Human(controller_id) => BrowserControlOwner::Human { controller_id },
                Owner::Suspended(reason) => BrowserControlOwner::Suspended {
                    reason: suspension_from(reason)?,
                },
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_wire_preserves_capabilities_control_and_typed_unavailability() {
        let snapshot = BrowserSnapshot {
            enabled: true,
            host: BrowserHostKind::Desktop,
            selected_target_id: Some("managed".into()),
            targets: vec![BrowserTarget {
                target_id: "managed".into(),
                display_name: "Talos browser".into(),
                kind: BrowserTargetKind::Managed,
                readiness: BrowserReadiness::Unavailable {
                    error: talos_error::ApiError::with_message(
                        talos_error::ErrorCode::ServiceUnavailable,
                        "Browser host is disconnected.",
                    ),
                },
                local_open: BrowserLocalOpenCapability::DesktopOwned,
            }],
            display: BrowserDisplayCapability::Unavailable {
                reason: BrowserDisplayUnavailableReason::NativeHost,
            },
            control: BrowserControlState {
                generation: "generation".into(),
                revision: 3,
                epoch: 2,
                owner: BrowserControlOwner::Suspended {
                    reason: BrowserSuspensionReason::WorkerRestarted,
                },
            },
        };
        assert_eq!(
            decode_browser_result(encode_browser_result(snapshot.clone()).unwrap()).unwrap(),
            snapshot
        );
        for operation in [
            BrowserOperation::Read,
            BrowserOperation::SelectTarget {
                target_id: "connected".into(),
                expected_revision: 3,
            },
            BrowserOperation::Pause {
                expected_revision: 3,
            },
            BrowserOperation::TakeControl {
                expected_revision: 3,
            },
            BrowserOperation::ReleaseControl {
                controller_id: "controller".into(),
                expected_revision: 3,
            },
            BrowserOperation::OpenLocal,
        ] {
            let request = BrowserRequest {
                node_id: None,
                operation,
            };
            assert_eq!(
                decode_browser_params(Some(encode_browser_params(request.clone()).unwrap()))
                    .unwrap(),
                request
            );
        }
        assert!(decode_browser_params(None).is_err());
        assert!(decode_browser_params(Some(pb::BrowserParams {
            operation: Some(pb::browser_params::Operation::Pause(
                pb::BrowserExpectedRevision {
                    expected_revision: u64::MAX
                }
            ))
        }))
        .is_err());
    }
}
