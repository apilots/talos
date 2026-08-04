use std::collections::{HashMap, HashSet};

use super::*;
use talos_llm_provider::Role;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader, SeekFrom};

pub(super) async fn load_recent_artifacts(
    path: &Path,
    session_id: &str,
    limit: usize,
    matches: fn(&SessionArtifactRecord) -> bool,
) -> TalosResult<Vec<SessionArtifactRecord>> {
    const READ_CHUNK_BYTES: u64 = 8 * 1024;

    let mut file = tokio::fs::File::open(path).await.map_err(|error| {
        TalosError::internal(format!("open session artifact {}: {error}", path.display()))
    })?;
    let mut position = file
        .metadata()
        .await
        .map_err(|error| {
            TalosError::internal(format!(
                "read session artifact metadata {}: {error}",
                path.display()
            ))
        })?
        .len();
    let mut carry = Vec::new();
    let mut records = Vec::with_capacity(limit);
    while position > 0 && records.len() < limit {
        let chunk_len = position.min(READ_CHUNK_BYTES);
        position -= chunk_len;
        file.seek(SeekFrom::Start(position))
            .await
            .map_err(|error| {
                TalosError::internal(format!("seek session artifact {}: {error}", path.display()))
            })?;
        let mut data = vec![0; chunk_len as usize];
        file.read_exact(&mut data).await.map_err(|error| {
            TalosError::internal(format!("read session artifact {}: {error}", path.display()))
        })?;
        data.extend_from_slice(&carry);
        let complete_from = if position == 0 {
            0
        } else if let Some(index) = data.iter().position(|byte| *byte == b'\n') {
            carry = data[..index].to_vec();
            index.saturating_add(1)
        } else {
            carry = data;
            continue;
        };
        for line in data[complete_from..].rsplit(|byte| *byte == b'\n') {
            let line = std::str::from_utf8(line).map_err(|error| {
                TalosError::internal(format!(
                    "decode session artifact {} during tail scan: {error}",
                    path.display()
                ))
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let record = parse_artifact_record(path, "tail scan", line, session_id)?;
            if matches(&record) {
                records.push(record);
                if records.len() == limit {
                    break;
                }
            }
        }
    }
    Ok(records)
}

async fn first_file_after_seq(files: &[PathBuf], session_id: &str, seq: u64) -> TalosResult<usize> {
    let mut start = 0_usize;
    let mut end = files.len();
    while start < end {
        let middle = start + (end - start) / 2;
        let mut latest_seq = None;
        for path in files[..=middle].iter().rev() {
            let latest_turn = load_recent_artifacts(path, session_id, 1, |record| {
                matches!(record, SessionArtifactRecord::Turn { .. })
            })
            .await?
            .pop();
            if let Some(SessionArtifactRecord::Turn { turn }) = latest_turn {
                latest_seq = Some(turn.seq);
                break;
            }
        }
        if latest_seq.is_some_and(|latest| latest > seq) {
            end = middle;
        } else {
            start = middle.saturating_add(1);
        }
    }
    Ok(start)
}

pub(super) async fn load_turn_window(
    workspace_root: &Path,
    session_id: &str,
    window: SessionTurnWindow,
    limit: usize,
) -> TalosResult<Vec<TalosTurn>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut files = list_day_files(workspace_root, session_id).await?;
    match window {
        SessionTurnWindow::Latest => files.reverse(),
        SessionTurnWindow::OldestAfter { seq } if seq > 0 => {
            let start = first_file_after_seq(&files, session_id, seq).await?;
            files = files.split_off(start);
        }
        SessionTurnWindow::OldestAfter { .. } => {}
    }
    let mut turns = Vec::with_capacity(limit);
    for path in files {
        if matches!(window, SessionTurnWindow::OldestAfter { .. }) {
            let file = tokio::fs::File::open(&path).await.map_err(|error| {
                TalosError::internal(format!("open session artifact {}: {error}", path.display()))
            })?;
            let mut lines = BufReader::new(file).lines();
            let mut line_index = 0_usize;
            while let Some(line) = lines.next_line().await.map_err(|error| {
                TalosError::internal(format!("read session artifact {}: {error}", path.display()))
            })? {
                if !line.trim().is_empty() {
                    let SessionArtifactRecord::Turn { turn } =
                        parse_artifact_line(&path, line_index, &line, session_id)?
                    else {
                        line_index = line_index.saturating_add(1);
                        continue;
                    };
                    if matches!(window, SessionTurnWindow::OldestAfter { seq } if turn.seq > seq) {
                        turns.push(turn);
                        if turns.len() == limit {
                            break;
                        }
                    }
                }
                line_index = line_index.saturating_add(1);
            }
            if turns.len() == limit {
                break;
            }
            continue;
        }
        let records = load_recent_artifacts(&path, session_id, limit - turns.len(), |record| {
            matches!(record, SessionArtifactRecord::Turn { .. })
        })
        .await?;
        for record in records {
            if let SessionArtifactRecord::Turn { turn } = record {
                turns.push(turn);
            }
        }
        if turns.len() == limit {
            break;
        }
    }
    turns.sort_by(|left, right| {
        left.seq
            .cmp(&right.seq)
            .then(left.timestamp.cmp(&right.timestamp))
    });
    Ok(turns)
}

pub(super) fn synthesized_cancelled_turns(
    session_id: &str,
    artifacts: &[SessionArtifactRecord],
    turns: &[TalosTurn],
) -> Vec<TalosTurn> {
    let turn_ids = turns
        .iter()
        .map(|turn| turn.turn_id.as_str())
        .collect::<HashSet<_>>();
    let canonical_inputs = turns
        .iter()
        .flat_map(|turn| {
            turn.items.iter().filter_map(move |item| match item {
                RunItem::UserInput { text, .. } => Some((text.as_str(), turn.timestamp)),
                _ => None,
            })
        })
        .collect::<Vec<_>>();
    let mut interrupted = HashMap::<String, (DateTime<Utc>, Option<String>, bool)>::new();
    for artifact in artifacts {
        let SessionArtifactRecord::Transcript { record } = artifact else {
            continue;
        };
        if turn_ids.contains(record.header.invocation_id.as_str()) {
            continue;
        }
        let group = interrupted
            .entry(record.header.invocation_id.clone())
            .or_insert((record.header.timestamp, None, false));
        group.0 = group.0.min(record.header.timestamp);
        let SessionTranscriptPayload::Message { message } = &record.payload else {
            continue;
        };
        let text = message.content.trim();
        if text.is_empty() {
            continue;
        }
        match message.role {
            Role::User if group.1.is_none() => {
                group.0 = record.header.timestamp;
                group.1 = Some(text.to_string());
            }
            Role::Assistant => group.2 = true,
            Role::System | Role::Tool | Role::User => {}
        }
    }
    let first_seq = turns
        .iter()
        .map(|turn| turn.seq)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let mut interrupted = interrupted.into_iter().collect::<Vec<_>>();
    interrupted.sort_by_key(|(_, group)| group.0);
    interrupted
        .into_iter()
        .filter_map(|(invocation_id, (timestamp, user_text, has_response))| {
            (!has_response).then_some((invocation_id, timestamp, user_text?))
        })
        .filter(|(_, timestamp, user_text)| {
            !canonical_inputs
                .iter()
                .any(|(text, turn_timestamp)| *text == user_text && *turn_timestamp >= *timestamp)
        })
        .enumerate()
        .map(|(index, (turn_id, timestamp, text))| TalosTurn {
            turn_id,
            message_id: String::new(),
            session_id: session_id.to_string(),
            seq: first_seq.saturating_add(index as u64),
            timestamp,
            items: vec![RunItem::UserInput {
                text,
                file_ids: Vec::new(),
            }],
            usage: TokenUsage::default(),
            stop_reason: StopReason::Cancelled,
        })
        .collect()
}
