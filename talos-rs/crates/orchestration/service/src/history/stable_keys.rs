use std::collections::HashSet;

use talos_types::talos::{AgentEvent, SessionHistoryMessage};

pub(crate) struct StableMessageKeys<'a> {
    turn_ids: HashSet<&'a str>,
    message_ids: HashSet<&'a str>,
}

impl StableMessageKeys<'_> {
    pub(crate) fn is_empty(&self) -> bool {
        self.turn_ids.is_empty() && self.message_ids.is_empty()
    }

    pub(crate) fn contains_event(&self, event: &AgentEvent) -> bool {
        self.turn_ids.contains(event.turn_id.as_str())
            || self.message_ids.contains(event.message_id.as_str())
    }
}

pub(crate) fn stable_message_keys(messages: &[SessionHistoryMessage]) -> StableMessageKeys<'_> {
    StableMessageKeys {
        turn_ids: messages
            .iter()
            .map(|message| message.turn_id.as_str())
            .filter(|turn_id| !turn_id.is_empty())
            .collect(),
        message_ids: messages
            .iter()
            .map(|message| message.message_id.as_str())
            .filter(|message_id| !message_id.is_empty())
            .collect(),
    }
}
