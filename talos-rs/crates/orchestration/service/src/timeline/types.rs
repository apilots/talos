use talos_store::session::TimelineEventId;
use talos_types::talos::TimelineEventEnvelope;
use talos_types::{SubscribeTimelineResponse, TimelineActiveTurnSnapshot};
use tokio::sync::{broadcast, mpsc};

pub type ApplicationEvent = TimelineEventEnvelope;
pub type ApplicationEventStream = broadcast::Receiver<TimelineEventEnvelope>;

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
    pub session_id: String,
    pub high_water_mark_event_id: Option<TimelineEventId>,
    pub replay: EventReplay,
    pub events: mpsc::Receiver<TimelineSubscriptionMessage>,
    pub has_live_turn: bool,
}

pub struct TimelineSubscriptionSource {
    pub response: SubscribeTimelineResponse,
    pub subscription: TimelineSubscription,
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
    Event(TimelineEventEnvelope),
    Closed(TimelineSubscriptionClosed),
}

#[derive(Debug)]
pub struct TimelineRecoveryState {
    pub replay: EventReplay,
    pub active_turn: TimelineActiveTurnSnapshot,
}

#[derive(Debug)]
pub struct EventReplay {
    pub events: Vec<TimelineEventEnvelope>,
    pub high_water_mark_event_id: Option<TimelineEventId>,
    pub last_replay_seq: i64,
    pub cursor_status: TimelineCursorStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineCursorStatus {
    Current,
    FromBeginning,
}
