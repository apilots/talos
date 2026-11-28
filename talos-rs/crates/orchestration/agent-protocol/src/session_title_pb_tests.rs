use super::*;
#[test]
fn retained_title_preserves_actual_worker_model_and_usage() {
    let result = AgentSessionTitleResult {
        outcome: AgentSessionTitleOutcome::Generated {
            title: "Worker generated title".into(),
        },
        model_usage: Some(crate::AgentMemoryModelUsage {
            request_id: "worker-call".into(),
            model_ref: talos_types::ModelRef {
                provider: "target".into(),
                id: "inner".into(),
            },
            usage: talos_types::TokenUsage::default(),
            completed_at: 42,
            timezone: "Asia/Shanghai".into(),
        }),
    };
    assert_eq!(
        decode_result(encode_result(result.clone())).unwrap(),
        result
    );
}
