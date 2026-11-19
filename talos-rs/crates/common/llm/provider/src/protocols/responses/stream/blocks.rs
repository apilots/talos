use super::super::{event_type, item_type, ResponsesContract};
use super::content::{event_index, reasoning_blocks, Part, TextUpdate};
use crate::{
    ModelBlockCompleted, ModelBlockDelta, ModelBlockKind, ModelBlockStarted, ModelBlockStreamEvent,
    ModelOutputPhase, ModelToolCallStart, ReasoningContinuation, ReasoningOutput, Usage,
};
use serde::Deserialize;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use talos_error::{TalosError, TalosResult};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Kind {
    Text,
    Reasoning,
    Tool,
}

impl Kind {
    fn from_item(item: &Value) -> Option<Self> {
        match item_type(item) {
            "message" => Some(Self::Text),
            "reasoning" => Some(Self::Reasoning),
            "function_call" => Some(Self::Tool),
            _ => None,
        }
    }

    fn model_kind(self) -> ModelBlockKind {
        match self {
            Self::Text => ModelBlockKind::AssistantText,
            Self::Reasoning => ModelBlockKind::Reasoning,
            Self::Tool => ModelBlockKind::ToolCall,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Reasoning => "reasoning",
            Self::Tool => "tool",
        }
    }
}

#[derive(Default)]
struct Tool {
    ordinal: usize,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
}

struct OutputItem {
    kind: Kind,
    model_item_id: String,
    provider_item_id: Option<String>,
    phase: ModelOutputPhase,
    parts: BTreeMap<Part, String>,
    encrypted: Option<String>,
    tool: Option<Tool>,
    started: bool,
    completed: bool,
}

impl OutputItem {
    fn start(&mut self, events: &mut Vec<ModelBlockStreamEvent>) {
        if self.started {
            return;
        }
        self.started = true;
        events.push(ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
            model_item_id: self.model_item_id.clone(),
            provider_item_id: self.provider_item_id.clone(),
            kind: self.kind.model_kind(),
            output_phase: self.phase,
            tool_call: self.tool.as_ref().map(|tool| ModelToolCallStart {
                index: tool.ordinal,
                call_id: tool
                    .call_id
                    .clone()
                    .or_else(|| self.provider_item_id.clone()),
                tool_name: tool.name.clone(),
            }),
        }));
    }

    fn complete(&mut self, events: &mut Vec<ModelBlockStreamEvent>) {
        if self.started && !self.completed {
            self.completed = true;
            events.push(ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                model_item_id: self.model_item_id.clone(),
            }));
        }
    }

    fn delta(&self, delta: ModelBlockDelta, events: &mut Vec<ModelBlockStreamEvent>) {
        events.push(ModelBlockStreamEvent::BlockDelta {
            model_item_id: self.model_item_id.clone(),
            delta,
        });
    }
}

/// Tracks aliases and streamed content for a single provider response.
pub(super) struct ResponseBlocks {
    prefix: String,
    contract: ResponsesContract,
    items: Vec<OutputItem>,
    indexes: HashMap<(Kind, usize), usize>,
    provider_ids: HashMap<(Kind, String), usize>,
    next_tool_index: usize,
    final_answer_started: bool,
    reasoning_display: Vec<String>,
    last_reasoning_part: Option<(usize, Part)>,
}

impl ResponseBlocks {
    pub fn new(prefix: String, contract: ResponsesContract) -> Self {
        Self {
            prefix,
            contract,
            items: Vec::new(),
            indexes: HashMap::new(),
            provider_ids: HashMap::new(),
            next_tool_index: 0,
            final_answer_started: false,
            reasoning_display: Vec::new(),
            last_reasoning_part: None,
        }
    }

    pub fn has_tool_calls(&self) -> bool {
        self.next_tool_index > 0
    }

    pub fn apply(&mut self, event: &Value) -> TalosResult<Vec<ModelBlockStreamEvent>> {
        let mut events = Vec::new();
        let item = &event["item"];
        let wire_index = event_index(event, "output_index").or_else(|| event_index(event, "index"));
        let provider_id = item["id"].as_str().or_else(|| event["item_id"].as_str());
        let event_type = event_type(event);
        match event_type {
            "response.output_item.added" | "response.output_item.done" => {
                self.apply_item(
                    wire_index,
                    provider_id,
                    item,
                    event_type.ends_with(".done"),
                    &mut events,
                )?;
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                let slot = self.resolve(Kind::Tool, wire_index, provider_id)?;
                if let Some(name) = event["name"].as_str() {
                    self.items[slot]
                        .tool
                        .as_mut()
                        .unwrap()
                        .name
                        .get_or_insert_with(|| name.to_owned());
                }
                let snapshot = event_type.ends_with(".done");
                let arguments = if snapshot {
                    argument_text(&event["arguments"])
                } else {
                    event["delta"].as_str().map(Cow::Borrowed)
                };
                self.update_tool(slot, arguments.as_deref(), snapshot, false, &mut events)?;
            }
            _ => {
                if let Some(update) = TextUpdate::from_event(event) {
                    let kind = match update.part {
                        Part::Text(_) => Kind::Text,
                        Part::Summary(_) | Part::Reasoning(_) => Kind::Reasoning,
                    };
                    let slot = self.resolve(kind, wire_index, provider_id)?;
                    self.update_text(slot, update, &mut events)?;
                }
            }
        }
        Ok(events)
    }

    pub fn finish(
        &mut self,
        response: &Value,
        finish_reason: String,
        usage: Option<Usage>,
    ) -> TalosResult<Vec<ModelBlockStreamEvent>> {
        let mut events = Vec::new();
        if let Some(output) = response["output"].as_array() {
            for (index, item) in output.iter().enumerate() {
                let provider_id = item["id"].as_str();
                self.apply_item(Some(index), provider_id, item, true, &mut events)?;
            }
        }
        let text_fallback = response["output_text"]
            .as_str()
            .filter(|text| !text.is_empty());
        if let Some(text) =
            text_fallback.filter(|_| self.items.iter().all(|item| item.kind != Kind::Text))
        {
            let slot = self.resolve(Kind::Text, None, None)?;
            self.update_text(
                slot,
                TextUpdate {
                    part: Part::Text(0),
                    text,
                    snapshot: true,
                },
                &mut events,
            )?;
        }
        for slot in 0..self.items.len() {
            if self.items[slot].tool.is_some() && !self.items[slot].completed {
                self.update_tool(slot, None, false, true, &mut events)?;
                if !self.items[slot].started {
                    return Err(TalosError::internal(
                        "The model finished a tool call without a tool name.",
                    ));
                }
            }
            self.items[slot].complete(&mut events);
        }
        let continuation = self
            .items
            .iter()
            .filter(|item| item.kind == Kind::Reasoning)
            .filter_map(|item| {
                let id = item.provider_item_id.as_deref()?;
                Some(reasoning_blocks(id, &item.parts, item.encrypted.as_deref()))
            })
            .flatten()
            .collect::<Vec<_>>();
        if !self.reasoning_display.is_empty() || !continuation.is_empty() {
            events.push(ModelBlockStreamEvent::ReasoningCompleted {
                output: ReasoningOutput {
                    display_text: std::mem::take(&mut self.reasoning_display),
                    continuation: (!continuation.is_empty())
                        .then_some(ReasoningContinuation::Blocks(continuation)),
                },
            });
        }
        events.push(ModelBlockStreamEvent::Done {
            finish_reason: Some(finish_reason),
            usage,
        });
        Ok(events)
    }

    fn resolve(
        &mut self,
        kind: Kind,
        index: Option<usize>,
        provider_id: Option<&str>,
    ) -> TalosResult<usize> {
        let index = index.or_else(|| provider_id.is_none().then_some(0));
        let index_key = index.map(|index| (kind, index));
        let id_key = provider_id.map(|id| (kind, id.to_owned()));
        let indexed = index_key
            .as_ref()
            .and_then(|key| self.indexes.get(key))
            .copied();
        let named = id_key
            .as_ref()
            .and_then(|key| self.provider_ids.get(key))
            .copied();
        if indexed
            .zip(named)
            .is_some_and(|(indexed, named)| indexed != named)
        {
            return Err(TalosError::internal(
                "The model response used conflicting output item identities.",
            ));
        }
        let slot = indexed.or(named).unwrap_or_else(|| {
            let identity = index
                .map(|value| value.to_string())
                .unwrap_or_else(|| format!("item-{}", self.items.len()));
            let tool = (kind == Kind::Tool).then(|| {
                let ordinal = self.next_tool_index;
                self.next_tool_index += 1;
                Tool {
                    ordinal,
                    ..Tool::default()
                }
            });
            let slot = self.items.len();
            self.items.push(OutputItem {
                kind,
                model_item_id: format!("{}-response-{identity}-{}", self.prefix, kind.key()),
                provider_item_id: None,
                phase: ModelOutputPhase::Unknown,
                parts: BTreeMap::new(),
                encrypted: None,
                tool,
                started: false,
                completed: false,
            });
            slot
        });
        if let Some(key) = index_key {
            self.indexes.insert(key, slot);
        }
        if let Some(key) = id_key {
            let item = &mut self.items[slot];
            if item
                .provider_item_id
                .as_ref()
                .is_some_and(|existing| existing != &key.1)
            {
                return Err(TalosError::internal(
                    "The model response changed an output item's provider ID.",
                ));
            }
            item.provider_item_id.get_or_insert_with(|| key.1.clone());
            self.provider_ids.insert(key, slot);
        }
        Ok(slot)
    }

    fn apply_item(
        &mut self,
        index: Option<usize>,
        provider_id: Option<&str>,
        item: &Value,
        completed: bool,
        events: &mut Vec<ModelBlockStreamEvent>,
    ) -> TalosResult<()> {
        let Some(kind) = Kind::from_item(item) else {
            return Ok(());
        };
        let slot = self.resolve(kind, index, provider_id)?;
        let state = &mut self.items[slot];
        if kind == Kind::Text {
            let phase = ModelOutputPhase::deserialize(&item["phase"]).unwrap_or_default();
            if phase != ModelOutputPhase::Unknown && state.phase != phase {
                if state.phase != ModelOutputPhase::Unknown {
                    return Err(TalosError::internal(
                        "The model response changed an explicit assistant message phase.",
                    ));
                }
                state.phase = phase;
                if state.started {
                    self.final_answer_started |= phase == ModelOutputPhase::FinalAnswer;
                    events.push(ModelBlockStreamEvent::AssistantTextPhase {
                        model_item_id: state.model_item_id.clone(),
                        phase,
                    });
                }
            }
        }
        if let Some(encrypted) = item["encrypted_content"]
            .as_str()
            .filter(|_| kind == Kind::Reasoning)
        {
            state.encrypted = Some(encrypted.to_owned());
        }
        if state.completed {
            return Ok(());
        }
        if let Some(tool) = state.tool.as_mut() {
            if completed && item["status"] == "incomplete" {
                return Err(TalosError::internal(
                    "The model response contained an incomplete tool call.",
                ));
            }
            if let Some(name) = item["name"].as_str() {
                tool.name.get_or_insert_with(|| name.to_owned());
            }
            if let Some(call_id) = item["call_id"].as_str() {
                tool.call_id.get_or_insert_with(|| call_id.to_owned());
            }
            let arguments = argument_text(&item["arguments"]);
            self.update_tool(slot, arguments.as_deref(), true, completed, events)?;
        } else {
            if !state.started && (kind != Kind::Reasoning || !self.final_answer_started) {
                self.final_answer_started |= state.phase == ModelOutputPhase::FinalAnswer;
                state.start(events);
            }
            for update in TextUpdate::from_item(item) {
                self.update_text(slot, update, events)?;
            }
        }
        if completed {
            self.items[slot].complete(events);
        }
        Ok(())
    }

    fn update_text(
        &mut self,
        slot: usize,
        update: TextUpdate<'_>,
        events: &mut Vec<ModelBlockStreamEvent>,
    ) -> TalosResult<()> {
        let item = &mut self.items[slot];
        if item.completed || update.text.is_empty() {
            return Ok(());
        }
        let saved = item.parts.entry(update.part).or_default();
        let delta = if update.snapshot {
            reconcile(
                saved,
                update.text,
                self.contract,
                "The completed model text differs from the streamed text.",
            )?
        } else {
            saved.push_str(update.text);
            update.text
        };
        // The continuation can retain a late reasoning snapshot after final text is already visible.
        if delta.is_empty() || (item.kind == Kind::Reasoning && self.final_answer_started) {
            return Ok(());
        }
        item.start(events);
        let delta = match update.part {
            Part::Text(_) => ModelBlockDelta::AssistantText {
                delta: delta.to_owned(),
            },
            Part::Summary(_) | Part::Reasoning(_) => {
                let part = (slot, update.part);
                if self.last_reasoning_part != Some(part) {
                    self.reasoning_display.push(String::new());
                    self.last_reasoning_part = Some(part);
                }
                self.reasoning_display.last_mut().unwrap().push_str(delta);
                ModelBlockDelta::ReasoningText {
                    delta: delta.to_owned(),
                }
            }
        };
        item.delta(delta, events);
        Ok(())
    }

    fn update_tool(
        &mut self,
        slot: usize,
        arguments: Option<&str>,
        snapshot: bool,
        completed: bool,
        events: &mut Vec<ModelBlockStreamEvent>,
    ) -> TalosResult<()> {
        let item = &mut self.items[slot];
        if item.completed {
            return Ok(());
        }
        let tool = item
            .tool
            .as_mut()
            .expect("tool arguments require a function call item");
        let previous = tool.arguments.len();
        if let Some(arguments) = arguments.filter(|arguments| !arguments.is_empty()) {
            if snapshot {
                reconcile(
                    &mut tool.arguments,
                    arguments,
                    self.contract,
                    "The completed model tool arguments differ from the streamed arguments.",
                )?;
            } else {
                tool.arguments.push_str(arguments);
            }
        }
        let can_start = tool.name.is_some() && (tool.call_id.is_some() || completed);
        if !item.started && !can_start {
            return Ok(());
        }
        let start = if item.started { previous } else { 0 };
        let delta = (start < tool.arguments.len()).then(|| tool.arguments[start..].to_owned());
        item.start(events);
        if let Some(delta) = delta {
            item.delta(ModelBlockDelta::ToolArguments { delta }, events);
        }
        Ok(())
    }
}

fn reconcile<'a>(
    saved: &mut String,
    snapshot: &'a str,
    contract: ResponsesContract,
    mismatch: &'static str,
) -> TalosResult<&'a str> {
    if let Some(suffix) = snapshot.strip_prefix(saved.as_str()) {
        saved.push_str(suffix);
        return Ok(suffix);
    }
    match contract {
        ResponsesContract::Official => Err(TalosError::internal(mismatch)),
        ResponsesContract::Compatible => Ok(""),
    }
}

fn argument_text(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::Null => None,
        Value::String(arguments) => Some(Cow::Borrowed(arguments)),
        value => Some(Cow::Owned(value.to_string())),
    }
}
