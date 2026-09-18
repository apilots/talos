use std::time::Duration;

use anyhow::{anyhow, Result};
use talos_sdk::{
    AgentEventPayload, AgentTurnRef, AgentUserContent, AgentUserInput, ErrorCode, GatewayClient,
    InterruptTurnRequest, MessageQueue, MessageQueueAddRequest, MessageQueueDeleteRequest,
    MessageQueueListRequest, MessageQueueReorderRequest, MessageQueueStartRequest,
    MessageQueueUpdateRequest, SubmitTurnInput, SubmitTurnInputRequest, SubscribeTimelineRequest,
    TimelineStreamSubscription,
};

use super::request::start_turn_request;

const DEFAULT_MAX_MESSAGES: u32 = 10;

pub(super) async fn message_queue_e2e(client: &mut talos_sdk::AppServerClient) -> Result<()> {
    let session_id = uuid::Uuid::new_v4().to_string();
    eprintln!("message-queue steer");
    let active = GatewayClient::start_turn(
        client,
        start_turn_request(
            &session_id,
            "Use the Bash tool to sleep for 30 seconds, then answer with ok",
            Some(format!(
                "message-queue-e2e-{}",
                uuid::Uuid::new_v4().simple()
            )),
        ),
    )
    .await?;
    let mut subscription = GatewayClient::open_timeline_stream(
        client,
        SubscribeTimelineRequest {
            session_id: session_id.clone(),
            turn_id: None,
            since_event_id: None,
            replay_limit: Some(32),
            close_when_idle: Some(false),
            close_on_terminal: Some(false),
        },
    )
    .await?;

    let queued = add_text(client, &session_id, "Use this instruction instead").await?;
    assert_change(&mut subscription, &session_id, queued.revision).await?;
    GatewayClient::submit_turn_input(
        client,
        SubmitTurnInputRequest {
            identity: AgentTurnRef::new(&session_id, &active.turn_id),
            input: SubmitTurnInput::QueuedMessage {
                message_id: queued.messages[0].id.clone(),
            },
            client_request_id: None,
        },
    )
    .await?;
    let steered = GatewayClient::list_message_queue(
        client,
        MessageQueueListRequest {
            session_id: session_id.clone(),
        },
    )
    .await?;
    if !steered.messages.is_empty() {
        return Err(anyhow!("steered message was not consumed"));
    }
    assert_change(&mut subscription, &session_id, steered.revision).await?;

    GatewayClient::interrupt_turn(
        client,
        InterruptTurnRequest {
            identity: AgentTurnRef::new(&session_id, active.turn_id),
        },
    )
    .await?;
    wait_for_terminal(&mut subscription).await?;

    eprintln!("message-queue bounded mutations");
    let empty = GatewayClient::list_message_queue(
        client,
        MessageQueueListRequest {
            session_id: session_id.clone(),
        },
    )
    .await?;
    if empty.max_messages != DEFAULT_MAX_MESSAGES || !empty.messages.is_empty() {
        return Err(anyhow!("unexpected initial queue: {empty:?}"));
    }

    let mut queue = empty;
    for index in 0..DEFAULT_MAX_MESSAGES {
        queue = add_text(client, &session_id, &format!("queued message {index}")).await?;
        assert_change(&mut subscription, &session_id, queue.revision).await?;
    }
    let full_error = add_text(client, &session_id, "must be rejected")
        .await
        .expect_err("the eleventh queued message must be rejected");
    if full_error.error_code() != ErrorCode::MessageQueueFull {
        return Err(anyhow!("unexpected queue full error: {full_error}"));
    }

    let first_id = queue.messages[0].id.clone();
    queue = GatewayClient::update_queued_message(
        client,
        MessageQueueUpdateRequest {
            session_id: session_id.clone(),
            message_id: first_id.clone(),
            input: text_input("updated first message"),
        },
    )
    .await?;
    assert_change(&mut subscription, &session_id, queue.revision).await?;
    if queue.messages[0].id != first_id
        || queue.messages[0].input != text_input("updated first message")
    {
        return Err(anyhow!("queue update changed identity or position"));
    }

    let mut reversed_ids = queue
        .messages
        .iter()
        .map(|message| message.id.clone())
        .collect::<Vec<_>>();
    reversed_ids.reverse();
    queue = GatewayClient::reorder_message_queue(
        client,
        MessageQueueReorderRequest {
            session_id: session_id.clone(),
            message_ids: reversed_ids.clone(),
        },
    )
    .await?;
    assert_change(&mut subscription, &session_id, queue.revision).await?;
    if queue_ids(&queue) != reversed_ids {
        return Err(anyhow!("queue reorder was not applied"));
    }

    let deleted_id = queue.messages[2].id.clone();
    queue = GatewayClient::delete_queued_message(
        client,
        MessageQueueDeleteRequest {
            session_id: session_id.clone(),
            message_id: deleted_id.clone(),
        },
    )
    .await?;
    assert_change(&mut subscription, &session_id, queue.revision).await?;
    if queue
        .messages
        .iter()
        .any(|message| message.id == deleted_id)
    {
        return Err(anyhow!("deleted message remains in the queue"));
    }

    eprintln!("message-queue explicit start");
    let selected_id = queue.messages[1].id.clone();
    queue = GatewayClient::update_queued_message(
        client,
        MessageQueueUpdateRequest {
            session_id: session_id.clone(),
            message_id: selected_id.clone(),
            input: text_input("Use the Bash tool to sleep for 30 seconds, then answer with ok"),
        },
    )
    .await?;
    assert_change(&mut subscription, &session_id, queue.revision).await?;
    let expected_ids = queue
        .messages
        .iter()
        .filter(|message| message.id != selected_id)
        .map(|message| message.id.clone())
        .collect::<Vec<_>>();
    let started = GatewayClient::start_queued_message(
        client,
        MessageQueueStartRequest {
            session_id: session_id.clone(),
            message_id: Some(selected_id),
        },
    )
    .await?;
    let start_revision = queue.revision + 1;
    assert_change(&mut subscription, &session_id, start_revision).await?;
    let after_start = GatewayClient::list_message_queue(
        client,
        MessageQueueListRequest {
            session_id: session_id.clone(),
        },
    )
    .await?;
    if after_start.revision != start_revision || queue_ids(&after_start) != expected_ids {
        return Err(anyhow!(
            "explicit start did not consume only the selected message"
        ));
    }

    GatewayClient::interrupt_turn(
        client,
        InterruptTurnRequest {
            identity: AgentTurnRef::new(session_id, started.turn_id),
        },
    )
    .await?;
    wait_for_terminal(&mut subscription).await?;
    Ok(())
}

async fn add_text(
    client: &mut talos_sdk::AppServerClient,
    session_id: &str,
    text: &str,
) -> talos_sdk::TalosResult<MessageQueue> {
    GatewayClient::add_queued_message(
        client,
        MessageQueueAddRequest {
            session_id: session_id.to_string(),
            input: text_input(text),
        },
    )
    .await
}

fn text_input(text: &str) -> AgentUserInput {
    AgentUserInput {
        content: vec![AgentUserContent::Text {
            text: text.to_string(),
        }],
    }
}

fn queue_ids(queue: &MessageQueue) -> Vec<String> {
    queue
        .messages
        .iter()
        .map(|message| message.id.clone())
        .collect()
}

async fn assert_change(
    subscription: &mut TimelineStreamSubscription,
    session_id: &str,
    revision: u64,
) -> Result<()> {
    let change = tokio::time::timeout(
        Duration::from_secs(10),
        subscription.message_queue_changes.recv(),
    )
    .await
    .map_err(|_| anyhow!("timed out waiting for message queue revision {revision}"))?
    .ok_or_else(|| anyhow!("message queue change stream closed"))?;
    if change.session_id != session_id || change.revision != revision {
        return Err(anyhow!(
            "unexpected message queue change: expected {session_id}@{revision}, got {}@{}",
            change.session_id,
            change.revision
        ));
    }
    Ok(())
}

async fn wait_for_terminal(subscription: &mut TimelineStreamSubscription) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = subscription.stream.recv().await?;
            if super::is_terminal_event(&event) {
                return match event.event.agent_event().map(|event| &event.payload) {
                    Some(AgentEventPayload::TurnFailed(failed))
                        if failed.error.code == ErrorCode::Cancelled =>
                    {
                        Ok(())
                    }
                    _ => Err(anyhow!(
                        "interrupted turn did not settle as cancelled: {event:?}"
                    )),
                };
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for terminal turn event"))?
}
