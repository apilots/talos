use super::*;
use talos_agent::{AgentFuture, AgentResult, SessionHistoryReadParams};
use talos_llm_provider::{ModelBlockDelta, ModelBlockStarted, ModelBlockStreamEvent};
#[path = "trajectory_attachment_tests.rs"]
mod attachment_tests;
#[path = "trajectory_completeness_tests.rs"]
mod completeness_tests;

#[derive(Default)]
struct History {
    records: Mutex<Vec<SessionHistorySupplement>>,
}
impl SessionHistoryReader for History {
    fn read(
        &self,
        _: SessionHistoryReadParams,
    ) -> AgentFuture<'_, AgentResult<Vec<talos_agent_api::SessionHistoryTurn>>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn append_supplement(
        &self,
        value: SessionHistorySupplement,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async move {
            self.records.lock().unwrap().push(value);
            Ok(())
        })
    }
}
fn setup() -> (History, Arc<Capture>) {
    (
        History::default(),
        Capture::new(AgentTurnRef::new("s", "t"), false, Some("node".into())),
    )
}
fn request() -> ChatRequest {
    ChatRequest {
        model: "bound-model".into(),
        messages: vec![ChatMessage::system("system"), ChatMessage::user("你好")],
        temperature: Some(0.25),
        ..ChatRequest::new_user("", "")
    }
}
fn done(observation: &dyn KernelModelObservation) {
    observation.model_event(&ModelBlockStreamEvent::Done {
        finish_reason: Some("stop".into()),
        usage: Some(talos_llm_provider::Usage {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
            ..Default::default()
        }),
    });
}

#[tokio::test]
async fn invocation_handoff_saves_each_response_once_and_reuses_equal_options() {
    let (history, capture) = setup();
    let first = "x".repeat(1024 * 1024);
    let second = "y".repeat(2 * 1024 * 1024);
    for _ in 0..3 {
        let observation = capture
            .recorder()
            .start_model(KernelObservedPurpose::Working, "configured", &request())
            .unwrap();
        observation.model_event(&ModelBlockStreamEvent::BlockStarted(
            ModelBlockStarted::assistant_text(None, talos_llm_provider::ModelOutputPhase::Unknown),
        ));
        for text in [first.as_str(), second.as_str()] {
            observation.model_event(&ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".into(),
                delta: ModelBlockDelta::AssistantText { delta: text.into() },
            });
        }
        done(&*observation);
        observation.finished(None);
    }
    assert!(history.records.lock().unwrap().is_empty());
    capture.flush(&history).await;
    let records = history.records.lock().unwrap();
    assert_eq!(records.len(), 1);
    let value = &records[0];
    assert_eq!(value.requests.len(), 3);
    assert_eq!(value.requests[0].tools, value.requests[1].tools);
    assert_eq!(value.requests[0].parameters, value.requests[1].parameters);
    assert_eq!(
        value
            .payloads
            .values()
            .filter(|p| matches!(p, TrajectoryPayload::Response { .. }))
            .count(),
        3
    );
    for detail in &value.requests {
        let reference = detail.outputs[0].response.as_ref().unwrap();
        assert_eq!(reference.complete, Some(true));
        let TrajectoryPayload::Response { parts } = &value.payloads[&reference.payload_id] else {
            panic!("response payload expected")
        };
        assert_eq!(
            parts,
            &[ResponsePart::Text {
                text: format!("{first}{second}")
            }]
        );
    }
    assert!(value
        .requests
        .iter()
        .all(|r| r.request.call_records[0].observed_state == ModelCallState::Completed));
    assert_eq!(value.clock_segments[0].source_node_id, "node");
}
#[tokio::test]
async fn refused_input_body_still_records_real_terminal_usage_and_timing() {
    let (history, capture) = setup();
    let mut request = request();
    request.messages[0].content = "x".repeat(4 * 1024 * 1024 + 1);
    let observation = capture
        .recorder()
        .start_model(KernelObservedPurpose::Working, "c", &request)
        .unwrap();
    done(&*observation);
    observation.finished(None);
    capture.flush(&history).await;
    let records = history.records.lock().unwrap();
    let detail = &records[0].requests[0];
    assert_eq!(detail.prompt, None);
    assert!(detail.tools.is_some());
    assert!(detail.parameters.is_some());
    assert_eq!(
        detail.request.call_records[0]
            .metrics
            .as_ref()
            .unwrap()
            .usage
            .as_ref()
            .unwrap()
            .total_tokens,
        12
    );
    assert!(detail.request.call_records[0]
        .metrics
        .as_ref()
        .unwrap()
        .finished_at_ms
        .is_some());
}
#[tokio::test]
async fn rendered_prompt_origins_reach_the_final_snapshot_without_guessing_history() {
    let mut config = talos_prompt::PromptBuildConfig::default();
    config.prompt.bootstrap.enabled = false;
    let input = "<system>Skills and runtime text supplied by the user</system>";
    let assembly = talos_prompt::PromptBuilder::new()
        .config(config)
        .planning_instruction(Some("inspect current context".into()))
        .user_input(input)
        .assemble()
        .await
        .unwrap();
    let mut request = request();
    request.messages = talos_prompt::render_prompt_assembly(&assembly);
    request
        .messages
        .push(ChatMessage::user("restored context without origin"));
    let (history, capture) = setup();
    let observation = capture
        .recorder()
        .start_model(KernelObservedPurpose::Working, "c", &request)
        .unwrap();
    observation.interrupted();
    capture.flush(&history).await;
    let records = history.records.lock().unwrap();
    let TrajectoryPayload::Prompt { messages } = records[0]
        .payloads
        .values()
        .find(|p| matches!(p, TrajectoryPayload::Prompt { .. }))
        .unwrap()
    else {
        panic!()
    };
    let origins = messages
        .iter()
        .map(|m| {
            m.input_origin
                .as_ref()
                .map(|o| (o.kind, o.source, o.admission))
        })
        .collect::<Vec<_>>();
    use SnapshotInputAdmission::Unknown;
    use SnapshotInputKind::{Context, System, User};
    assert_eq!(
        origins,
        vec![
            Some((System, SnapshotInputSource::Prompt, Unknown)),
            Some((Context, SnapshotInputSource::Mixed, Unknown)),
            Some((Context, SnapshotInputSource::Runtime, Unknown)),
            Some((Context, SnapshotInputSource::Runtime, Unknown)),
            Some((User, SnapshotInputSource::User, Unknown)),
            None
        ]
    );
    assert_eq!(messages[4].content, input);
}
