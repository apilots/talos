//! Product metadata is projected from the original Turn Store, never Agent facts.
use std::collections::{HashMap, HashSet};
use talos_agent_api::AgentTurnRef;
use talos_error::TalosResult;
use talos_types::AutomationInputOrigin;

use super::TimelineEventService;

impl TimelineEventService {
    pub(crate) async fn read_input_origins(
        &self,
        identities: &[AgentTurnRef],
    ) -> TalosResult<HashMap<(String, String), AutomationInputOrigin>> {
        let mut seen = HashSet::new();
        let identities: Vec<_> = identities
            .iter()
            .filter(|identity| seen.insert((identity.session_id.clone(), identity.turn_id.clone())))
            .cloned()
            .collect();
        let mut origins = HashMap::new();
        for batch in identities.chunks(1000) {
            for (identity, origin) in self
                .execution_store
                .read_automation_input_origins(batch)
                .await?
            {
                origins.insert((identity.session_id, identity.turn_id), origin);
            }
        }
        Ok(origins)
    }
}
