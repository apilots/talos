use prost::Message;
use talos_types::*;

use super::*;

fn request(action: AutomationAction) -> AutomationModelRequest {
    AutomationModelRequest {
        caller: AgentCaller {
            session_id: "session".into(),
            turn_id: "turn".into(),
            agent_id: "agent".into(),
        },
        call_id: "runtime-tool-call".into(),
        action,
    }
}

fn actions() -> Vec<AutomationAction> {
    let goals = [
        GoalAction::Create {
            objective: "Fix tests".into(),
            token_budget: Some(0),
        },
        GoalAction::Read {},
        GoalAction::Edit {
            goal_id: "goal".into(),
            expected_revision: 2,
            objective: "Fix lint".into(),
        },
        GoalAction::Pause {
            goal_id: "goal".into(),
        },
        GoalAction::Resume {
            goal_id: "goal".into(),
        },
        GoalAction::Clear {
            goal_id: "goal".into(),
        },
        GoalAction::Complete {
            goal_id: "goal".into(),
            objective_revision: 3,
            evidence: "Tests passed".into(),
        },
    ]
    .map(|action| AutomationAction::Goal { action });
    let loops = [
        LoopAction::Create {
            prompt: "Check CI".into(),
            schedule: LoopSchedule::ModelDecided {},
        },
        LoopAction::Create {
            prompt: "Check CI".into(),
            schedule: LoopSchedule::FixedInterval { minutes: 5 },
        },
        LoopAction::List {},
        LoopAction::Edit {
            loop_id: "loop".into(),
            expected_revision: 4,
            prompt: "Check deploy".into(),
            schedule: LoopSchedule::FixedInterval { minutes: 1 },
        },
        LoopAction::Stop {
            loop_id: "loop".into(),
        },
        LoopAction::Clear {
            loop_id: "loop".into(),
        },
        LoopAction::Decide {
            loop_id: "loop".into(),
            config_revision: 4,
            decision: LoopWakeDecision::Continue {
                delay_minutes: 2,
                reason: "CI running".into(),
            },
        },
        LoopAction::Decide {
            loop_id: "loop".into(),
            config_revision: 4,
            decision: LoopWakeDecision::Stop {
                reason: "CI passed".into(),
            },
        },
    ]
    .map(|action| AutomationAction::Loop { action });
    goals.into_iter().chain(loops).collect()
}

fn result() -> AutomationControlResult {
    AutomationControlResult {
        created_object: None,
        effect: AutomationEffect::PendingSettlement,
        bound_origin: Some(AutomationTurnOrigin::Loop {
            loop_id: "loop".into(),
            occurrence_id: "occurrence".into(),
            config_revision: 4,
        }),
        snapshot: SessionAutomationSnapshot {
            session_id: "session".into(),
            goal: Some(SessionGoal {
                id: "goal".into(),
                session_id: "session".into(),
                objective: "Fix tests".into(),
                objective_revision: 3,
                status: GoalStatus::Paused,
                pause_reason: Some(GoalPauseReason::WaitingForUser),
                reason: Some("Approval needed".into()),
                token_budget: Some(0),
                tokens_used: u64::MAX,
                latest_turn_id: Some("turn".into()),
                completion_evidence: Some("Checked".into()),
                created_at: 1,
                updated_at: 2,
            }),
            loops: vec![SessionLoop {
                id: "loop".into(),
                session_id: "session".into(),
                prompt: "Check CI".into(),
                schedule: LoopSchedule::ModelDecided {},
                config_revision: 4,
                status: LoopStatus::Stopped,
                stop_reason: Some(LoopStopReason::UserStopped),
                reason: Some("Stopped".into()),
                next_due_at: None,
                latest_turn_id: None,
                created_at: 1,
                updated_at: 2,
            }],
        },
    }
}

#[test]
fn every_closed_action_and_runtime_identity_survive_real_protobuf_bytes() {
    for action in actions() {
        let original = request(action);
        let encoded = encode_model_params(original.clone())
            .unwrap()
            .encode_to_vec();
        let decoded = pb::AutomationModelParams::decode(encoded.as_slice()).unwrap();
        assert_eq!(decode_model_params(Some(decoded)).unwrap(), original);
    }
}

#[test]
fn result_preserves_bound_origin_effect_optional_values_and_all_snapshot_fields() {
    for bound_origin in [
        None,
        result().bound_origin,
        Some(AutomationTurnOrigin::Goal {
            goal_id: "goal".into(),
            objective_revision: 9,
        }),
    ] {
        let original = AutomationControlResult {
            created_object: None,
            bound_origin,
            ..result()
        };
        let encoded = encode_response(Ok(original.clone())).encode_to_vec();
        let decoded = pb::ExecuteAutomationResponse::decode(encoded.as_slice()).unwrap();
        assert_eq!(decode_response(decoded).unwrap(), original);
    }
    let mut original = result();
    original.snapshot.goal.as_mut().unwrap().token_budget = None;
    original.snapshot.loops[0].next_due_at = Some(0);
    assert_eq!(
        decode_result(encode_result(original.clone())).unwrap(),
        original
    );
}

#[test]
fn status_reason_and_effect_variants_have_exhaustive_typed_roundtrips() {
    macro_rules! check {
        ($encode:ident, $decode:ident, $ty:ident, [$($variant:ident),+]) => {
            $(assert_eq!(facts::$decode(facts::$encode($ty::$variant)).unwrap(), $ty::$variant);)+
            assert!(facts::$decode(0).is_err());
            assert!(facts::$decode(999).is_err());
        };
    }
    check!(
        encode_effect,
        decode_effect,
        AutomationEffect,
        [Applied, PendingSettlement]
    );
    check!(
        encode_goal_status,
        decode_goal_status,
        GoalStatus,
        [Active, Paused, Complete, Cleared]
    );
    check!(
        encode_pause_reason,
        decode_pause_reason,
        GoalPauseReason,
        [
            UserPaused,
            UserCancelled,
            WaitingForUser,
            ExecutionFailed,
            BudgetExhausted,
            NoProgress
        ]
    );
    check!(
        encode_loop_status,
        decode_loop_status,
        LoopStatus,
        [Active, Stopped]
    );
    check!(
        encode_stop_reason,
        decode_stop_reason,
        LoopStopReason,
        [
            UserStopped,
            UserCancelled,
            ModelStopped,
            WaitingForUser,
            ExecutionFailed,
            MissingDecision,
            QueueFull
        ]
    );
}

#[test]
fn malformed_identity_and_missing_or_unknown_actions_are_rejected() {
    let valid = encode_model_params(request(AutomationAction::Goal {
        action: GoalAction::Read {},
    }))
    .unwrap();
    for mutate in [
        |value: &mut pb::AutomationModelParams| value.caller = None,
        |value: &mut pb::AutomationModelParams| value.caller.as_mut().unwrap().session_id.clear(),
        |value: &mut pb::AutomationModelParams| value.caller.as_mut().unwrap().turn_id.clear(),
        |value: &mut pb::AutomationModelParams| value.caller.as_mut().unwrap().agent_id.clear(),
        |value: &mut pb::AutomationModelParams| value.call_id = "  ".into(),
        |value: &mut pb::AutomationModelParams| value.action = None,
        |value: &mut pb::AutomationModelParams| value.action.as_mut().unwrap().target = None,
    ] {
        let mut value = valid.clone();
        mutate(&mut value);
        assert!(decode_model_params(Some(value))
            .unwrap_err()
            .is_code(ErrorCode::InvalidRequest));
    }
    let unknown = pb::AutomationAction::decode([0x9a, 0x06, 0x00].as_slice()).unwrap();
    assert!(actions::decode_action(unknown).is_err());
    let mut request = request(AutomationAction::Goal {
        action: GoalAction::Read {},
    });
    request.call_id.clear();
    assert!(encode_model_params(request).is_err());
}

#[test]
fn invalid_schedules_missing_declarations_and_cross_session_facts_never_fallback() {
    assert!(actions::decode_schedule(None).is_err());
    assert!(actions::decode_schedule(Some(pb::AutomationLoopSchedule {
        schedule: Some(pb::automation_loop_schedule::Schedule::FixedMinutes(0))
    }))
    .is_err());
    let action = AutomationAction::Loop {
        action: LoopAction::Decide {
            loop_id: "loop".into(),
            config_revision: 4,
            decision: LoopWakeDecision::Continue {
                delay_minutes: 0,
                reason: "Too soon".into(),
            },
        },
    };
    assert!(decode_model_params(Some(encode_model_params(request(action)).unwrap())).is_err());
    let mut declaration = encode_model_params(request(AutomationAction::Loop {
        action: LoopAction::Decide {
            loop_id: "loop".into(),
            config_revision: 4,
            decision: LoopWakeDecision::Stop {
                reason: "Done".into(),
            },
        },
    }))
    .unwrap();
    let Some(pb::automation_action::Target::LoopTask(action)) = declaration
        .action
        .as_mut()
        .and_then(|value| value.target.as_mut())
    else {
        panic!("Loop action");
    };
    let Some(pb::automation_loop_action::Action::Decide(value)) = action.action.as_mut() else {
        panic!("decision action");
    };
    value.decision = None;
    assert!(decode_model_params(Some(declaration)).is_err());
    let mut facts = encode_result(result());
    facts.snapshot.as_mut().unwrap().loops[0].session_id = "other-session".into();
    assert!(decode_result(facts).is_err());
    let mut facts = encode_result(result());
    facts.bound_origin.as_mut().unwrap().origin = None;
    assert!(decode_result(facts).is_err());
    assert!(decode_response(pb::ExecuteAutomationResponse { result: None }).is_err());
}

#[test]
fn original_typed_failure_and_resource_caller_survive_the_same_carrier() {
    let error = TalosError::new(ErrorCode::PermissionDenied, "No explicit activation intent");
    let decoded = decode_response(encode_response(Err(error.clone()))).unwrap_err();
    assert_eq!(decoded.to_api_error(), error.to_api_error());
    let scope = pb::CoordinatorCallContext {
        connection_id: "connection".into(),
        rpc_call_id: 12,
        turn_id: Some("turn".into()),
        ..Default::default()
    };
    let mut call =
        pb::worker_rpc_call::Request::ExecuteAutomation(Box::new(pb::ExecuteAutomationRequest {
            auth: Some(pb::NodeAuth {
                node_id: "node".into(),
                token: "old-token".into(),
            }),
            params: Some(
                encode_model_params(request(AutomationAction::Loop {
                    action: LoopAction::List {},
                }))
                .unwrap(),
            ),
            caller: Some(scope.clone()),
        }));
    let new_auth = pb::NodeAuth {
        node_id: "replacement-node".into(),
        token: "replacement-token".into(),
    };
    crate::connection_pb::replace_auth(&mut call, &new_auth);
    assert_eq!(crate::connection_pb::auth(&call), Some(&new_auth));
    assert_eq!(crate::connection_pb::resource_caller(&call), Some(&scope));
    assert!(crate::connection_pb::execution_scope(&call).is_none());
    assert!(!crate::connection_pb::is_management_peer_call(&call));
    let pb::worker_rpc_call::Request::ExecuteAutomation(value) = call else {
        panic!("typed case");
    };
    assert_eq!(value.params.unwrap().call_id, "runtime-tool-call");
}

#[test]
fn created_object_round_trip_is_exact_and_rejects_missing_or_empty_identity() {
    for created in [
        None,
        Some(AutomationObjectRef::Goal {
            goal_id: "goal-created".into(),
        }),
        Some(AutomationObjectRef::Loop {
            loop_id: "loop-created".into(),
        }),
    ] {
        let original = AutomationControlResult {
            created_object: created,
            ..result()
        };
        assert_eq!(
            decode_result(encode_result(original.clone())).unwrap(),
            original
        );
    }
    for value in [
        None,
        Some(pb::automation_object_ref::Value::GoalId(String::new())),
    ] {
        let mut invalid = encode_result(result());
        invalid.created_object = Some(pb::AutomationObjectRef { value });
        assert!(decode_result(invalid).is_err());
    }
}
