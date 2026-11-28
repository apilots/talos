use talos_agent_api::{AgentMessage, AgentMessageBlockContent, AgentRunMetrics, AgentTurnRef};
use talos_error::{ErrorCode, TalosError, TalosResult};

use super::{database_error, decode, WorkerExecutionStore};

// Only metadata enters the window sort. Completed bodies are joined after the
// byte/record boundary is known. An admitted completed input may have no start.
const MESSAGE_FACTS: &str = "WITH starts AS (
    SELECT json_extract(event_json,'$.payload.data.message.messageId') AS message_id,min(event_offset) AS creation_offset
    FROM agent_events WHERE session_id=? AND turn_id=? AND generation=(SELECT generation FROM sessions WHERE session_id=?) AND json_extract(event_json,'$.payload.type')='messageStarted' GROUP BY message_id
), facts AS (
    SELECT e.event_offset,length(CAST(e.event_json AS BLOB)) AS bytes,COALESCE(starts.creation_offset,e.event_offset) AS message_position,
        json_extract(e.event_json,'$.payload.data.messageId') AS message_id,json_extract(e.event_json,'$.payload.data.role') AS role
    FROM agent_events e LEFT JOIN starts ON starts.message_id=json_extract(e.event_json,'$.payload.data.messageId')
    WHERE e.session_id=? AND e.turn_id=? AND e.generation=(SELECT generation FROM sessions WHERE session_id=?) AND json_extract(e.event_json,'$.payload.type')='messageCompleted'
)";

/// A bounded canonical message window with its contiguous local execution coverage.
pub struct ExecutionHistoryMessages {
    pub messages: Vec<AgentMessage>,
    pub message_event_sequences: Vec<(String, i64)>,
    pub covered_until_event_seq: i64,
    pub has_more: bool,
    pub bytes: usize,
}

impl WorkerExecutionStore {
    /// Read completed bodies in canonical creation order. The progress position
    /// belongs to the durable creation fact, so late completion cannot skip a
    /// later-created Tool result or User input when resuming inside a Turn.
    pub async fn history_messages(
        &self,
        identity: &AgentTurnRef,
        after: Option<i64>,
        limit: usize,
        byte_budget: usize,
    ) -> TalosResult<ExecutionHistoryMessages> {
        let limit = limit.clamp(1, 128);
        let direction = if after.is_some() { "ASC" } else { "DESC" };
        let query = format!("{MESSAGE_FACTS}, candidates AS (
            SELECT * FROM facts WHERE message_position>? ORDER BY message_position {direction} LIMIT ?
        ), bounded AS (
            SELECT *,row_number() OVER (ORDER BY message_position {direction}) AS position,sum(bytes) OVER (ORDER BY message_position {direction}) AS total FROM candidates
        ) SELECT message_position,CASE WHEN position<=? AND bytes<=? AND total<=? THEN json_extract(body.event_json,'$.payload.data') END,bytes
          FROM bounded JOIN agent_events body ON body.event_offset=bounded.event_offset ORDER BY message_position {direction}");
        let rows: Vec<(i64, Option<String>, i64)> = sqlx::query_as(&query)
            .bind(&identity.session_id)
            .bind(&identity.turn_id)
            .bind(&identity.session_id)
            .bind(&identity.session_id)
            .bind(&identity.turn_id)
            .bind(&identity.session_id)
            .bind(after.unwrap_or_default())
            .bind((limit + 1) as i64)
            .bind(limit as i64)
            .bind(agent_protocol::MAX_EXECUTION_EVENT_BYTES as i64)
            // Reserve the remaining bytes for required input/invocation context.
            .bind((byte_budget / 2) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        let mut page = ExecutionHistoryMessages {
            messages: Vec::new(),
            message_event_sequences: Vec::new(),
            covered_until_event_seq: after.unwrap_or_default(),
            has_more: false,
            bytes: 0,
        };
        for (index, (position, body, bytes)) in rows.into_iter().enumerate() {
            if index == limit {
                page.has_more = true;
                break;
            }
            if bytes as usize > agent_protocol::MAX_EXECUTION_EVENT_BYTES {
                return Err(history_limit());
            }
            let Some(body) = body else {
                page.has_more = true;
                break;
            };
            let message: AgentMessage = decode(&body)?;
            page.message_event_sequences
                .push((message.message_id.clone(), position));
            page.messages.push(message);
            page.covered_until_event_seq = page.covered_until_event_seq.max(position);
            page.bytes += bytes as usize;
        }
        if after.is_none() {
            page.messages.reverse();
            page.message_event_sequences.reverse();
        }
        if page.messages.is_empty() && page.has_more {
            return Err(history_limit());
        }
        self.history_companions(identity, &mut page, byte_budget, after)
            .await?;
        Ok(page)
    }

    async fn history_companions(
        &self,
        identity: &AgentTurnRef,
        page: &mut ExecutionHistoryMessages,
        byte_budget: usize,
        after: Option<i64>,
    ) -> TalosResult<()> {
        let Some(first) = page
            .message_event_sequences
            .iter()
            .map(|(_, position)| *position)
            .min()
        else {
            return Ok(());
        };
        let call_ids = page
            .messages
            .iter()
            .flat_map(|message| &message.blocks)
            .filter_map(|block| match &block.content {
                AgentMessageBlockContent::ToolResult(result) => Some(result.call_id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let needs_user = !page.messages.iter().any(|message| {
            message.role == talos_agent_api::AgentMessageRole::User
                || message.blocks.iter().any(|block| {
                    matches!(
                        &block.content,
                        AgentMessageBlockContent::UserContent(_)
                            | AgentMessageBlockContent::AssistantText(_)
                    )
                })
        });
        let query = format!("{MESSAGE_FACTS}, candidates AS (
            SELECT facts.* FROM facts JOIN agent_events body ON body.event_offset=facts.event_offset
            WHERE message_position<? AND ((? AND message_position=(SELECT max(message_position) FROM facts WHERE role='user' AND message_position<?))
                OR EXISTS (SELECT 1 FROM json_each(body.event_json,'$.payload.data.blocks') block JOIN json_each(?) required ON json_extract(block.value,'$.content.data.callId')=required.value WHERE json_extract(block.value,'$.content.type')='toolCall'))
            ORDER BY message_position DESC LIMIT 129
        ), bounded AS (
            SELECT *,row_number() OVER (ORDER BY message_position DESC) AS position,sum(bytes) OVER (ORDER BY message_position DESC) AS total FROM candidates
        ) SELECT message_position,CASE WHEN position<=128 AND bytes<=? AND total<=? THEN json_extract(body.event_json,'$.payload.data') END,bytes
          FROM bounded JOIN agent_events body ON body.event_offset=bounded.event_offset ORDER BY message_position");
        let rows: Vec<(i64, Option<String>, i64)> = sqlx::query_as(&query)
            .bind(&identity.session_id)
            .bind(&identity.turn_id)
            .bind(&identity.session_id)
            .bind(&identity.session_id)
            .bind(&identity.turn_id)
            .bind(&identity.session_id)
            .bind(first)
            .bind(needs_user)
            .bind(first)
            .bind(super::encode(&call_ids)?)
            .bind(agent_protocol::MAX_EXECUTION_EVENT_BYTES as i64)
            .bind(byte_budget.saturating_sub(page.bytes) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        if rows.len() > 128 {
            return Err(history_limit());
        }
        let mut companions = Vec::new();
        for (_, body, bytes) in rows {
            let mut message: AgentMessage = decode(&body.ok_or_else(history_limit)?)?;
            if after.is_some() && !matches!(message.role, talos_agent_api::AgentMessageRole::User) {
                // Retain the matched invocation without repeating consumed text.
                message.blocks.retain(|block| matches!(&block.content, AgentMessageBlockContent::ToolCall(call) if call_ids.contains(&call.call_id.as_str())));
            }
            page.bytes += bytes as usize;
            companions.push(message);
        }
        companions.append(&mut page.messages);
        page.messages = companions;
        Ok(())
    }

    /// Inspect an exact Turn's successful Tool outcomes without transferring any
    /// message body. Callers retain their own Tool-name policy.
    pub async fn has_successful_tool_result(
        &self,
        identity: &AgentTurnRef,
        tool_names: &[&str],
    ) -> TalosResult<bool> {
        let exists: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_events e,json_each(e.event_json,'$.payload.data.blocks') block WHERE e.session_id=? AND e.turn_id=? AND e.generation=(SELECT generation FROM sessions WHERE session_id=?) AND json_extract(e.event_json,'$.payload.type')='messageCompleted' AND json_extract(block.value,'$.content.type')='toolResult' AND json_extract(block.value,'$.content.data.isError')=0 AND json_extract(block.value,'$.content.data.toolName') IN (SELECT value FROM json_each(?)))")
            .bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).bind(super::encode(&tool_names)?).fetch_one(&self.pool).await.map_err(database_error)?;
        Ok(exists != 0)
    }

    /// Metrics are bounded independently from the potentially large message list.
    pub async fn history_metrics(&self, identity: &AgentTurnRef) -> TalosResult<AgentRunMetrics> {
        let row: Option<(Option<String>,)> = sqlx::query_as("SELECT CASE WHEN length(CAST(json_extract(checkpoint_json,'$.metrics') AS BLOB))<=? THEN json_extract(checkpoint_json,'$.metrics') END FROM turn_checkpoints WHERE session_id=? AND turn_id=? AND generation=(SELECT generation FROM sessions WHERE session_id=?)")
            .bind(agent_protocol::MAX_EXECUTION_EVENT_BYTES as i64).bind(&identity.session_id)
            .bind(&identity.turn_id).bind(&identity.session_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        let (body,) =
            row.ok_or_else(|| TalosError::internal("Worker terminal checkpoint is missing."))?;
        decode(&body.ok_or_else(history_limit)?)
    }
}

fn history_limit() -> TalosError {
    TalosError::new(
        ErrorCode::ContextWindowExceeded,
        "One Worker history item exceeds the local context read budget.",
    )
}
