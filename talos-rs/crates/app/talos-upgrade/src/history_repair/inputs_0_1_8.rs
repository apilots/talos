//! Recover running user inputs that the old Kernel wrote only to its artifact.
use super::{history_atoms, invalid, normalize, text, DatabaseRepair};
use serde_json::{json, Value};
use talos_agent_api::{AgentEventPayload, AgentTurnRef};
use talos_error::TalosResult;

pub(super) fn supplement(
    session: &str,
    records: &[(usize, String, Value)],
    repair: &mut DatabaseRepair,
) -> TalosResult<()> {
    for (_, _, record) in records {
        if record["kind"] != "turn" {
            continue;
        }
        let turn = &record["turn"];
        let Some(id) = repair
            .message_turns
            .get(&(session.to_owned(), text(turn, "message_id")?.to_owned()))
        else {
            continue;
        };
        let identity = AgentTurnRef {
            session_id: session.to_owned(),
            turn_id: id.clone(),
        };
        let events = repair
            .turns
            .get_mut(&(session.to_owned(), id.clone()))
            .ok_or_else(|| invalid("artifact turn identity missing"))?;
        let items = turn["items"]
            .as_array()
            .ok_or_else(|| invalid("legacy artifact items missing"))?
            .iter()
            .cloned()
            .map(normalize)
            .collect::<TalosResult<Vec<_>>>()?;
        let atoms = history_atoms(events)?;
        if items
            .iter()
            .any(|item| item["type"] != "user_input" && !atoms.contains(item))
        {
            return Err(invalid("artifact contains unmatched non-user content"));
        }
        let mut existing_inputs = atoms
            .into_iter()
            .filter(|atom| atom["type"] == "user_input")
            .collect::<Vec<_>>();
        let original_len = events.len();
        for (index, input) in items
            .iter()
            .enumerate()
            .filter(|(_, item)| item["type"] == "user_input")
        {
            if let Some(position) = existing_inputs.iter().position(|atom| atom == input) {
                existing_inputs.remove(position);
                continue;
            }
            let previous = items[..index]
                .iter()
                .rev()
                .find(|item| item["type"] != "user_input");
            let next = items[index + 1..]
                .iter()
                .find(|item| item["type"] != "user_input");
            let mut lower = 0;
            let mut upper = events.len() - 1;
            for (source, is_next) in [(previous, false), (next, true)] {
                let Some(source) = source else {
                    continue;
                };
                let candidates = events
                    .iter()
                    .enumerate()
                    .filter_map(|(i, event)| {
                        let AgentEventPayload::MessageCompleted(message) = &event.payload else {
                            return None;
                        };
                        match history_atoms(std::slice::from_ref(event)) {
                            Ok(atoms) if atoms.contains(source) => {
                                Some((i, message.message_id.clone()))
                            }
                            _ => None,
                        }
                    })
                    .collect::<Vec<_>>();
                if candidates.len() != 1 {
                    return Err(invalid("running input adjacency is ambiguous"));
                }
                if is_next {
                    upper = events
                        .iter()
                        .position(|event| {
                            matches!(&event.payload, AgentEventPayload::MessageStarted(start)
                                if start.message.message_id == candidates[0].1)
                        })
                        .ok_or_else(|| invalid("next message start missing"))?;
                } else {
                    lower = candidates[0].0;
                }
            }
            if lower >= upper {
                return Err(invalid("running input adjacency order differs"));
            }
            // No timestamp was stored for a running input. Place it at the
            // adjacent canonical boundary, without claiming original timing.
            let at = events[upper].occurred_at;
            let message_id = format!(
                "legacy:{}:input:{}:{index}",
                identity.turn_id,
                text(turn, "turn_id")?
            );
            let mut blocks = vec![
                json!({"type":"userContent","data":{"type":"text","text":text(input,"text")?}}),
            ];
            for file in input["fileIds"]
                .as_array()
                .ok_or_else(|| invalid("running input files missing"))?
            {
                blocks.push(json!({"type":"userContent","data":{"type":"file","fileId":file}}));
            }
            let mut added = Vec::new();
            super::super::events::append_message(
                &identity,
                &mut added,
                json!({"messageId":message_id,"turnId":identity.turn_id,"role":"user","status":"completed","blocks":blocks,"createdAt":at,"completedAt":at}),
            )?;
            events.splice(upper..upper, added);
            tracing::info!(session_id=session,turn_id=%identity.turn_id,artifact_input=index,"recovered running user input from legacy artifact");
        }
        if events.len() != original_len {
            for (index, event) in events.iter_mut().enumerate() {
                event.sequence = index as u64 + 1;
            }
            let mut reducer = talos_agent::AgentEventReducer::new(identity.clone());
            for event in events.iter() {
                reducer.apply(event.clone())?;
            }
        }
    }
    Ok(())
}
