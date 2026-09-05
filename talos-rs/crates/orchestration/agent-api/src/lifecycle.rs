use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Durable nonterminal state of one logical Agent Turn.
///
/// Terminal truth belongs exclusively to the terminal Agent event, so this
/// enum intentionally has no completed or failed variants.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ActiveTurnState {
    Running,
    WaitingForInput,
    Cancelling,
}
