use std::collections::HashSet;
use std::sync::{Arc, Barrier};

use talos_error::ErrorCode;
use talos_types::{
    AgentUserContent, AgentUserInput, MessageQueue, MessageQueueChangedNotification,
};

use super::*;

fn text_input(text: impl Into<String>) -> AgentUserInput {
    AgentUserInput {
        content: vec![AgentUserContent::Text { text: text.into() }],
    }
}

#[test]
fn add_preserves_fifo_and_enforces_capacity() {
    let service = SessionMessageQueueService::new(2).expect("queue service should initialize");

    let first = service
        .add("session-1", text_input("first"))
        .expect("first message should be added");
    let second = service
        .add("session-1", text_input("second"))
        .expect("second message should be added");
    let error = service
        .add("session-1", text_input("third"))
        .expect_err("queue beyond capacity should fail");

    assert_eq!(first.revision, 1);
    assert_eq!(second.revision, 2);
    assert_eq!(second.max_messages, 2);
    assert_eq!(second.messages[0].input, text_input("first"));
    assert_eq!(second.messages[1].input, text_input("second"));
    assert_eq!(error.error_code(), ErrorCode::MessageQueueFull);
    assert_eq!(
        service.list("session-1").expect("queue should list"),
        second
    );
}

#[test]
fn update_delete_and_reorder_return_complete_queue() {
    let service = SessionMessageQueueService::new(10).expect("queue service should initialize");
    let first = service
        .add("session-1", text_input("first"))
        .expect("first message should be added");
    let second = service
        .add("session-1", text_input("second"))
        .expect("second message should be added");
    let first_id = first.messages[0].id.clone();
    let second_id = second.messages[1].id.clone();

    let updated = service
        .update("session-1", &first_id, text_input("updated"))
        .expect("message should update");
    assert_eq!(updated.revision, 3);
    assert_eq!(updated.messages[0].id, first_id);
    assert_eq!(updated.messages[0].input, text_input("updated"));

    let reordered = service
        .reorder("session-1", &[second_id.clone(), first_id.clone()])
        .expect("complete message id permutation should reorder");
    assert_eq!(reordered.revision, 4);
    assert_eq!(reordered.messages[0].id, second_id);
    assert_eq!(reordered.messages[1].id, first_id);

    let deleted = service
        .delete("session-1", &second_id)
        .expect("message should delete");
    assert_eq!(deleted.revision, 5);
    assert_eq!(deleted.messages.len(), 1);
    assert_eq!(deleted.messages[0].id, first_id);
}

#[test]
fn reorder_rejects_incomplete_duplicate_or_unknown_ids() {
    let service = SessionMessageQueueService::new(10).expect("queue service should initialize");
    let first = service
        .add("session-1", text_input("first"))
        .expect("first message should be added");
    let second = service
        .add("session-1", text_input("second"))
        .expect("second message should be added");
    let first_id = first.messages[0].id.clone();
    let second_id = second.messages[1].id.clone();

    for ids in [
        vec![first_id.clone()],
        vec![first_id.clone(), first_id.clone()],
        vec![first_id.clone(), "missing".to_string()],
    ] {
        let error = service
            .reorder("session-1", &ids)
            .expect_err("invalid reorder should fail");
        assert_eq!(error.error_code(), ErrorCode::Conflict);
    }

    assert_eq!(
        service.list("session-1").expect("queue should list"),
        second
    );
    assert_eq!(
        service
            .reorder("session-1", &[first_id, second_id])
            .expect("existing order should be accepted"),
        second
    );
}

#[test]
fn visible_mutations_publish_once_and_no_ops_do_not_publish() {
    let service = SessionMessageQueueService::new(10).expect("queue service should initialize");
    let mut changed = service.subscribe_changed();

    let added = service
        .add("session-1", text_input("first"))
        .expect("message should be added");
    assert_eq!(
        changed.try_recv().expect("add should notify"),
        MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 1,
        }
    );

    let message_id = added.messages[0].id.clone();
    let unchanged = service
        .update("session-1", &message_id, text_input("first"))
        .expect("same input should be accepted");
    assert_eq!(unchanged, added);
    assert!(changed.try_recv().is_err());

    let unchanged = service
        .reorder("session-1", std::slice::from_ref(&message_id))
        .expect("same order should be accepted");
    assert_eq!(unchanged, added);
    assert!(changed.try_recv().is_err());

    let deleted = service
        .delete("session-1", &message_id)
        .expect("message should delete");
    assert_eq!(deleted.revision, 2);
    assert_eq!(
        changed.try_recv().expect("delete should notify"),
        MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 2,
        }
    );
}

#[test]
fn reserved_message_rejects_competing_mutations_until_released() {
    let service = SessionMessageQueueService::new(10).expect("queue service should initialize");
    let queue = service
        .add("session-1", text_input("first"))
        .expect("message should be added");
    let message_id = queue.messages[0].id.clone();
    let reservation = service
        .reserve_front("session-1")
        .expect("queue front should reserve")
        .expect("queue front should exist");
    assert_eq!(reservation.input(), &text_input("first"));

    for error in [
        service
            .update("session-1", &message_id, text_input("updated"))
            .expect_err("reserved message should not update"),
        service
            .delete("session-1", &message_id)
            .expect_err("reserved message should not delete"),
        service
            .reorder("session-1", std::slice::from_ref(&message_id))
            .expect_err("queue containing reserved message should not reorder"),
    ] {
        assert_eq!(error.error_code(), ErrorCode::MessageQueueBusy);
    }
    assert_eq!(service.list("session-1").expect("queue should list"), queue);

    service
        .release(reservation)
        .expect("reservation should release");
    let updated = service
        .update("session-1", &message_id, text_input("updated"))
        .expect("released message should update");
    assert_eq!(updated.revision, 2);
}

#[test]
fn committing_a_reservation_removes_the_message_once() {
    let service = SessionMessageQueueService::new(10).expect("queue service should initialize");
    let mut changed = service.subscribe_changed();
    let queue = service
        .add("session-1", text_input("first"))
        .expect("message should be added");
    let message_id = queue.messages[0].id.clone();
    changed.try_recv().expect("add should notify");

    let reservation = service
        .reserve("session-1", &message_id)
        .expect("message should reserve");
    assert!(changed.try_recv().is_err());

    let committed = service
        .commit(reservation)
        .expect("reserved message should commit");
    assert_eq!(committed.revision, 2);
    assert!(committed.messages.is_empty());
    assert_eq!(
        changed.try_recv().expect("commit should notify"),
        MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 2,
        }
    );
}

#[test]
fn queues_are_isolated_by_session_and_cleanup_discards_state() {
    let service = SessionMessageQueueService::new(10).expect("queue service should initialize");
    service
        .add("session-1", text_input("first"))
        .expect("first session message should be added");
    let second = service
        .add("session-2", text_input("second"))
        .expect("second session message should be added");

    service
        .clear_session("session-1")
        .expect("session queue should clear");

    assert_eq!(
        service
            .list("session-1")
            .expect("cleared queue should list"),
        MessageQueue {
            revision: 0,
            max_messages: 10,
            messages: Vec::new(),
        }
    );
    assert_eq!(
        service.list("session-2").expect("other queue should list"),
        second
    );
}

#[test]
fn concurrent_adds_are_serialized_without_losing_messages() {
    let service = SessionMessageQueueService::new(16).expect("queue service should initialize");
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
        .map(|index| {
            let service = service.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service.add("session-1", text_input(format!("message-{index}")))
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle
            .join()
            .expect("add thread should not panic")
            .expect("concurrent add should succeed");
    }

    let queue = service.list("session-1").expect("queue should list");
    let texts = queue
        .messages
        .iter()
        .filter_map(|message| match &message.input.content[0] {
            AgentUserContent::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    assert_eq!(queue.revision, 8);
    assert_eq!(queue.messages.len(), 8);
    assert_eq!(texts.len(), 8);
}
