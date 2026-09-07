use std::time::{Duration, Instant};

use talos_error::{ErrorCode, TalosError};
use talos_sdk::{
    AgentTurnRef, AgentUserContent, AgentUserInput, MessageQueue, MessageQueueAddRequest,
    MessageQueueDeleteRequest, MessageQueueListRequest, MessageQueueReorderRequest,
    MessageQueueStartRequest, MessageQueueUpdateRequest, QueuedMessage, SkillSuggestion,
    SubmitTurnInput, SubmitTurnInputRequest,
};
use tokio::sync::mpsc::error::TryRecvError;

use crate::turn_request::build_terminal_input;
use crate::TuiApp;

const QUEUE_SUBSCRIPTION_RETRY_DELAY: Duration = Duration::from_secs(1);

impl TuiApp {
    pub(crate) async fn ensure_session_subscription(&mut self) {
        let Some(session_id) = self.session_id.clone() else {
            self.clear_message_queue_projection();
            return;
        };
        if self.connection_status != "connected" {
            self.clear_message_queue_projection();
            return;
        }
        if self.session_subscription.is_some()
            && self.session_subscription_session_id.as_deref() == Some(session_id.as_str())
        {
            return;
        }
        if self
            .message_queue_retry_at
            .is_some_and(|retry_at| retry_at > Instant::now())
        {
            return;
        }
        let Some(client) = self.gateway_client.clone() else {
            return;
        };

        self.clear_message_queue_projection();
        match client.subscribe_session_events(&session_id).await {
            Ok(subscription) => {
                self.session_subscription = Some(subscription);
                self.session_subscription_session_id = Some(session_id.clone());
                match client
                    .list_message_queue(MessageQueueListRequest { session_id })
                    .await
                {
                    Ok(queue) => {
                        self.apply_message_queue(queue);
                        self.message_queue_retry_at = None;
                    }
                    Err(error) => self.handle_message_queue_disconnect(error),
                }
            }
            Err(error) => self.handle_message_queue_disconnect(error),
        }
    }

    pub(crate) async fn poll_session_events(&mut self) {
        let mut highest_revision: Option<u64> = None;
        let mut disconnected = false;
        let mut notices = Vec::new();
        if let Some(subscription) = self.session_subscription.as_mut() {
            loop {
                match subscription.try_recv() {
                    Ok(crate::client::SessionEvent::Notice { event_seq, notice }) => {
                        notices.push((event_seq, notice))
                    }
                    Ok(crate::client::SessionEvent::QueueChanged(change)) => {
                        if self.session_subscription_session_id.as_deref()
                            == Some(change.session_id.as_str())
                        {
                            highest_revision =
                                Some(highest_revision.unwrap_or_default().max(change.revision));
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        for (event_seq, notice) in notices {
            self.apply_system_notice(event_seq, notice);
        }
        if disconnected {
            self.handle_message_queue_disconnect(TalosError::internal(
                "message queue subscription closed",
            ));
            return;
        }
        if let Some(revision) = highest_revision {
            if revision > self.message_queue.revision {
                self.refresh_message_queue(revision).await;
            }
        }
        self.reconcile_queue_driven_turn().await;
    }

    pub(crate) async fn enqueue_message(
        &mut self,
        message: String,
        skills: Vec<SkillSuggestion>,
        file_ids: Vec<String>,
    ) -> bool {
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            self.show_status_hint("No active session for Queue.", Duration::from_secs(2));
            return false;
        };
        let request = MessageQueueAddRequest {
            session_id,
            input: build_terminal_input(message, &skills, file_ids),
        };
        match client.add_queued_message(request).await {
            Ok(queue) => {
                self.apply_message_queue(queue);
                true
            }
            Err(error) => {
                self.show_message_queue_error(&error);
                false
            }
        }
    }

    pub(crate) fn select_message_queue(&mut self) {
        self.selected_queue_message = if self.message_queue.messages.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    pub(crate) fn move_message_queue_selection(&mut self, offset: isize) {
        let Some(current) = self.selected_queue_message else {
            self.select_message_queue();
            return;
        };
        let last = self.message_queue.messages.len().saturating_sub(1);
        self.selected_queue_message = Some(current.saturating_add_signed(offset).min(last));
    }

    pub(crate) fn begin_selected_queue_edit(&mut self) {
        let Some(message) = self.selected_queue_message().cloned() else {
            return;
        };
        self.input_buffer = queued_message_text(&message);
        self.cursor_index = self.input_buffer.chars().count();
        self.editing_queue_message_id = Some(message.id);
        self.selected_queue_message = None;
    }

    pub(crate) async fn update_queued_message_text(&mut self, text: String) -> bool {
        let Some(message_id) = self.editing_queue_message_id.clone() else {
            return false;
        };
        let Some(existing) = self
            .message_queue
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .cloned()
        else {
            self.editing_queue_message_id = None;
            self.show_status_hint("Queued message no longer exists.", Duration::from_secs(2));
            return false;
        };
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return false;
        };
        let mut content = Vec::new();
        if !text.trim().is_empty() {
            content.push(AgentUserContent::Text {
                text: text.trim().to_string(),
            });
        }
        content.extend(
            existing
                .input
                .content
                .into_iter()
                .filter(|item| !matches!(item, AgentUserContent::Text { .. })),
        );
        match client
            .update_queued_message(MessageQueueUpdateRequest {
                session_id,
                message_id,
                input: AgentUserInput { content },
            })
            .await
        {
            Ok(queue) => {
                self.apply_message_queue(queue);
                self.editing_queue_message_id = None;
                true
            }
            Err(error) => {
                self.show_message_queue_error(&error);
                false
            }
        }
    }

    pub(crate) async fn delete_selected_queued_message(&mut self) {
        let Some(message_id) = self
            .selected_queue_message()
            .map(|message| message.id.clone())
        else {
            return;
        };
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return;
        };
        match client
            .delete_queued_message(MessageQueueDeleteRequest {
                session_id,
                message_id,
            })
            .await
        {
            Ok(queue) => self.apply_message_queue(queue),
            Err(error) => self.show_message_queue_error(&error),
        }
    }

    pub(crate) async fn reorder_selected_queued_message(&mut self, offset: isize) {
        let Some(index) = self.selected_queue_message else {
            return;
        };
        let target = index.saturating_add_signed(offset);
        if target >= self.message_queue.messages.len() || target == index {
            return;
        }
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return;
        };
        let mut message_ids = self
            .message_queue
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        message_ids.swap(index, target);
        match client
            .reorder_message_queue(MessageQueueReorderRequest {
                session_id,
                message_ids,
            })
            .await
        {
            Ok(queue) => {
                self.apply_message_queue(queue);
                self.selected_queue_message = Some(target);
            }
            Err(error) => self.show_message_queue_error(&error),
        }
    }

    pub(crate) async fn start_or_steer_selected_queued_message(&mut self) {
        let Some(message) = self.selected_queue_message().cloned() else {
            return;
        };
        if self.in_flight {
            if !queued_message_is_steerable(&message) {
                self.show_status_hint(
                    "Messages containing skills cannot Steer an active turn.",
                    Duration::from_secs(3),
                );
                return;
            }
            self.steer_queued_message(message.id).await;
        } else {
            self.start_queued_message(message.id).await;
        }
    }

    fn selected_queue_message(&self) -> Option<&QueuedMessage> {
        self.selected_queue_message
            .and_then(|index| self.message_queue.messages.get(index))
    }

    async fn start_queued_message(&mut self, message_id: String) {
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return;
        };
        match client
            .start_queued_message(MessageQueueStartRequest {
                session_id: session_id.clone(),
                message_id: Some(message_id),
            })
            .await
        {
            Ok(_) => {
                self.selected_queue_message = None;
                self.refresh_message_queue(self.message_queue.revision.saturating_add(1))
                    .await;
            }
            Err(error) => self.show_message_queue_error(&error),
        }
    }

    async fn steer_queued_message(&mut self, message_id: String) {
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return;
        };
        let active_turn = match client.get_active_turn(&session_id).await {
            Ok(active_turn) => active_turn,
            Err(error) => {
                self.show_message_queue_error(&error);
                return;
            }
        };
        let Some(turn_id) = active_turn.map(|active| active.identity.turn_id) else {
            self.show_status_hint("No active turn for Steer.", Duration::from_secs(2));
            return;
        };
        match client
            .submit_turn_input(SubmitTurnInputRequest {
                identity: AgentTurnRef::new(&session_id, turn_id),
                input: SubmitTurnInput::QueuedMessage {
                    message_id: message_id.clone(),
                },
                client_request_id: None,
            })
            .await
        {
            Ok(_) => {
                self.selected_queue_message = None;
                self.refresh_message_queue(self.message_queue.revision.saturating_add(1))
                    .await;
            }
            Err(error) => self.show_message_queue_error(&error),
        }
    }

    async fn refresh_message_queue(&mut self, target_revision: u64) {
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return;
        };
        match client
            .list_message_queue(MessageQueueListRequest { session_id })
            .await
        {
            Ok(queue) if queue.revision >= target_revision => {
                let removed_message = self
                    .message_queue
                    .messages
                    .iter()
                    .any(|current| !queue.messages.iter().any(|next| next.id == current.id));
                self.apply_message_queue(queue);
                self.message_queue_turn_reconcile_pending |= removed_message;
                self.reconcile_queue_driven_turn().await;
            }
            Ok(_) => self.handle_message_queue_disconnect(TalosError::internal(
                "message queue list did not reach the notified revision",
            )),
            Err(error) => self.handle_message_queue_disconnect(error),
        }
    }

    async fn reconcile_queue_driven_turn(&mut self) {
        if !self.message_queue_turn_reconcile_pending {
            return;
        }
        if self.in_flight || self.pending_stream.is_some() || self.pending_stream_connect.is_some()
        {
            return;
        }
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return;
        };
        let active_turn = match client.get_active_turn(&session_id).await {
            Ok(active_turn) => active_turn,
            Err(error) => {
                self.message_queue_turn_reconcile_pending = false;
                self.show_message_queue_error(&error);
                return;
            }
        };
        self.message_queue_turn_reconcile_pending = false;
        if active_turn.is_some() {
            self.connect_queued_turn_stream(&session_id).await;
            return;
        }
        match client.get_session_history(&session_id).await {
            Ok(history) => {
                self.reset_chat_surface();
                let item_count = self.replay_history_snapshot(history);
                self.push_right_event(format!(
                    "queue-driven turn history loaded: {item_count} items"
                ));
            }
            Err(error) => self.show_message_queue_error(&error),
        }
    }

    async fn connect_queued_turn_stream(&mut self, session_id: &str) {
        let Some(client) = self.gateway_client.clone() else {
            return;
        };
        match client.reconnect_active_turn_stream(session_id, None).await {
            Ok(receiver) => {
                self.abort_pending_stream_task();
                self.pending_stream_connect = None;
                self.pending_stream = Some(receiver);
                self.active_stream_turn_id = None;
                self.last_stream_sequence = None;
                self.last_stream_event_id = None;
                self.context_compaction_stage = None;
                self.in_flight = true;
                self.stream_started_at = Some(Instant::now());
                self.last_stream_event_at = Some(Instant::now());
                self.push_right_event("queued turn stream connected".to_string());
            }
            Err(error) => self.show_message_queue_error(&error),
        }
    }

    fn apply_message_queue(&mut self, queue: MessageQueue) {
        if queue.revision < self.message_queue.revision {
            return;
        }
        self.message_queue = queue;
        if self
            .selected_queue_message
            .is_some_and(|index| index >= self.message_queue.messages.len())
        {
            self.selected_queue_message = self.message_queue.messages.len().checked_sub(1);
        }
        if self
            .editing_queue_message_id
            .as_ref()
            .is_some_and(|message_id| {
                !self
                    .message_queue
                    .messages
                    .iter()
                    .any(|message| &message.id == message_id)
            })
        {
            self.editing_queue_message_id = None;
        }
    }

    fn clear_message_queue_projection(&mut self) {
        self.session_subscription = None;
        self.session_subscription_session_id = None;
        self.message_queue = MessageQueue {
            revision: 0,
            max_messages: 0,
            messages: Vec::new(),
        };
        self.message_queue_turn_reconcile_pending = false;
        self.selected_queue_message = None;
        self.editing_queue_message_id = None;
    }

    fn handle_message_queue_disconnect(&mut self, error: TalosError) {
        self.clear_message_queue_projection();
        self.message_queue_retry_at = Some(Instant::now() + QUEUE_SUBSCRIPTION_RETRY_DELAY);
        self.push_right_event(format!(
            "message queue disconnected: {}",
            error.to_api_error().message
        ));
    }

    fn show_message_queue_error(&mut self, error: &TalosError) {
        let message = if error.is_code(ErrorCode::MessageQueueFull) {
            "Queue is full. The message remains in the composer.".to_string()
        } else {
            format!("Queue operation failed: {}", error.to_api_error().message)
        };
        self.show_status_hint(message, Duration::from_secs(3));
    }
}

pub(crate) fn queued_message_text(message: &QueuedMessage) -> String {
    message
        .input
        .content
        .iter()
        .filter_map(|item| match item {
            AgentUserContent::Text { text } => Some(text.as_str()),
            AgentUserContent::SkillSelection { .. }
            | AgentUserContent::File { .. }
            | AgentUserContent::Audio { .. }
            | AgentUserContent::Video { .. }
            | AgentUserContent::UserActionResponse { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn queued_message_summary(message: &QueuedMessage) -> String {
    let text = queued_message_text(message);
    if !text.trim().is_empty() {
        return crate::utils::summarize_for_queue(&text);
    }
    let attachment_count = message
        .input
        .content
        .iter()
        .map(|item| match item {
            AgentUserContent::File { .. }
            | AgentUserContent::Audio { .. }
            | AgentUserContent::Video { .. } => 1,
            AgentUserContent::Text { .. }
            | AgentUserContent::SkillSelection { .. }
            | AgentUserContent::UserActionResponse { .. } => 0,
        })
        .sum::<usize>();
    if attachment_count > 0 {
        return format!("{attachment_count} attachment(s)");
    }
    message
        .input
        .content
        .iter()
        .find_map(|item| match item {
            AgentUserContent::SkillSelection { name, .. } => Some(format!("${name}")),
            AgentUserContent::Text { .. }
            | AgentUserContent::File { .. }
            | AgentUserContent::Audio { .. }
            | AgentUserContent::Video { .. }
            | AgentUserContent::UserActionResponse { .. } => None,
        })
        .unwrap_or_else(|| "Queued message".to_string())
}

pub(crate) fn queued_message_is_steerable(message: &QueuedMessage) -> bool {
    message.input.content.iter().all(|item| {
        matches!(
            item,
            AgentUserContent::Text { .. } | AgentUserContent::File { .. }
        )
    })
}
