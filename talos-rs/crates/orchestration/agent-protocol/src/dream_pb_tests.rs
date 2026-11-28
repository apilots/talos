use super::*;

#[test]
fn worker_formation_preserves_cursor_failure_and_all_model_usage_fields() {
    let result = DreamMemoryResult {
        processed_until_seq: 17,
        next_cursor: DreamMemoryCursor {
            generation: 7,
            offset: 19,
        },
        formation: Some(DreamMemoryFormation {
            covers_until_seq: None,
            model_usage: crate::AgentMemoryModelUsage {
                request_id: "stable-worker-request".into(),
                model_ref: talos_types::ModelRef {
                    provider: "worker-provider".into(),
                    id: "worker-inner".into(),
                },
                usage: talos_types::TokenUsage {
                    input_tokens: 3,
                    output_tokens: 2,
                    total_tokens: 5,
                    reasoning_tokens: Some(1),
                    cache_creation_input_tokens: Some(2),
                    cache_read_input_tokens: Some(1),
                },
                completed_at: 1_800_000_000,
                timezone: "Pacific/Kiritimati".into(),
            },
            failure: Some(
                TalosError::new(
                    ErrorCode::ProviderUnavailable,
                    "Dream Memory formation could not commit its result.",
                )
                .to_api_error(),
            ),
        }),
    };
    assert_eq!(
        decode_result(encode_result(result.clone())).unwrap(),
        result
    );
}

#[test]
fn product_consumption_position_and_target_scope_round_trip() {
    let config = talos_config::Config::default();
    let params = DreamMemoryParams {
        session: crate::AgentSessionContext {
            agent_id: "worker-agent".into(),
            session_id: "session".into(),
            workspace: config
                .resolve_workspace(talos_config::DEFAULT_WORKSPACE_ID, "alice")
                .unwrap(),
        },
        after_cursor: Some(DreamMemoryCursor {
            generation: 2,
            offset: 7,
        }),
    };
    assert_eq!(
        decode_params(Some(encode_params(params.clone())), &config).unwrap(),
        params
    );
}
