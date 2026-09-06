use talos_error::{ErrorCode, TalosError, TalosResult};

const EVENT_ID_PREFIX: &str = "ev_";
const EVENT_ID_WIDTH: usize = 20;

/// Service-private parsed form of the opaque Timeline cursor exposed as a String.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimelineEventId(String);

impl TimelineEventId {
    pub fn from_seq(event_seq: i64) -> Self {
        Self(format!(
            "{EVENT_ID_PREFIX}{event_seq:0width$}",
            width = EVENT_ID_WIDTH
        ))
    }

    pub fn event_seq(&self) -> TalosResult<i64> {
        let raw = self.0.strip_prefix(EVENT_ID_PREFIX).ok_or_else(|| {
            TalosError::public(
                ErrorCode::InvalidRequest,
                format!("invalid timeline event id: {}", self.0),
            )
        })?;
        raw.parse::<i64>().map_err(|error| {
            TalosError::public(
                ErrorCode::InvalidRequest,
                format!("invalid timeline event id {}: {error}", self.0),
            )
        })
    }
}

impl std::fmt::Display for TimelineEventId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<String> for TimelineEventId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
