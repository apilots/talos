use super::*;
use crate::continuation::KernelProviderContinuation;
use std::sync::atomic::AtomicBool;
use talos_agent::{AgentEventPipeline, AgentEventReducer, AgentRunCheckpoint};
use talos_agent_api::{AgentPendingInput, AgentToolCall, AgentToolCallArguments, AgentTurnStarted};
use talos_agent_kernel::{KernelTranscriptUserActionRequestPayload, UserInput};
use talos_config::ModelInputKind;
use talos_llm_provider::{FunctionCall, Role};
use talos_types::{
    QuestionAnswer, QuestionOption, QuestionParams, TurnExecutionMetrics, UserActionParams,
    UserActionResponse,
};

const QUESTION_TOOL: &str = "AskUserQuestion";
const FOLLOW_UP: &str = "Review the attached note after answering.";

#[tokio::test]
async fn persisted_input_reaches_model_after_both_questions() {
    exercise_resume(true).await;
}

#[tokio::test]
async fn missing_attachment_fails_only_after_the_original_batch() {
    exercise_resume(false).await;
}

async fn exercise_resume(attachment_available: bool) {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let mut context = context(identity.clone(), root);
    context.model.model.input = vec![ModelInputKind::File];
    let store = Arc::new(FileKernelSessionArtifactStore);
    let reader = Arc::new(TestFileReader(AtomicBool::new(false)));
    let model = Arc::new(TextProvider {
        requests: Some(Mutex::new(Vec::new())),
        ..Default::default()
    });
    let mut params = provider_params(root, store.clone());
    params.attachment_reader = reader.clone();
    params.llm_provider_router = Arc::new(StaticRouter(model.clone()));
    params
        .tool_registry
        .register(Arc::new(talos_tools::AskUserQuestionTool));
    let provider = KernelAgentProvider::new(params).unwrap();

    let (checkpoint, reference) = persist_first_question(root, &identity, store.as_ref()).await;
    let mut reducer = AgentEventReducer::from_checkpoint(identity, checkpoint.clone()).unwrap();
    let (events, waiting) = resume(&provider, &context, checkpoint, reference, 2).await;
    for event in events {
        reducer.apply(event).unwrap();
    }
    let AgentRunSettlement::WaitingForInput {
        event,
        continuation,
        ..
    } = waiting
    else {
        panic!("the second question must remain available even when its deferred attachment is missing");
    };
    let AgentEventPayload::WaitingForInput(AgentPendingInput::UserAction { action }) =
        &event.payload
    else {
        panic!("expected second question");
    };
    assert_eq!(action.call_id, "call-2");
    assert!(model.requests.as_ref().unwrap().lock().unwrap().is_empty());
    reducer.apply(event).unwrap();

    reader.0.store(attachment_available, Ordering::Relaxed);
    let (_, finished) = resume(&provider, &context, reducer.checkpoint(), continuation, 3).await;
    let AgentRunSettlement::Finished {
        result, messages, ..
    } = finished
    else {
        panic!("the second answer must finish this execution");
    };
    let resolved_calls: Vec<_> = messages
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| match &block.content {
            AgentMessageBlockContent::ToolResult(result) => Some(result.call_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(resolved_calls, ["call-1", "call-2"]);
    let requests = model.requests.as_ref().unwrap().lock().unwrap();
    if !attachment_available {
        assert!(result
            .unwrap_err()
            .is_code(talos_error::ErrorCode::ResourceNotFound));
        assert!(requests.is_empty());
        return;
    }
    result.unwrap();
    let [request] = requests.as_slice() else {
        panic!("expected one resumed model request")
    };
    let messages = &request.messages;
    let last_result = messages
        .iter()
        .position(|message| message.tool_call_id.as_deref() == Some("call-2"))
        .unwrap();
    assert_eq!(
        messages[last_result - 1].tool_call_id.as_deref(),
        Some("call-1")
    );
    assert_eq!(
        (
            messages[last_result + 1].role,
            messages[last_result + 1].content.as_str()
        ),
        (Role::User, FOLLOW_UP)
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.content == FOLLOW_UP)
            .count(),
        1
    );
    let [attachment] = messages[last_result + 1].attachments.as_deref().unwrap() else {
        panic!("expected one resolved PDF");
    };
    assert_eq!(attachment.file_id.as_deref(), Some("file-1"));
    assert_eq!(attachment.bytes.as_ref(), b"%PDF");
}

async fn resume(
    provider: &KernelAgentProvider,
    context: &AgentContext,
    checkpoint: AgentRunCheckpoint,
    continuation: talos_agent_api::AgentContinuationRef,
    message_number: u32,
) -> (Vec<talos_agent_api::AgentEvent>, AgentRunSettlement) {
    let action = KernelProviderContinuation::decode(&continuation).unwrap();
    let mut answer = user_message();
    answer.message_id = format!("answer-{message_number}");
    answer.blocks[0].block_id = format!("answer-block-{message_number}");
    answer.blocks[0].content =
        AgentMessageBlockContent::UserContent(AgentUserContent::UserActionResponse {
            action_id: action.action_id,
            response: UserActionResponse::Question(QuestionAnswer::SelectedOption {
                option_id: "review".into(),
            }),
        });
    let harness = create_agent_harness("agent-1".into(), provider.clone());
    let mut execution = harness
        .start(AgentRunParams {
            context: context.clone(),
            input: answer,
            checkpoint: Some(checkpoint),
            continuation: Some(continuation),
        })
        .await
        .unwrap();
    execution.admission.permit.consume().await.unwrap();
    let mut events = execution.admission.events;
    let (settlement, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(execution.settlement.recv(), async {
            while let Some(event) = execution.events.recv().await {
                events.push(event);
            }
        })
    })
    .await
    .expect("the resumed turn must settle");
    (events, settlement.unwrap())
}

async fn persist_first_question(
    root: &Path,
    identity: &AgentTurnRef,
    store: &FileKernelSessionArtifactStore,
) -> (AgentRunCheckpoint, talos_agent_api::AgentContinuationRef) {
    let (pipeline, sink, _events) = AgentEventPipeline::fresh(identity.clone());
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let message = sink.start_message(AgentMessageRole::Agent).await.unwrap();
    let params = QuestionParams {
        header: "Review".into(),
        question: "Should I review the note?".into(),
        options: [
            ("review", "Review", "Read the note"),
            ("skip", "Skip", "Leave the note unread"),
        ]
        .into_iter()
        .map(|(id, label, description)| QuestionOption {
            id: id.into(),
            label: label.into(),
            description: description.into(),
        })
        .collect(),
    };
    let arguments = serde_json::to_value(&params).unwrap();
    let calls =
        ["call-1", "call-2"].map(|id| FunctionCall::new(id, QUESTION_TOOL, arguments.clone()));
    let mut blocks = Vec::new();
    for call in &calls {
        blocks.push(
            sink.start_block(
                &message,
                AgentMessageBlockContent::ToolCall(AgentToolCall {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: AgentToolCallArguments::Complete {
                        value: arguments.clone().into(),
                    },
                    progress: vec![],
                }),
            )
            .await
            .unwrap(),
        );
    }
    let block_id = blocks[0].block_id().to_string();
    let action_id = "action-1";
    pipeline
        .record(AgentEventPayload::WaitingForInput(
            AgentPendingInput::UserAction {
                action: talos_types::PendingUserAction {
                    action_id: action_id.into(),
                    call_id: "call-1".into(),
                    item_id: Some(block_id.clone()),
                    state: talos_types::UserActionState::Pending,
                    params: UserActionParams::Question(params.clone()),
                },
            },
        ))
        .await
        .unwrap();

    let mut payload = KernelTranscriptUserActionRequestPayload {
        action_id: action_id.into(),
        call_id: "call-1".into(),
        tool_name: QUESTION_TOOL.into(),
        arguments: arguments.into(),
        params: UserActionParams::Question(params),
        sandbox_request_fingerprint: None,
        reasoning_continuation: None,
        tool_calls: calls.to_vec(),
        tool_messages: vec![],
        deferred_tool_calls: vec![calls[1].clone()],
        deferred_inputs: vec![UserInput {
            text: FOLLOW_UP.into(),
            file_ids: vec!["file-1".into()],
        }],
    };
    for _ in 0..2 {
        store
            .append_transcript_event(
                root,
                &KernelTranscriptEvent::new(
                    &identity.session_id,
                    &identity.turn_id,
                    ExecutionBranch::ReactiveLoop,
                    EventAuthor::System,
                    KernelTranscriptPayload::UserActionRequest {
                        payload: payload.clone(),
                    },
                ),
            )
            .await
            .unwrap();
        // A newer request with the same call ID cannot replace this action.
        payload.action_id = "other-action".into();
        payload.deferred_inputs.clear();
    }

    let reference = KernelProviderContinuation::new(
        action_id.into(),
        "call-1".into(),
        QUESTION_TOOL.into(),
        message.message_id().into(),
        block_id,
        TurnExecutionMetrics {
            model_rounds: 1,
            tool_calls: 2,
            ..Default::default()
        },
    )
    .encode()
    .unwrap();
    (pipeline.checkpoint().await, reference)
}

struct TestFileReader(AtomicBool);

impl AttachmentReader for TestFileReader {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>> {
        Box::pin(async move {
            if file_ids.is_empty() {
                return Ok(vec![]);
            }
            assert_eq!(scope.owner_user_id, "user-1");
            assert_eq!(file_ids, ["file-1"]);
            if !self.0.load(Ordering::Relaxed) {
                return Err(TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    "Test attachment is unavailable",
                ));
            }
            Ok(vec![ResolvedAttachment {
                file_id: "file-1".into(),
                filename: "note.pdf".into(),
                media_type: "application/pdf".into(),
                size_bytes: 4,
                bytes: b"%PDF"[..].into(),
            }])
        })
    }
}
