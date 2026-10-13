use talos_agent_api::{TimelineEvent, TimelineReplay};
use talos_types::{MessageQueueChangedNotification, SubscribeTimelineResponse};
use tokio::sync::{broadcast, mpsc};

use super::TimelineEventId;

pub type TimelineEventStream = broadcast::Receiver<TimelineEvent>;

#[derive(Debug, Clone)]
pub struct TimelineSubscribeCommand {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub after_event_id: Option<TimelineEventId>,
    pub replay_limit: usize,
    pub close_when_idle: bool,
    pub close_on_terminal: bool,
}

pub struct TimelineSubscription {
    pub subscription_id: String,
    pub session_id: String,
    pub high_water_mark_event_id: Option<TimelineEventId>,
    pub replay: TimelineReplay,
    pub events: mpsc::Receiver<TimelineSubscriptionMessage>,
    pub has_live_turn: bool,
}

impl TimelineSubscription {
    pub fn response(&self) -> SubscribeTimelineResponse {
        SubscribeTimelineResponse {
            subscription_id: self.subscription_id.clone(),
            session_id: self.session_id.clone(),
            high_water_mark_event_id: self
                .high_water_mark_event_id
                .as_ref()
                .map(ToString::to_string),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineSubscriptionClosed {
    pub reason: TimelineSubscriptionCloseReason,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineSubscriptionCloseReason {
    Completed,
    ServerShutdown,
    Lagged,
}

#[derive(Debug, Clone)]
pub enum TimelineSubscriptionMessage {
    ActiveTurnChanged(talos_agent_api::SessionActiveTurnChangedNotification),
    Event(Box<TimelineEvent>),
    MessageQueueChanged(MessageQueueChangedNotification),
    Closed(TimelineSubscriptionClosed),
}
