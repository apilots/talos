use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, RwLock};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{AgentUserInput, MessageQueue, MessageQueueChangedNotification, QueuedMessage};
use tokio::sync::broadcast;
use uuid::Uuid;

const CHANGED_CHANNEL_CAPACITY: usize = 128;

#[derive(Default)]
struct SessionMessageQueue {
    messages: VecDeque<QueuedMessage>,
    busy_message_ids: HashSet<String>,
    revision: u64,
}

struct SessionMessageQueueInner {
    queues: RwLock<HashMap<String, Arc<Mutex<SessionMessageQueue>>>>,
    max_messages: u32,
    changed_tx: broadcast::Sender<MessageQueueChangedNotification>,
}

/// Owns bounded, process-local message queues isolated by session.
///
/// Callers validate session access and user input before mutation. This service
/// owns FIFO order, capacity, revisions, mutation conflicts, and live change
/// signals. It does not start turns, persist messages, or manage connections.
#[derive(Clone)]
pub struct SessionMessageQueueService {
    inner: Arc<SessionMessageQueueInner>,
}

pub(crate) struct MessageQueueReservation {
    session_id: String,
    message_id: String,
    input: AgentUserInput,
    source: talos_types::QueuedMessageSource,
}

impl MessageQueueReservation {
    pub(crate) fn source(&self) -> &talos_types::QueuedMessageSource {
        &self.source
    }
    pub(crate) fn message_id(&self) -> &str {
        &self.message_id
    }
    pub(crate) fn input(&self) -> &AgentUserInput {
        &self.input
    }
}

impl SessionMessageQueueService {
    pub fn new(max_messages: u32) -> TalosResult<Self> {
        if max_messages == 0 {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "message queue max_messages must be > 0",
            ));
        }

        Ok(Self {
            inner: Arc::new(SessionMessageQueueInner {
                queues: RwLock::new(HashMap::new()),
                max_messages,
                changed_tx: broadcast::channel(CHANGED_CHANNEL_CAPACITY).0,
            }),
        })
    }

    pub fn subscribe_changed(&self) -> broadcast::Receiver<MessageQueueChangedNotification> {
        self.inner.changed_tx.subscribe()
    }

    pub fn add(&self, session_id: &str, input: AgentUserInput) -> TalosResult<MessageQueue> {
        self.add_from(session_id, input, talos_types::QueuedMessageSource::User)
    }

    pub(crate) fn add_loop(
        &self,
        session_id: &str,
        input: AgentUserInput,
        loop_id: String,
        occurrence_id: String,
    ) -> TalosResult<MessageQueue> {
        self.add_from(
            session_id,
            input,
            talos_types::QueuedMessageSource::Loop {
                loop_id,
                occurrence_id,
            },
        )
    }
    fn add_from(
        &self,
        session_id: &str,
        input: AgentUserInput,
        source: talos_types::QueuedMessageSource,
    ) -> TalosResult<MessageQueue> {
        self.with_queue(session_id, QueueAccess::Create, |queue| {
            if let talos_types::QueuedMessageSource::Loop { loop_id, .. } = &source {
                if queue.messages.iter().any(|m| matches!(&m.source,talos_types::QueuedMessageSource::Loop {loop_id: id,..} if id==loop_id)) { return Ok(self.snapshot(queue)); }
            } else { validate_intent(queue,&input,None)?; }

            if queue.messages.len() >= self.inner.max_messages as usize {
                return Err(TalosError::new(
                    ErrorCode::MessageQueueFull,
                    "The session message queue has reached its configured capacity; remove a queued message before adding another.",
                ));
            }

            let revision = next_revision(queue.revision)?;
            let message=QueuedMessage {
                id: format!("qmsg_{}", Uuid::now_v7().simple()),
                input,
                source: source.clone(),
            };
            if source == talos_types::QueuedMessageSource::User {
                let index=queue.messages.iter().position(|m|matches!(m.source,talos_types::QueuedMessageSource::Loop{..})).unwrap_or(queue.messages.len());
                queue.messages.insert(index,message);
            } else { queue.messages.push_back(message); }
            queue.revision = revision;
            self.publish_changed(session_id, revision);
            Ok(self.snapshot(queue))
        })?
        .ok_or_else(|| TalosError::internal("create queue access returned no session queue"))
    }

    pub fn list(&self, session_id: &str) -> TalosResult<MessageQueue> {
        Ok(self
            .with_queue(session_id, QueueAccess::Existing, |queue| {
                Ok(self.snapshot(queue))
            })?
            .unwrap_or_else(|| self.empty_queue()))
    }

    pub fn update(
        &self,
        session_id: &str,
        message_id: &str,
        input: AgentUserInput,
    ) -> TalosResult<MessageQueue> {
        self.with_existing_message(session_id, message_id, |queue, index| {
            ensure_user(queue, index)?;
            validate_intent(queue, &input, Some(message_id))?;
            ensure_not_busy(queue, message_id)?;
            if queue.messages[index].input == input {
                return Ok(self.snapshot(queue));
            }

            let revision = next_revision(queue.revision)?;
            queue.messages[index].input = input;
            queue.revision = revision;
            self.publish_changed(session_id, revision);
            Ok(self.snapshot(queue))
        })
    }

    pub fn delete(&self, session_id: &str, message_id: &str) -> TalosResult<MessageQueue> {
        self.with_existing_message(session_id, message_id, |queue, index| {
            ensure_user(queue, index)?;
            ensure_not_busy(queue, message_id)?;
            let revision = next_revision(queue.revision)?;
            queue.messages.remove(index);
            queue.revision = revision;
            self.publish_changed(session_id, revision);
            Ok(self.snapshot(queue))
        })
    }

    pub fn reorder(&self, session_id: &str, message_ids: &[String]) -> TalosResult<MessageQueue> {
        self.with_queue(session_id, QueueAccess::Existing, |queue| {
            if !queue.busy_message_ids.is_empty() {
                return Err(queue_busy("message queue contains a busy message"));
            }

            let current_ids = queue
                .messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<HashSet<_>>();
            let requested_ids = message_ids
                .iter()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            if message_ids.len() != queue.messages.len()
                || requested_ids.len() != message_ids.len()
                || requested_ids != current_ids
            {
                return Err(queue_conflict(
                    "message ids must be a complete unique queue permutation",
                ));
            }

            if queue
                .messages
                .iter()
                .map(|message| message.id.as_str())
                .eq(message_ids.iter().map(String::as_str))
            {
                return Ok(self.snapshot(queue));
            }

            let mut loop_seen = false;
            let ordered = message_ids
                .iter()
                .map(|id| {
                    queue
                        .messages
                        .iter()
                        .find(|m| &m.id == id)
                        .expect("validated IDs")
                })
                .collect::<Vec<_>>();
            for message in &ordered {
                match message.source {
                    talos_types::QueuedMessageSource::Loop { .. } => loop_seen = true,
                    talos_types::QueuedMessageSource::User if loop_seen => {
                        return Err(queue_conflict("User messages must precede Loop messages"))
                    }
                    _ => {}
                }
            }
            let previous_loops = queue
                .messages
                .iter()
                .filter(|m| matches!(m.source, talos_types::QueuedMessageSource::Loop { .. }))
                .map(|m| &m.id)
                .collect::<Vec<_>>();
            let next_loops = ordered
                .iter()
                .filter(|m| matches!(m.source, talos_types::QueuedMessageSource::Loop { .. }))
                .map(|m| &m.id)
                .collect::<Vec<_>>();
            if previous_loops != next_loops {
                return Err(queue_conflict("Loop message order is read-only"));
            }
            let revision = next_revision(queue.revision)?;
            let mut messages = queue
                .messages
                .drain(..)
                .map(|message| (message.id.clone(), message))
                .collect::<HashMap<_, _>>();
            queue.messages = message_ids
                .iter()
                .map(|message_id| {
                    messages
                        .remove(message_id)
                        .expect("validated message id must exist")
                })
                .collect();
            queue.revision = revision;
            self.publish_changed(session_id, revision);
            Ok(self.snapshot(queue))
        })?
        .ok_or_else(|| message_not_found(session_id, "queue"))
    }

    pub(crate) fn reserve(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> TalosResult<MessageQueueReservation> {
        self.with_existing_message(session_id, message_id, |queue, index| {
            ensure_not_busy(queue, message_id)?;
            queue.busy_message_ids.insert(message_id.to_string());
            Ok(MessageQueueReservation {
                session_id: session_id.to_string(),
                message_id: message_id.to_string(),
                input: queue.messages[index].input.clone(),
                source: queue.messages[index].source.clone(),
            })
        })
    }

    pub(crate) fn reserve_front(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<MessageQueueReservation>> {
        Ok(self
            .with_queue(session_id, QueueAccess::Existing, |queue| {
                let Some(message) = queue.messages.front() else {
                    return Ok(None);
                };
                ensure_not_busy(queue, &message.id)?;
                let reservation = MessageQueueReservation {
                    session_id: session_id.to_string(),
                    message_id: message.id.clone(),
                    input: message.input.clone(),
                    source: message.source.clone(),
                };
                queue
                    .busy_message_ids
                    .insert(reservation.message_id.clone());
                Ok(Some(reservation))
            })?
            .flatten())
    }

    pub(crate) fn release(&self, reservation: MessageQueueReservation) -> TalosResult<()> {
        let _ = self.with_queue(&reservation.session_id, QueueAccess::Existing, |queue| {
            queue.busy_message_ids.remove(&reservation.message_id);
            Ok(())
        })?;
        Ok(())
    }

    pub(crate) fn commit(&self, reservation: MessageQueueReservation) -> TalosResult<MessageQueue> {
        let session_id = reservation.session_id;
        let message_id = reservation.message_id;
        self.with_existing_message(&session_id, &message_id, |queue, index| {
            if !queue.busy_message_ids.contains(&message_id) {
                return Err(queue_conflict(
                    "message queue reservation is no longer active",
                ));
            }
            let revision = match next_revision(queue.revision) {
                Ok(revision) => revision,
                Err(error) => {
                    queue.busy_message_ids.remove(&message_id);
                    return Err(error);
                }
            };
            queue.busy_message_ids.remove(&message_id);
            queue.messages.remove(index);
            queue.revision = revision;
            self.publish_changed(&session_id, revision);
            Ok(self.snapshot(queue))
        })
    }

    pub(crate) fn is_reserved(&self, id: &str, session: &str) -> TalosResult<bool> {
        Ok(self
            .with_queue(session, QueueAccess::Existing, |queue| {
                Ok(queue.busy_message_ids.contains(id))
            })?
            .unwrap_or(false))
    }
    pub(crate) fn remove_loop(&self, session_id: &str, loop_id: &str) -> TalosResult<()> {
        self.with_queue(session_id,QueueAccess::Existing,|queue| {
            let before=queue.messages.len();
            queue.messages.retain(|m|!matches!(&m.source,talos_types::QueuedMessageSource::Loop{loop_id:id,..} if id==loop_id) || queue.busy_message_ids.contains(&m.id));
            if queue.messages.len()!=before { queue.revision=next_revision(queue.revision)?; self.publish_changed(session_id,queue.revision); }
            Ok(())
        })?;
        Ok(())
    }
    pub(crate) fn discard(&self, reservation: MessageQueueReservation) -> TalosResult<()> {
        self.commit(reservation).map(|_| ())
    }
    pub fn clear_session(&self, session_id: &str) -> TalosResult<()> {
        self.inner
            .queues
            .write()
            .map_err(|_| TalosError::internal("message queue map write lock poisoned"))?
            .remove(session_id);
        Ok(())
    }

    fn with_existing_message<T>(
        &self,
        session_id: &str,
        message_id: &str,
        operation: impl FnOnce(&mut SessionMessageQueue, usize) -> TalosResult<T>,
    ) -> TalosResult<T> {
        self.with_queue(session_id, QueueAccess::Existing, |queue| {
            let index = queue
                .messages
                .iter()
                .position(|message| message.id == message_id)
                .ok_or_else(|| message_not_found(session_id, message_id))?;
            operation(queue, index)
        })?
        .ok_or_else(|| message_not_found(session_id, message_id))
    }

    fn with_queue<T>(
        &self,
        session_id: &str,
        access: QueueAccess,
        operation: impl FnOnce(&mut SessionMessageQueue) -> TalosResult<T>,
    ) -> TalosResult<Option<T>> {
        {
            let queues = self
                .inner
                .queues
                .read()
                .map_err(|_| TalosError::internal("message queue map read lock poisoned"))?;
            if let Some(queue) = queues.get(session_id) {
                let mut queue = queue
                    .lock()
                    .map_err(|_| TalosError::internal("session message queue lock poisoned"))?;
                return operation(&mut queue).map(Some);
            }
            if access == QueueAccess::Existing {
                return Ok(None);
            }
        }

        let mut queues = self
            .inner
            .queues
            .write()
            .map_err(|_| TalosError::internal("message queue map write lock poisoned"))?;
        let queue = queues
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(SessionMessageQueue::default())));
        let mut queue = queue
            .lock()
            .map_err(|_| TalosError::internal("session message queue lock poisoned"))?;
        operation(&mut queue).map(Some)
    }

    fn snapshot(&self, queue: &SessionMessageQueue) -> MessageQueue {
        MessageQueue {
            revision: queue.revision,
            max_messages: self.inner.max_messages,
            messages: queue.messages.iter().cloned().collect(),
        }
    }

    fn empty_queue(&self) -> MessageQueue {
        MessageQueue {
            revision: 0,
            max_messages: self.inner.max_messages,
            messages: Vec::new(),
        }
    }

    fn publish_changed(&self, session_id: &str, revision: u64) {
        let _ = self.inner.changed_tx.send(MessageQueueChangedNotification {
            session_id: session_id.to_string(),
            revision,
        });
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueueAccess {
    Existing,
    Create,
}

fn next_revision(revision: u64) -> TalosResult<u64> {
    revision
        .checked_add(1)
        .ok_or_else(|| TalosError::internal("message queue revision overflow"))
}

fn ensure_not_busy(queue: &SessionMessageQueue, message_id: &str) -> TalosResult<()> {
    if queue.busy_message_ids.contains(message_id) {
        return Err(queue_busy("message queue message is busy"));
    }
    Ok(())
}

fn queue_busy(diagnostic: impl Into<String>) -> TalosError {
    TalosError::new(ErrorCode::MessageQueueBusy, diagnostic)
}

fn queue_conflict(diagnostic: impl Into<String>) -> TalosError {
    TalosError::new(ErrorCode::Conflict, diagnostic)
}

fn message_not_found(session_id: &str, message_id: &str) -> TalosError {
    TalosError::new(
        ErrorCode::ResourceNotFound,
        format!("message queue item `{message_id}` was not found in session `{session_id}`"),
    )
}

fn ensure_user(queue: &SessionMessageQueue, index: usize) -> TalosResult<()> {
    if queue.messages[index].source != talos_types::QueuedMessageSource::User {
        return Err(queue_conflict("Automatic Loop messages are read-only"));
    }
    Ok(())
}
fn validate_intent(
    queue: &SessionMessageQueue,
    input: &AgentUserInput,
    exclude: Option<&str>,
) -> TalosResult<()> {
    let Some(incoming) = talos_types::explicit_automation_mode(input)? else {
        return Ok(());
    };
    for message in queue.messages.iter().filter(|m| {
        Some(m.id.as_str()) != exclude && m.source == talos_types::QueuedMessageSource::User
    }) {
        if let Some(pending) = talos_types::explicit_automation_mode(&message.input)? {
            if incoming == talos_types::AutomationMode::Goal
                || pending == talos_types::AutomationMode::Goal
            {
                return Err(queue_conflict(
                    "Session has a conflicting pending activation",
                ));
            }
        }
    }
    Ok(())
}
