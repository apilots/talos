use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use sqlx::{Executor, SqliteConnection};
use talos_agent_api::{AgentEvent, AgentTurnRef};
use talos_error::TalosResult;

use super::{
    events::{integer, text, LegacyTurn},
    invalid, source_error,
};

type EventRow = (i64, String, String, Option<i64>, i64, i64, String);
pub(super) type TurnKey = (String, String);

#[derive(Default)]
pub(super) struct DatabaseRepair {
    pub legacy_exists: bool,
    pub turns: BTreeMap<TurnKey, Vec<AgentEvent>>,
    pub message_turns: BTreeMap<(String, String), String>,
    pub cursors: BTreeMap<i64, i64>,
    pub turn_ends: BTreeMap<TurnKey, i64>,
    rows: Vec<EventRow>,
    order: BTreeMap<TurnKey, i64>,
    sessions: BTreeSet<String>,
}

pub(super) async fn prepare(db: &mut SqliteConnection) -> TalosResult<DatabaseRepair> {
    let legacy_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'legacy_timeline_events' AND type = 'table')")
        .fetch_one(&mut *db).await.map_err(source_error)?;
    let mut result = DatabaseRepair {
        legacy_exists,
        ..Default::default()
    };
    if !legacy_exists {
        return Ok(result);
    }
    let rows: Vec<(i64,String,String,Option<String>,String,i64)> = sqlx::query_as("SELECT event_seq,session_id,turn_id,message_id,payload_json,created_at_ms FROM legacy_timeline_events ORDER BY event_seq")
        .fetch_all(&mut *db).await.map_err(source_error)?;
    let mut turns: BTreeMap<TurnKey, LegacyTurn> = BTreeMap::new();
    for (seq, session, turn, message, raw, at) in rows {
        let event: Value = serde_json::from_str(&raw).map_err(source_error)?;
        if text(&event, "turn_id")? != turn {
            return Err(
                invalid("legacy turn identity mismatch").context(format!("legacy event {seq}"))
            );
        }
        let key = (session.clone(), turn.clone());
        if event["type"] == "turn_started" {
            if let Some(message) = message {
                result
                    .message_turns
                    .insert((session.clone(), message), turn.clone());
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = turns.entry(key.clone()) {
                result.order.insert(key.clone(), seq);
                entry.insert(LegacyTurn {
                    identity: AgentTurnRef {
                        session_id: session,
                        turn_id: turn,
                    },
                    started_at: integer(&event, "started_at_ms")?,
                    last_at: at,
                    items: Vec::new(),
                    terminal: None,
                    metrics: None,
                });
                continue;
            }
        }
        turns
            .get_mut(&key)
            .ok_or_else(|| invalid("legacy turn start missing"))?
            .apply(event, at)
            .map_err(|e| e.context(format!("legacy event {seq}")))?;
    }
    for (key, turn) in turns {
        let live: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM agent_active_turns WHERE session_id=? AND turn_id=?)",
        )
        .bind(&key.0)
        .bind(&key.1)
        .fetch_one(&mut *db)
        .await
        .map_err(source_error)?;
        if live {
            return Err(invalid("legacy turn overlaps live canonical execution"));
        }
        if turn
            .terminal
            .as_ref()
            .is_some_and(|event| event["stop_reason"] == "waiting_user_action")
        {
            let mut pending = BTreeSet::new();
            for item in &turn.items {
                if let Some(actions) = item["legacyActions"].as_array() {
                    for action in actions {
                        let event = &action["event"];
                        match text(event, "type")? {
                            "requested" => {
                                pending.insert(text(&event["action"], "actionId")?.to_owned());
                            }
                            "finished" => {
                                pending.remove(text(event, "actionId")?);
                            }
                            _ => return Err(invalid("unknown user action history")),
                        }
                    }
                }
            }
            if !pending.is_empty() {
                return Err(invalid("legacy user action must be settled before upgrade"));
            }
        }
        result.sessions.insert(key.0.clone());
        result.turns.insert(
            key.clone(),
            turn.convert().map_err(|error| {
                error.context(format!("convert session {} turn {}", key.0, key.1))
            })?,
        );
    }
    Ok(result)
}

impl DatabaseRepair {
    pub async fn finalize(&mut self, db: &mut SqliteConnection) -> TalosResult<()> {
        let result = self;
        if result.sessions.is_empty() {
            return Ok(());
        }
        let existing: Vec<EventRow> = sqlx::query_as("SELECT event_seq,session_id,turn_id,sequence,occurred_at,persisted_at,payload_json FROM agent_events ORDER BY event_seq")
        .fetch_all(&mut *db).await.map_err(source_error)?;
        let mut grouped: BTreeMap<TurnKey, Vec<EventRow>> = BTreeMap::new();
        let mut high: i64 = sqlx::query_scalar(
            "SELECT coalesce(max(seq),0) FROM sqlite_sequence WHERE name='agent_events'",
        )
        .fetch_one(&mut *db)
        .await
        .map_err(source_error)?;
        for row in existing {
            high = high.max(row.0);
            if result.sessions.contains(&row.1) {
                grouped
                    .entry((row.1.clone(), row.2.clone()))
                    .or_default()
                    .push(row);
            }
        }
        for (key, events) in &result.turns {
            let canonical = grouped.entry(key.clone()).or_default();
            let existing_events = canonical
                .iter()
                .filter(|row| row.3.is_some())
                .collect::<Vec<_>>();
            if !existing_events.is_empty() {
                let identity = AgentTurnRef {
                    session_id: key.0.clone(),
                    turn_id: key.1.clone(),
                };
                let mut actual = talos_agent::AgentEventReducer::new(identity.clone());
                let mut expected = talos_agent::AgentEventReducer::new(identity.clone());
                for row in &existing_events {
                    actual.apply(AgentEvent {
                        identity: identity.clone(),
                        sequence: row.3.unwrap() as u64,
                        occurred_at: row.4,
                        payload: serde_json::from_str(&row.6).map_err(source_error)?,
                    })?;
                }
                for event in events {
                    expected.apply(event.clone())?;
                }
                let terminal: talos_agent_api::AgentEventPayload = serde_json::from_str(
                    &existing_events
                        .last()
                        .ok_or_else(|| invalid("empty imported turn"))?
                        .6,
                )
                .map_err(source_error)?;
                if actual.checkpoint().messages != expected.checkpoint().messages
                    || events.last().is_none_or(|last| last.payload != terminal)
                {
                    return Err(invalid("existing import differs from legacy history")
                        .context(format!("session {} turn {}", key.0, key.1)));
                }
            } else {
                canonical.extend(
                    events
                        .iter()
                        .map(|event| {
                            Ok((
                                0,
                                key.0.clone(),
                                key.1.clone(),
                                Some(event.sequence as i64),
                                event.occurred_at,
                                event.occurred_at,
                                serde_json::to_string(&event.payload).map_err(source_error)?,
                            ))
                        })
                        .collect::<TalosResult<Vec<_>>>()?,
                );
            }
        }
        let mut ordered = grouped.into_iter().collect::<Vec<_>>();
        ordered.sort_by_key(|(key, rows)| match result.order.get(key) {
            Some(seq) => (0, *seq),
            None => (1, rows.iter().map(|row| row.0).min().unwrap_or(0)),
        });
        for (key, rows) in ordered {
            for mut row in rows {
                high = high
                    .checked_add(1)
                    .ok_or_else(|| invalid("event cursor overflow"))?;
                if row.0 > 0 {
                    result.cursors.insert(row.0, high);
                }
                row.0 = high;
                if row.3.is_some() {
                    result.turn_ends.insert(key.clone(), high);
                }
                result.rows.push(row);
            }
        }
        Ok(())
    }

    pub async fn apply(&self, db: &mut SqliteConnection) -> TalosResult<()> {
        // Reassign above the old high-water mark so an old client cursor expires
        // rather than accidentally naming a different imported event.
        (&mut *db).execute("CREATE TEMP TABLE history_repair_cursor_0_1_8 (old_seq INTEGER PRIMARY KEY, new_seq INTEGER NOT NULL);").await.map_err(source_error)?;
        for (old, new) in &self.cursors {
            sqlx::query("INSERT INTO history_repair_cursor_0_1_8 VALUES (?,?)")
                .bind(old)
                .bind(new)
                .execute(&mut *db)
                .await
                .map_err(source_error)?;
        }
        for session in &self.sessions {
            sqlx::query("DELETE FROM agent_events WHERE session_id=?")
                .bind(session)
                .execute(&mut *db)
                .await
                .map_err(source_error)?;
        }
        for (seq, session, turn, sequence, at, persisted, raw) in &self.rows {
            sqlx::query("INSERT INTO agent_events(event_seq,session_id,turn_id,sequence,occurred_at,persisted_at,payload_json) VALUES (?,?,?,?,?,?,?)")
                .bind(seq).bind(session).bind(turn).bind(sequence).bind(at).bind(persisted).bind(raw).execute(&mut *db).await.map_err(source_error)?;
        }
        tracing::info!(
            sessions = self.sessions.len(),
            legacy_turns = self.turns.len(),
            events = self.rows.len(),
            "canonical history repair validated and written"
        );
        Ok(())
    }
}

/// Retain the pre-SQLx database before released migrations remove legacy state.
pub(super) async fn backup_legacy_database(
    pool: &sqlx::SqlitePool,
    directory: &std::path::Path,
) -> TalosResult<()> {
    let table:Option<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name IN ('timeline_events','legacy_timeline_events') LIMIT 1")
        .fetch_optional(pool).await.map_err(source_error)?;
    let Some(table) = table else {
        return Ok(());
    };
    let populated: bool = sqlx::query_scalar(&format!("SELECT EXISTS(SELECT 1 FROM {table})"))
        .fetch_one(pool)
        .await
        .map_err(source_error)?;
    if !populated {
        return Ok(());
    }
    let target = directory.join("database.sqlite");
    if target.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(directory).map_err(source_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(source_error)?;
    }
    let temporary = directory.join("database.sqlite.partial");
    // This filename belongs exclusively to an unfinished VACUUM, never user data.
    if temporary.exists() {
        std::fs::remove_file(&temporary).map_err(source_error)?;
    }
    sqlx::query("VACUUM INTO ?")
        .bind(
            temporary
                .to_str()
                .ok_or_else(|| invalid("non-UTF8 backup path"))?,
        )
        .fetch_all(pool)
        .await
        .map_err(source_error)?;
    std::fs::File::open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|e| {
            source_error(e).context(format!("sync database backup {}", temporary.display()))
        })?;
    std::fs::rename(&temporary, &target).map_err(source_error)?;
    std::fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(source_error)?;
    tracing::info!(path=%target.display(),"pre-upgrade database backup retained");
    Ok(())
}
