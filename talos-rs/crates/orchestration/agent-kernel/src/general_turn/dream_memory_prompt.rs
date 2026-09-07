use chrono::{TimeZone, Utc};
use serde::Serialize;
use talos_memory::{DreamMemoryDaySnapshot, DreamMemoryEntry};

const SAME_DAY_CONTEXT_MAX_CHARS: usize = 6_000;
const SAME_DAY_ENTRY_BODY_MAX_CHARS: usize = 600;

pub(super) const DREAM_MEMORY_FORMATION_PROMPT: &str = r#"Form Talos Dream Memory and return only valid JSON with no markdown fences or explanation.

The user message declares exactly one mode. Return the matching tagged shape:
- compaction: {"mode":"compaction","title":"...","facts":[],"preferences":[],"open_tasks":[],"risks":[],"memory_candidates":[]}
- maintenance: {"mode":"maintenance","memory_candidates":[]}

Each memory candidate is {"title":"...","body":"...","kind":"preference|fact|decision|task|episode","importance":"low|normal|high"}. Return at most 8 candidates and use the transcript language.

Save only information likely to change behavior in a future session and not reliably recoverable from an authoritative source:
- preference: confirmed user collaboration or work preference;
- fact: durable user, business, or external fact;
- decision: confirmed conclusion with rationale and applicability;
- task: cross-session commitment with a meaningful owner or deadline;
- episode: incident or lesson likely to prevent a repeated mistake.

Treat explicit requests to remember, user corrections, and repeated confirmations as stronger evidence, while still applying every exclusion below.

Do not save conversation summaries, activity logs, code structure, file paths, git history, debugging recipes, current progress, temporary plans, speculation, secrets, or information already available in repository files. User Workspace Memory represents current normative conclusions; Dream Memory represents dated historical evidence. Do not ask to update User Workspace Memory from this task.

Consolidate repeated signals within the transcript. Compare candidates only with the supplied existing memories for the same date: omit an equivalent conclusion, emit only genuinely new detail, and preserve a contradiction as new dated evidence instead of silently replacing history. Never compare, move, remove, or rewrite memories from another date. Prefer an empty memory_candidates array when uncertain.

Importance means: high changes important future behavior or preserves a critical commitment; normal has durable future value; low is useful but non-critical. For compaction, also produce a concise session continuity summary under 200 words. Omit optional summary arrays when empty."#;

#[derive(Serialize)]
struct SameDayPromptEntry<'a> {
    memory_id: &'a str,
    title: &'a str,
    body: String,
    kind: talos_types::MemoryKind,
    importance: talos_types::MemoryImportance,
    occurred_at: i64,
}

pub(super) enum DreamMemoryFormationMode {
    Compaction,
    Maintenance,
}

impl DreamMemoryFormationMode {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Compaction => "compaction",
            Self::Maintenance => "maintenance",
        }
    }
}

pub(super) fn formation_user_prompt(
    mode: DreamMemoryFormationMode,
    transcript: &str,
    snapshot: &DreamMemoryDaySnapshot,
) -> talos_error::TalosResult<String> {
    let existing = render_same_day_entries(snapshot.entries())?;
    Ok(format!(
        "Mode: {}\nDream Memory date: {}\nExisting memories for this date:\n{}\n\nConversation transcript:\n{}",
        mode.as_str(),
        snapshot.date(),
        existing,
        transcript
    ))
}

pub(super) fn date_for_timestamp(
    timestamp: i64,
    timezone: &str,
) -> talos_error::TalosResult<talos_memory::DreamMemoryDate> {
    let timezone = talos_config::parse_timezone(timezone)?;
    let timestamp = Utc.timestamp_opt(timestamp, 0).single().ok_or_else(|| {
        talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "invalid Dream Memory occurred_at".to_string(),
        )
    })?;
    talos_memory::DreamMemoryDate::parse(
        &timestamp
            .with_timezone(&timezone)
            .format("%Y-%m-%d")
            .to_string(),
    )
    .map_err(|error| {
        talos_error::TalosError::new(talos_error::ErrorCode::InvalidRequest, error.to_string())
    })
}

fn render_same_day_entries(entries: &[DreamMemoryEntry]) -> talos_error::TalosResult<String> {
    let mut rendered = String::from("[");
    for entry in entries {
        let prompt_entry = SameDayPromptEntry {
            memory_id: entry.memory_id().as_str(),
            title: entry.title(),
            body: entry
                .body()
                .chars()
                .take(SAME_DAY_ENTRY_BODY_MAX_CHARS)
                .collect(),
            kind: entry.kind(),
            importance: entry.importance(),
            occurred_at: entry.occurred_at(),
        };
        let item = serde_json::to_string(&prompt_entry).map_err(|error| {
            talos_error::TalosError::internal_with_source(
                "serialize Dream Memory prompt entry; technical details were withheld.",
                error,
            )
        })?;
        let separator = if rendered.len() == 1 { "" } else { "," };
        if rendered.chars().count() + separator.len() + item.chars().count() + 1
            > SAME_DAY_CONTEXT_MAX_CHARS
        {
            break;
        }
        rendered.push_str(separator);
        rendered.push_str(&item);
    }
    rendered.push(']');
    Ok(rendered)
}
