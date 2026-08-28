//! Legacy Dream Memory parser used only by the one-time OKF converter.
//!
//! TODO(dream-memory-okf): Remove this one-time legacy converter in the next release.

use chrono::{NaiveDate, NaiveTime, TimeZone};
use serde::Serialize;
use sha2::{Digest, Sha256};
use talos_config::TalosTimezone;
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    DreamMemoryEntry, DreamMemoryProvenance, DreamMemorySourceRange, MemoryId, MemorySessionId,
};
use talos_types::{MemoryImportance, MemoryKind};

const MAX_TITLE_CHARS: usize = 160;
const MAX_BODY_CHARS: usize = 10_000;

#[derive(Serialize)]
struct LegacyMemoryIdKey<'a> {
    schema_version: u8,
    user_id: &'a str,
    source_date: &'a str,
    session_id: Option<&'a str>,
    ordinal: usize,
    title: &'a str,
    body: &'a str,
}

struct Heading<'a> {
    time: Option<NaiveTime>,
    title: &'a str,
}

#[derive(Clone, Copy)]
enum LegacyBlockKind {
    Plain,
    Versioned,
}

#[derive(Clone, Copy)]
enum LegacyMetadataKey {
    MemoryId,
    OccurredAt,
    Session,
    Kind,
    Importance,
    SourceFromSeq,
    SourceUntilSeq,
}

impl LegacyMetadataKey {
    const fn as_str(self) -> &'static str {
        match self {
            Self::MemoryId => "memory_id",
            Self::OccurredAt => "occurred_at",
            Self::Session => "session",
            Self::Kind => "kind",
            Self::Importance => "importance",
            Self::SourceFromSeq => "source_from_seq",
            Self::SourceUntilSeq => "source_until_seq",
        }
    }
}

const VERSIONED_METADATA_KEYS: [LegacyMetadataKey; 6] = [
    LegacyMetadataKey::MemoryId,
    LegacyMetadataKey::OccurredAt,
    LegacyMetadataKey::Kind,
    LegacyMetadataKey::Importance,
    LegacyMetadataKey::SourceFromSeq,
    LegacyMetadataKey::SourceUntilSeq,
];

const ALL_METADATA_KEYS: [LegacyMetadataKey; 7] = [
    LegacyMetadataKey::MemoryId,
    LegacyMetadataKey::OccurredAt,
    LegacyMetadataKey::Session,
    LegacyMetadataKey::Kind,
    LegacyMetadataKey::Importance,
    LegacyMetadataKey::SourceFromSeq,
    LegacyMetadataKey::SourceUntilSeq,
];

pub(super) fn parse_legacy_entries(
    user_id: &str,
    source_date: &str,
    content: &str,
    timezone: TalosTimezone,
) -> TalosResult<Vec<DreamMemoryEntry>> {
    let date = NaiveDate::parse_from_str(source_date, "%Y-%m-%d").map_err(|_| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid legacy Dream Memory date: {source_date}"),
        )
    })?;
    let lines = content.lines().collect::<Vec<_>>();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| parse_heading(line).is_some().then_some(index))
        .collect::<Vec<_>>();
    let mut entries = Vec::with_capacity(starts.len());
    for (ordinal, start) in starts.iter().copied().enumerate() {
        let end = starts.get(ordinal + 1).copied().unwrap_or(lines.len());
        entries.push(parse_block(
            user_id,
            source_date,
            date,
            timezone,
            ordinal,
            &lines[start..end],
        )?);
    }
    Ok(entries)
}

pub(super) fn legacy_partition_start(
    source_date: &str,
    timezone: TalosTimezone,
) -> TalosResult<i64> {
    let date = NaiveDate::parse_from_str(source_date, "%Y-%m-%d").map_err(|_| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid legacy Dream Memory date: {source_date}"),
        )
    })?;
    legacy_occurred_at(date, None, timezone)
}

fn parse_heading(line: &str) -> Option<Heading<'_>> {
    let heading = line.trim().strip_prefix("## ")?.trim();
    if heading.is_empty() {
        return None;
    }
    if let Some((time, title)) = heading
        .strip_prefix('[')
        .and_then(|suffix| suffix.split_once(']'))
        .and_then(|(time, title)| {
            NaiveTime::parse_from_str(time, "%H:%M")
                .ok()
                .map(|time| (time, title.trim()))
        })
        .filter(|(_, title)| !title.is_empty())
    {
        return Some(Heading {
            time: Some(time),
            title,
        });
    }
    Some(Heading {
        time: None,
        title: heading,
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_block(
    user_id: &str,
    source_date: &str,
    date: NaiveDate,
    timezone: TalosTimezone,
    ordinal: usize,
    lines: &[&str],
) -> TalosResult<DreamMemoryEntry> {
    let heading = parse_heading(lines[0]).ok_or_else(|| invalid_block(ordinal, "heading"))?;
    let block_kind = if VERSIONED_METADATA_KEYS
        .iter()
        .any(|key| metadata_value(lines, *key).is_some())
    {
        LegacyBlockKind::Versioned
    } else {
        LegacyBlockKind::Plain
    };
    for key in ALL_METADATA_KEYS {
        if metadata_value_count(lines, key) > 1 {
            return Err(invalid_block(ordinal, key.as_str()));
        }
    }
    let title = normalize_text(heading.title);
    let body = normalize_text(
        &lines[1..]
            .iter()
            .filter(|line| !is_metadata_line(line, block_kind))
            .copied()
            .collect::<Vec<_>>()
            .join("\n"),
    );
    if title.is_empty()
        || body.is_empty()
        || title.chars().count() > MAX_TITLE_CHARS
        || body.chars().count() > MAX_BODY_CHARS
    {
        return Err(invalid_block(ordinal, "content limits"));
    }
    let session =
        metadata_value(lines, LegacyMetadataKey::Session).filter(|value| !value.is_empty());
    let (memory_id, occurred_at, kind, importance, provenance) =
        if matches!(block_kind, LegacyBlockKind::Versioned) {
            let source_from_seq = required(lines, LegacyMetadataKey::SourceFromSeq, ordinal)?
                .parse::<u64>()
                .map_err(|_| invalid_block(ordinal, "source_from_seq"))?;
            let source_until_seq = required(lines, LegacyMetadataKey::SourceUntilSeq, ordinal)?
                .parse::<u64>()
                .map_err(|_| invalid_block(ordinal, "source_until_seq"))?;
            (
                MemoryId::parse(required(lines, LegacyMetadataKey::MemoryId, ordinal)?.to_string())
                    .map_err(map_domain_error)?,
                required(lines, LegacyMetadataKey::OccurredAt, ordinal)?
                    .parse::<i64>()
                    .map_err(|_| invalid_block(ordinal, "occurred_at"))?,
                parse_kind(required(lines, LegacyMetadataKey::Kind, ordinal)?)
                    .ok_or_else(|| invalid_block(ordinal, "kind"))?,
                parse_importance(required(lines, LegacyMetadataKey::Importance, ordinal)?)
                    .ok_or_else(|| invalid_block(ordinal, "importance"))?,
                DreamMemoryProvenance::Generated {
                    session_id: MemorySessionId::parse(
                        required(lines, LegacyMetadataKey::Session, ordinal)?.to_string(),
                    )
                    .map_err(map_domain_error)?,
                    source_range: DreamMemorySourceRange::new(source_from_seq, source_until_seq)
                        .map_err(map_domain_error)?,
                },
            )
        } else {
            (
                legacy_memory_id(user_id, source_date, session, ordinal, &title, &body)?,
                legacy_occurred_at(date, heading.time, timezone)?,
                MemoryKind::Episode,
                MemoryImportance::Normal,
                DreamMemoryProvenance::UserAuthored,
            )
        };
    DreamMemoryEntry::builder()
        .memory_id(memory_id)
        .title(title)
        .body(body)
        .occurred_at(occurred_at)
        .kind(kind)
        .importance(importance)
        .provenance(provenance)
        .build()
        .map_err(map_domain_error)
}

fn metadata_value<'a>(lines: &'a [&str], key: LegacyMetadataKey) -> Option<&'a str> {
    lines.iter().find_map(|line| line_metadata_value(line, key))
}

fn metadata_value_count(lines: &[&str], key: LegacyMetadataKey) -> usize {
    lines
        .iter()
        .filter(|line| line_metadata_value(line, key).is_some())
        .count()
}

fn line_metadata_value(line: &str, key: LegacyMetadataKey) -> Option<&str> {
    line.trim()
        .strip_prefix(key.as_str())
        .and_then(|value| value.strip_prefix(':'))
        .map(str::trim)
}

fn required<'a>(lines: &'a [&str], key: LegacyMetadataKey, ordinal: usize) -> TalosResult<&'a str> {
    metadata_value(lines, key)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_block(ordinal, key.as_str()))
}

fn is_metadata_line(line: &str, block_kind: LegacyBlockKind) -> bool {
    let key = line.trim().split_once(':').map(|(key, _)| key);
    match key {
        Some("session") => true,
        Some(
            "memory_id" | "occurred_at" | "kind" | "importance" | "source_from_seq"
            | "source_until_seq",
        ) if matches!(block_kind, LegacyBlockKind::Versioned) => true,
        _ => false,
    }
}

fn legacy_memory_id(
    user_id: &str,
    source_date: &str,
    session_id: Option<&str>,
    ordinal: usize,
    title: &str,
    body: &str,
) -> TalosResult<MemoryId> {
    let bytes = serde_json::to_vec(&LegacyMemoryIdKey {
        schema_version: 1,
        user_id,
        source_date,
        session_id,
        ordinal,
        title,
        body,
    })
    .map_err(|error| TalosError::internal(format!("serialize legacy memory id: {error}")))?;
    let digest = Sha256::digest(bytes);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    MemoryId::parse(format!("mem_{}", &hex[..32])).map_err(map_domain_error)
}

fn legacy_occurred_at(
    date: NaiveDate,
    time: Option<NaiveTime>,
    timezone: TalosTimezone,
) -> TalosResult<i64> {
    timezone
        .from_local_datetime(&date.and_time(time.unwrap_or(NaiveTime::MIN)))
        .earliest()
        .map(|value| value.timestamp())
        .ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "invalid legacy memory timestamp".to_string(),
            )
        })
}

fn parse_kind(value: &str) -> Option<MemoryKind> {
    match value {
        "preference" => Some(MemoryKind::Preference),
        "fact" => Some(MemoryKind::Fact),
        "decision" => Some(MemoryKind::Decision),
        "task" => Some(MemoryKind::Task),
        "episode" => Some(MemoryKind::Episode),
        _ => None,
    }
}

fn parse_importance(value: &str) -> Option<MemoryImportance> {
    match value {
        "low" => Some(MemoryImportance::Low),
        "medium" | "normal" => Some(MemoryImportance::Normal),
        "high" => Some(MemoryImportance::High),
        _ => None,
    }
}

fn normalize_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

fn invalid_block(ordinal: usize, field: &str) -> TalosError {
    TalosError::public(
        talos_error::ErrorCode::InvalidRequest,
        format!("legacy memory block {} has invalid {field}", ordinal + 1),
    )
}

fn map_domain_error(error: talos_memory::DreamMemoryError) -> TalosError {
    TalosError::public(talos_error::ErrorCode::InvalidRequest, error.to_string())
}
