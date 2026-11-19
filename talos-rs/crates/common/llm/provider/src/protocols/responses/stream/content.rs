use super::super::{
    decode::{message_part_text, reasoning_part_text, ReasoningPart},
    event_type, item_type,
};
use crate::ReasoningBlock;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Part {
    Text(usize),
    Summary(usize),
    Reasoning(usize),
}

pub(super) struct TextUpdate<'a> {
    pub part: Part,
    pub text: &'a str,
    pub snapshot: bool,
}

impl<'a> TextUpdate<'a> {
    pub fn from_event(event: &'a Value) -> Option<Self> {
        let content = event_index(event, "content_index").unwrap_or_default();
        let summary = event_index(event, "summary_index").unwrap_or_default();
        let (part, text, snapshot) = match event_type(event) {
            "response.output_text.delta" | "response.refusal.delta" => {
                (Part::Text(content), event["delta"].as_str(), false)
            }
            "response.output_text.done" => (Part::Text(content), event["text"].as_str(), true),
            "response.refusal.done" => (Part::Text(content), event["refusal"].as_str(), true),
            "response.content_part.added" | "response.content_part.done" => {
                (Part::Text(content), message_part_text(&event["part"]), true)
            }
            "response.reasoning_summary_text.delta" => {
                (Part::Summary(summary), event["delta"].as_str(), false)
            }
            "response.reasoning_summary_text.done" => {
                (Part::Summary(summary), event["text"].as_str(), true)
            }
            "response.reasoning_summary_part.added" | "response.reasoning_summary_part.done" => {
                (Part::Summary(summary), event["part"]["text"].as_str(), true)
            }
            "response.reasoning_text.delta" => {
                (Part::Reasoning(content), event["delta"].as_str(), false)
            }
            "response.reasoning_text.done" => {
                (Part::Reasoning(content), event["text"].as_str(), true)
            }
            _ => return None,
        };
        Some(Self {
            part,
            text: text?,
            snapshot,
        })
    }

    pub fn from_item(item: &'a Value) -> impl Iterator<Item = Self> {
        let message = item_type(item) == "message";
        let summary = if message {
            &Value::Null
        } else {
            &item["summary"]
        };
        indexed_parts(summary, Part::Summary, reasoning_part_text).chain(indexed_parts(
            &item["content"],
            if message { Part::Text } else { Part::Reasoning },
            if message {
                message_part_text
            } else {
                reasoning_part_text
            },
        ))
    }
}

fn indexed_parts(
    value: &Value,
    kind: fn(usize) -> Part,
    text: fn(&Value) -> Option<&str>,
) -> impl Iterator<Item = TextUpdate<'_>> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(move |(index, part)| {
            Some(TextUpdate {
                part: kind(index),
                text: text(part)?,
                snapshot: true,
            })
        })
}

pub(super) fn event_index(event: &Value, field: &str) -> Option<usize> {
    event
        .get(field)?
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
}

pub(super) fn reasoning_blocks<'a>(
    id: &'a str,
    parts: &'a BTreeMap<Part, String>,
    encrypted: Option<&'a str>,
) -> impl Iterator<Item = ReasoningBlock> + 'a {
    parts
        .iter()
        .filter_map(|(part, text)| match part {
            Part::Summary(_) => Some(ReasoningPart::Summary(text)),
            Part::Reasoning(_) => Some(ReasoningPart::Text(text)),
            Part::Text(_) => None,
        })
        .chain(encrypted.map(ReasoningPart::Encrypted))
        .map(move |part| part.into_block(Some(id)))
}
