use serde_json::{json, Value};
use talos_agent::AgentEventReducer;
use talos_agent_api::{AgentEvent, AgentTurnRef};
use talos_error::TalosResult;

use super::{decode, invalid};

pub(super) struct LegacyTurn {
    pub identity: AgentTurnRef,
    pub started_at: i64,
    pub last_at: i64,
    pub items: Vec<Value>,
    pub terminal: Option<Value>,
    pub metrics: Option<Value>,
}

impl LegacyTurn {
    pub fn apply(&mut self, event: Value, at: i64) -> TalosResult<()> {
        if event["type"] == "turn_started"
            && self
                .terminal
                .as_ref()
                .is_some_and(|terminal| terminal["stop_reason"] == "waiting_user_action")
        {
            self.terminal = None;
            return Ok(());
        }
        let settled_wait = self
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal["stop_reason"] == "waiting_user_action")
            && event["type"] == "item_delta"
            && event["delta"]["type"] == "user_action"
            && event["delta"]["event"]["type"] == "finished";
        if self.terminal.is_some() && !settled_wait {
            return Err(invalid("legacy event follows terminal"));
        }
        self.last_at = at;
        match text(&event, "type")? {
            "turn_completed" | "turn_error" => self.terminal = Some(event),
            "turn_metrics" => self.metrics = Some(event["metrics"].clone()),
            "item_started" => {
                let item = &event["item"];
                let id = text(item, "item_id")?;
                if self.items.iter().any(|other| other["item_id"] == id) {
                    return Err(invalid("duplicate legacy item"));
                }
                self.items.push(item.clone());
            }
            kind => {
                let id = text(&event, "item_id")?;
                let item = self
                    .items
                    .iter_mut()
                    .find(|item| item["item_id"] == id)
                    .ok_or_else(|| invalid("legacy item start missing"))?;
                match kind {
                    "assistant_text_committed" => item["phase"] = event["phase"].clone(),
                    "item_completed" => {
                        if !item["completed_at_ms"].is_null() {
                            return Err(invalid("duplicate legacy item completion"));
                        }
                        if !event["final_content"].is_null() {
                            item["content"] = event["final_content"].clone();
                        }
                        item["status"] = event["status"].clone();
                        item["completed_at_ms"] = event["completed_at_ms"].clone();
                    }
                    "item_delta" => apply_delta(item, &event["delta"], at)?,
                    _ => return Err(invalid("unknown legacy event variant")),
                }
            }
        }
        Ok(())
    }

    pub fn convert(&self) -> TalosResult<Vec<AgentEvent>> {
        let mut events = Vec::new();
        emit(
            &self.identity,
            &mut events,
            "turnStarted",
            json!({}),
            self.started_at,
        )?;
        for item in &self.items {
            let (role, blocks) = blocks(item)?;
            let start = integer(item, "started_at_ms")?;
            let end = item["completed_at_ms"].as_i64().unwrap_or(self.last_at);
            let status = match text(item, "status")? {
                "completed" => "completed",
                "failed" => "failed",
                "cancelled"
                | "queued"
                | "running"
                | "waiting_dynamic_input"
                | "waiting_user_action" => "cancelled",
                _ => return Err(invalid("unknown legacy item status")),
            };
            let id = format!(
                "legacy:{}:{}",
                self.identity.turn_id,
                text(item, "item_id")?
            );
            append_message(
                &self.identity,
                &mut events,
                json!({"messageId":id,"turnId":self.identity.turn_id,
                "role":role,"status":status,"blocks":blocks,"createdAt":start,"completedAt":end,"legacyActions":item.get("legacyActions").cloned().unwrap_or(json!([]))}),
            )?;
            let content = &item["content"];
            if content["type"] == "tool_call" && !content["result"].is_null() {
                let result = &content["result"];
                let is_error = result
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| invalid("legacy tool result flag missing"))?;
                append_message(
                    &self.identity,
                    &mut events,
                    json!({"messageId":format!("{id}:result"),"turnId":self.identity.turn_id,
                    "role":"tool","status":if is_error {"failed"} else {"completed"},
                    "blocks":[{"type":"toolResult","data":{"callId":content["call_id"],"toolName":content["name"],"content":result["content"],"isError":is_error}}],
                    "createdAt":end,"completedAt":end}),
                )?;
            }
        }
        let terminal = self.terminal.as_ref();
        let metrics = terminal
            .and_then(|event| event.get("metrics"))
            .filter(|v| !v.is_null())
            .or(self.metrics.as_ref());
        if let Some(metrics) = metrics {
            emit(
                &self.identity,
                &mut events,
                "metricsUpdated",
                json!({
                    "modelRounds":metrics["model_rounds"],"toolCalls":metrics["tool_calls"],
                    "toolSuccesses":metrics["tool_successes"],"toolErrors":metrics["tool_errors"],
                    "usage":metrics["usage"],"durationMs":metrics["duration_ms"],"context":null
                }),
                self.last_at,
            )?;
        } else if let Some(usage) = terminal.and_then(|event| event.get("usage")) {
            let metrics = talos_agent_api::AgentRunMetrics {
                usage: decode(usage.clone())?,
                ..Default::default()
            };
            emit(
                &self.identity,
                &mut events,
                "metricsUpdated",
                serde_json::to_value(metrics).map_err(super::source_error)?,
                self.last_at,
            )?;
        }
        let reason = terminal.and_then(|event| event["stop_reason"].as_str());
        if reason == Some("tool_round_limit") {
            append_message(
                &self.identity,
                &mut events,
                json!({"messageId":format!("legacy:{}:tool-round-limit",self.identity.turn_id),"turnId":self.identity.turn_id,
                "role":"system","status":"completed","blocks":[{"type":"systemNotice","data":{"level":"info","content":{"type":"text","data":"The previous execution reached its tool round budget and produced a final summary."}}}],
                "createdAt":self.last_at,"completedAt":self.last_at}),
            )?;
        }
        match reason {
            Some("end_turn" | "tool_round_limit") | Some("max_tokens") => emit(
                &self.identity,
                &mut events,
                "turnCompleted",
                json!({"stopReason":if reason == Some("max_tokens") {"outputLimit"} else {"endTurn"}}),
                self.last_at,
            )?,
            Some("cancelled" | "waiting_user_action") | None => {
                let (code, message) = match reason {
                    Some("cancelled") => ("cancelled", "The previous model execution was cancelled."),
                    Some("waiting_user_action") => ("runForcedAbort", "The previous execution was waiting for input and cannot resume after the upgrade. Send a new message to continue."),
                                        None if terminal.is_none() => ("runForcedAbort", "The previous execution was interrupted before the upgrade. Send a new message to continue."),
                    None if terminal.is_some_and(|event|event["error_info"]["code"]=="run_forced_abort") => ("runForcedAbort", "The interrupted execution was settled before the upgrade."),
                    None => ("internalError", "The previous model execution failed; legacy technical details were withheld."),
                    _ => unreachable!(),
                };
                emit(
                    &self.identity,
                    &mut events,
                    "turnFailed",
                    json!({"error":{"code":code,"message":message,"localization":null},"retryable":false}),
                    self.last_at,
                )?;
            }
            _ => return Err(invalid("unknown legacy turn stop reason")),
        }
        let mut reducer = AgentEventReducer::new(self.identity.clone());
        for event in &events {
            reducer.apply(event.clone())?;
        }
        Ok(events)
    }
}

fn emit(
    identity: &AgentTurnRef,
    events: &mut Vec<AgentEvent>,
    kind: &str,
    data: Value,
    at: i64,
) -> TalosResult<()> {
    events.push(AgentEvent {
        identity: identity.clone(),
        sequence: events.len() as u64 + 1,
        occurred_at: at,
        payload: decode(json!({"type":kind,"data":data}))?,
    });
    Ok(())
}

pub(super) fn append_message(
    identity: &AgentTurnRef,
    events: &mut Vec<AgentEvent>,
    mut message: Value,
) -> TalosResult<()> {
    let actions = message
        .as_object_mut()
        .ok_or_else(|| invalid("invalid converted message"))?
        .remove("legacyActions")
        .unwrap_or(json!([]));
    let contents = message["blocks"]
        .as_array()
        .ok_or_else(|| invalid("missing converted blocks"))?
        .clone();
    let status = message["status"].clone();
    let end = message["completedAt"].clone();
    let at = integer(&message, "createdAt")?;
    message["status"] = json!("inProgress");
    message["completedAt"] = Value::Null;
    message["blocks"] = json!([]);
    emit(
        identity,
        events,
        "messageStarted",
        json!({"message":message}),
        at,
    )?;
    let mut blocks = Vec::new();
    for (index, content) in contents.into_iter().enumerate() {
        let mut block = json!({"blockId":format!("{}:block:{index}", text(&message,"messageId")?),
                "status":"running","content":content,"startedAt":at,"completedAt":null});
        emit(
            identity,
            events,
            "messageBlockStarted",
            json!({"messageId":message["messageId"],"block":block}),
            at,
        )?;
        if index == 0 {
            let mut pending = std::collections::BTreeSet::new();
            for action in actions
                .as_array()
                .ok_or_else(|| invalid("invalid action history"))?
            {
                let at = integer(action, "at")?;
                match decode::<talos_types::TimelineUserActionEvent>(action["event"].clone())? {
                    talos_types::TimelineUserActionEvent::Requested { mut action } => {
                        if !pending.insert(action.action_id.clone()) {
                            return Err(invalid("duplicate user action request"));
                        }
                        action.item_id = Some(text(&block, "blockId")?.to_owned());
                        emit(
                            identity,
                            events,
                            "waitingForInput",
                            json!({"type":"userAction","action":action}),
                            at,
                        )?;
                    }
                    talos_types::TimelineUserActionEvent::Finished {
                        action_id,
                        state,
                        response,
                    } => {
                        if !pending.remove(&action_id) {
                            return Err(invalid("user action response lacks request"));
                        }
                        if let Some(response) = response {
                            append_message(
                                identity,
                                events,
                                json!({"messageId":format!("legacy:action:{action_id}"),"turnId":identity.turn_id,
                                    "role":"user","status":"completed","blocks":[{"type":"userContent","data":{"type":"userActionResponse","actionId":action_id,"response":response}}],
                                    "createdAt":at,"completedAt":at}),
                            )?;
                        } else if state == talos_types::UserActionState::Resolved {
                            return Err(invalid("resolved user action lacks response"));
                        }
                    }
                }
            }
        }
        block["status"] = status.clone();
        block["completedAt"] = end.clone();
        emit(
            identity,
            events,
            "messageBlockCompleted",
            json!({"messageId":message["messageId"],"block":block}),
            end.as_i64().ok_or_else(|| invalid("missing end time"))?,
        )?;
        blocks.push(block);
    }
    message["blocks"] = json!(blocks);
    message["status"] = status;
    message["completedAt"] = end;
    emit(
        identity,
        events,
        "messageCompleted",
        message.clone(),
        integer(&message, "completedAt")?,
    )
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> TalosResult<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| invalid(format!("missing string field {key}")))
}
pub(super) fn integer(value: &Value, key: &str) -> TalosResult<i64> {
    value[key]
        .as_i64()
        .ok_or_else(|| invalid(format!("missing integer field {key}")))
}

fn apply_delta(item: &mut Value, delta: &Value, at: i64) -> TalosResult<()> {
    if !item["completed_at_ms"].is_null() {
        return Err(invalid("delta follows completed item"));
    }
    let content = &mut item["content"];
    match text(delta, "type")? {
        "text" => {
            content["text"] = json!(format!(
                "{}{}",
                text(content, "text")?,
                text(delta, "text")?
            ))
        }
        "tool_arguments" => {
            content["arguments"] = json!(format!(
                "{}{}",
                text(content, "arguments")?,
                text(delta, "arguments_delta")?
            ))
        }
        "tool_progress" => content["progress"]
            .as_array_mut()
            .ok_or_else(|| invalid("tool progress missing"))?
            .push(json!({
                "progress_kind": delta["progress_kind"],
                "text": delta["text_delta"],
                "timestamp_ms": at
            })),
        "tool_result" => {
            content["result"] = json!({"content":delta["content"],"is_error":delta["is_error"]})
        }
        "context_compaction" => {
            if content["type"] != "context_compaction" {
                return Err(invalid("compaction delta on different item"));
            }
            for key in ["stage", "outcome", "metrics", "message"] {
                if !delta[key].is_null() {
                    content[key] = delta[key].clone();
                }
            }
        }
        "status" => item["status"] = delta["status"].clone(),
        // Old action requests are also preserved in Provider transcript artifacts.
        // They cannot become live continuations under the new execution protocol.
        "user_action" => {
            let _: talos_types::TimelineUserActionEvent = decode(delta["event"].clone())?;
            let actions = item
                .as_object_mut()
                .ok_or_else(|| invalid("invalid item"))?
                .entry("legacyActions")
                .or_insert(json!([]));
            actions
                .as_array_mut()
                .ok_or_else(|| invalid("invalid action list"))?
                .push(json!({"event":delta["event"],"at":at}));
        }
        _ => return Err(invalid("unknown legacy delta variant")),
    }
    Ok(())
}

fn blocks(item: &Value) -> TalosResult<(&'static str, Vec<Value>)> {
    let c = &item["content"];
    let kind = text(item, "kind")?;
    let mut blocks = Vec::new();
    let role = match kind {
        "user_input" => {
            blocks
                .push(json!({"type":"userContent","data":{"type":"text","text":text(c,"text")?}}));
            if let Some(files) = c.get("fileIds") {
                for file in files.as_array().ok_or_else(|| invalid("invalid fileIds"))? {
                    blocks.push(json!({"type":"userContent","data":{"type":"file","fileId":file}}));
                }
            }
            "user"
        }
        "reasoning" => {
            blocks.push(json!({"type":"reasoning","data":{"text":text(c,"text")?}}));
            "agent"
        }
        "assistant_text" => {
            let phase = match text(item, "phase")? {
                "unknown" => "pending",
                "commentary" => "commentary",
                "final_answer" => "finalAnswer",
                _ => return Err(invalid("unknown assistant phase")),
            };
            blocks.push(
                json!({"type":"assistantText","data":{"text":text(c,"text")?,"phase":phase}}),
            );
            "agent"
        }
        "tool_call" => {
            let raw = text(c, "arguments")?;
            let arguments = match serde_json::from_str::<talos_types::ToolArguments>(raw) {
                Ok(value) => json!({"state":"complete","value":value}),
                Err(_) if item["status"] != "completed" => {
                    json!({"state":"streaming","jsonFragment":raw})
                }
                Err(error) => return Err(super::source_error(error)),
            };
            let progress = c["progress"]
                .as_array()
                .ok_or_else(|| invalid("tool progress missing"))?
                .iter()
                .map(|progress| {
                    json!({
                        "kind": progress["progress_kind"],
                        "text": progress["text"],
                        "occurredAt": progress["timestamp_ms"]
                    })
                })
                .collect::<Vec<_>>();
            blocks.push(json!({"type":"toolCall","data":{"callId":c["call_id"],"name":c["name"],"arguments":arguments,"progress":progress}}));
            "agent"
        }
        "execution_summary" => {
            blocks.push(json!({"type":"executionSummary","data":{"text":text(c,"summary")?}}));
            "agent"
        }
        "context_compaction" => {
            let mut data = c.clone();
            data.as_object_mut()
                .ok_or_else(|| invalid("invalid compaction"))?
                .remove("type");
            blocks.push(json!({"type":"contextCompaction","data":data}));
            "system"
        }
        "system_notice" => {
            blocks.push(json!({"type":"systemNotice","data":{"level":c["level"],"content":{"type":"text","data":text(c,"message")?}}}));
            "system"
        }
        _ => return Err(invalid("unknown legacy item kind")),
    };
    Ok((role, blocks))
}
