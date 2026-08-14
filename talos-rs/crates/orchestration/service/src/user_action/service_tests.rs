use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use talos_store::session::UserActionStore;
use talos_types::{
    PendingUserAction, QuestionAnswer, QuestionOption, QuestionParams, UserActionParams,
};

use super::*;

#[derive(Default)]
struct MemoryUserActionStore {
    record: Mutex<Option<UserActionRecord>>,
    concurrent_get_barrier: Option<Arc<tokio::sync::Barrier>>,
    pending_gets: AtomicUsize,
}

impl MemoryUserActionStore {
    fn with_record(record: UserActionRecord) -> Self {
        Self {
            record: Mutex::new(Some(record)),
            concurrent_get_barrier: None,
            pending_gets: AtomicUsize::new(0),
        }
    }

    fn with_concurrent_record(record: UserActionRecord) -> Self {
        Self {
            record: Mutex::new(Some(record)),
            concurrent_get_barrier: Some(Arc::new(tokio::sync::Barrier::new(2))),
            pending_gets: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl UserActionStore for MemoryUserActionStore {
    async fn create(&self, record: UserActionRecord) -> TalosResult<UserActionRecord> {
        *self.record.lock().expect("store lock") = Some(record.clone());
        Ok(record)
    }

    async fn get(
        &self,
        session_id: &str,
        turn_id: &str,
        action_id: &str,
    ) -> TalosResult<Option<UserActionRecord>> {
        let record = self
            .record
            .lock()
            .expect("store lock")
            .clone()
            .filter(|record| {
                record.session_id == session_id
                    && record.turn_id == turn_id
                    && record.action.action_id == action_id
            });
        if record
            .as_ref()
            .is_some_and(|record| record.action.state == UserActionState::Pending)
            && self.pending_gets.fetch_add(1, Ordering::SeqCst) < 2
        {
            if let Some(barrier) = &self.concurrent_get_barrier {
                barrier.wait().await;
            }
        }
        Ok(record)
    }

    async fn compare_and_set(
        &self,
        expected: &UserActionRecord,
        next: UserActionRecord,
    ) -> TalosResult<UserActionRecord> {
        let mut current = self.record.lock().expect("store lock");
        if current.as_ref() != Some(expected) {
            return Err(TalosError::Conflict("stale user action".to_string()));
        }
        *current = Some(next.clone());
        Ok(next)
    }
}

fn pending_question() -> UserActionRecord {
    UserActionRecord {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        action: PendingUserAction {
            action_id: "action-1".to_string(),
            call_id: "call-1".to_string(),
            item_id: Some("item-1".to_string()),
            state: UserActionState::Pending,
            params: UserActionParams::Question(QuestionParams {
                header: "Mode".to_string(),
                question: "Choose a mode".to_string(),
                options: vec![
                    QuestionOption {
                        id: "plan".to_string(),
                        label: "Plan".to_string(),
                        description: "Plan first".to_string(),
                    },
                    QuestionOption {
                        id: "direct".to_string(),
                        label: "Direct".to_string(),
                        description: "Execute now".to_string(),
                    },
                ],
            }),
        },
        response: None,
        version: 0,
        updated_at: 1,
    }
}

fn selected_plan() -> UserActionResponse {
    UserActionResponse::Question(QuestionAnswer::SelectedOption {
        option_id: "plan".to_string(),
    })
}

#[tokio::test]
async fn accepted_response_advances_through_resolved() {
    let service = UserActionService::new(Arc::new(MemoryUserActionStore::with_record(
        pending_question(),
    )));

    let accepted = service
        .accept("session-1", "turn-1", "action-1", selected_plan())
        .await
        .expect("response should be accepted");
    assert!(accepted.should_resume);
    assert_eq!(
        accepted.record.action.state,
        UserActionState::ResponseAccepted
    );
    assert_eq!(accepted.record.version, 1);

    let resuming = service
        .begin_resume(accepted.record)
        .await
        .expect("resume should begin");
    let resolved = service
        .resolve(resuming)
        .await
        .expect("action should resolve");
    assert_eq!(resolved.action.state, UserActionState::Resolved);
    assert_eq!(resolved.version, 3);
}

#[tokio::test]
async fn duplicate_response_is_idempotent_and_conflicting_response_fails() {
    let service = UserActionService::new(Arc::new(MemoryUserActionStore::with_record(
        pending_question(),
    )));
    let accepted = service
        .accept("session-1", "turn-1", "action-1", selected_plan())
        .await
        .expect("response should be accepted");
    let duplicate = service
        .accept("session-1", "turn-1", "action-1", selected_plan())
        .await
        .expect("same response should be idempotent");
    assert_eq!(duplicate.record, accepted.record);
    assert!(!duplicate.should_resume);

    let conflict = service
        .accept(
            "session-1",
            "turn-1",
            "action-1",
            UserActionResponse::Question(QuestionAnswer::SelectedOption {
                option_id: "direct".to_string(),
            }),
        )
        .await;
    assert!(matches!(conflict, Err(TalosError::Conflict(_))));
}

#[tokio::test]
async fn concurrent_identical_responses_start_only_one_resume() {
    let service = UserActionService::new(Arc::new(MemoryUserActionStore::with_concurrent_record(
        pending_question(),
    )));

    let (first, second) = tokio::join!(
        service.accept("session-1", "turn-1", "action-1", selected_plan()),
        service.accept("session-1", "turn-1", "action-1", selected_plan()),
    );
    let accepted = [
        first.expect("first response should resolve"),
        second.expect("second response should resolve"),
    ];

    assert_eq!(
        accepted
            .iter()
            .filter(|result| result.should_resume)
            .count(),
        1
    );
    assert_eq!(accepted[0].record, accepted[1].record);
    assert_eq!(
        accepted[0].record.action.state,
        UserActionState::ResponseAccepted
    );
}

#[tokio::test]
async fn invalid_question_answers_fail_without_mutation() {
    let store = Arc::new(MemoryUserActionStore::with_record(pending_question()));
    let service = UserActionService::new(store.clone());
    for response in [
        UserActionResponse::Question(QuestionAnswer::SelectedOption {
            option_id: "missing".to_string(),
        }),
        UserActionResponse::Question(QuestionAnswer::FreeForm {
            text: "   ".to_string(),
        }),
    ] {
        assert!(service
            .accept("session-1", "turn-1", "action-1", response)
            .await
            .is_err());
    }
    assert_eq!(
        store
            .get("session-1", "turn-1", "action-1")
            .await
            .expect("record lookup")
            .expect("record"),
        pending_question()
    );
}

#[tokio::test]
async fn resume_failure_retries_once_then_becomes_terminal() {
    let service = UserActionService::new(Arc::new(MemoryUserActionStore::with_record(
        pending_question(),
    )));
    let accepted = service
        .accept("session-1", "turn-1", "action-1", selected_plan())
        .await
        .expect("response should be accepted");
    let first_resuming = service
        .begin_resume(accepted.record)
        .await
        .expect("first resume");
    let retry = service
        .retry_or_fail(first_resuming)
        .await
        .expect("first failure should retry");
    assert_eq!(retry.action.state, UserActionState::ResponseAccepted);
    let second_resuming = service.begin_resume(retry).await.expect("second resume");
    let failed = service
        .retry_or_fail(second_resuming)
        .await
        .expect("second failure should terminate");
    assert_eq!(failed.action.state, UserActionState::ResumeFailed);
}

#[tokio::test]
async fn recovery_resumes_accepted_and_marks_resuming_execution_unknown() {
    let store = Arc::new(MemoryUserActionStore::with_record(pending_question()));
    let service = UserActionService::new(store);
    let accepted = service
        .accept("session-1", "turn-1", "action-1", selected_plan())
        .await
        .expect("response should be accepted");
    let recovered = service
        .recover(accepted.record.clone())
        .await
        .expect("accepted response should recover");
    assert!(recovered.should_resume);
    assert_eq!(recovered.record, accepted.record);

    let resuming = service
        .begin_resume(recovered.record)
        .await
        .expect("resume should begin");
    let unknown = service
        .recover(resuming)
        .await
        .expect("uncertain dispatch should become terminal");
    assert!(!unknown.should_resume);
    assert_eq!(
        unknown.record.action.state,
        UserActionState::ExecutionUnknown
    );
}
