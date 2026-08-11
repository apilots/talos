use chrono::{NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use serde::Serialize;
use sha2::{Digest, Sha256};
use talos_error::{TalosError, TalosResult};

use super::types::{
    parse_memory_importance, parse_memory_kind, MemoryImportance, MemoryKind, MemoryRecord,
    ParsedMemoryRecord, ParsedMemorySource,
};

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

fn parse_heading(line: &str) -> Option<Heading<'_>> {
    let value = line.trim();
    let heading = value.strip_prefix("## ")?.trim();
    if heading.is_empty() {
        return None;
    }
    let timed_heading = heading
        .strip_prefix('[')
        .and_then(|suffix| suffix.split_once(']'))
        .and_then(|(time, title)| {
            NaiveTime::parse_from_str(time, "%H:%M")
                .ok()
                .map(|time| (time, title.trim()))
        })
        .filter(|(_, title)| !title.is_empty());
    if let Some((time, title)) = timed_heading {
        return Some(Heading {
            time: Some(time),
            title: title.trim(),
        });
    }
    Some(Heading {
        time: None,
        title: heading,
    })
}

fn metadata_value<'a>(lines: &'a [&str], key: &str) -> Option<&'a str> {
    lines.iter().find_map(|line| {
        line.trim()
            .strip_prefix(key)
            .and_then(|value| value.strip_prefix(':'))
            .map(str::trim)
    })
}

fn is_metadata_line(line: &str, v1: bool) -> bool {
    let key = line.trim().split_once(':').map(|(key, _)| key);
    match key {
        Some("session") => true,
        Some(
            "memory_id" | "occurred_at" | "kind" | "importance" | "source_from_seq"
            | "source_until_seq",
        ) if v1 => true,
        _ => false,
    }
}

pub(super) fn normalize_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn legacy_memory_id(
    user_id: &str,
    source_date: &str,
    session_id: Option<&str>,
    ordinal: usize,
    title: &str,
    body: &str,
) -> TalosResult<String> {
    let key = LegacyMemoryIdKey {
        schema_version: 1,
        user_id,
        source_date,
        session_id,
        ordinal,
        title,
        body,
    };
    let bytes = serde_json::to_vec(&key)
        .map_err(|error| TalosError::internal(format!("serialize legacy memory id: {error}")))?;
    Ok(format!("mem_{}", &sha256_hex(&bytes)[..32]))
}

fn legacy_occurred_at(date: NaiveDate, time: Option<NaiveTime>, timezone: Tz) -> TalosResult<i64> {
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

fn parse_required<'a>(
    lines: &'a [&str],
    key: &str,
    source_path: &str,
    ordinal: usize,
) -> TalosResult<&'a str> {
    metadata_value(lines, key)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "memory block {} in {source_path} is missing {key}",
                    ordinal + 1
                ),
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn parse_block(
    user_id: &str,
    source_path: &str,
    source_date: &str,
    date: NaiveDate,
    timezone: Tz,
    ordinal: usize,
    lines: &[&str],
) -> TalosResult<ParsedMemoryRecord> {
    let heading = parse_heading(lines[0]).ok_or_else(|| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "invalid memory heading in {source_path} block {}",
                ordinal + 1
            ),
        )
    })?;
    let v1 = [
        "memory_id",
        "occurred_at",
        "kind",
        "importance",
        "source_from_seq",
        "source_until_seq",
    ]
    .iter()
    .any(|key| metadata_value(lines, key).is_some());
    let title = normalize_text(heading.title);
    let body = normalize_text(
        &lines[1..]
            .iter()
            .filter(|line| !is_metadata_line(line, v1))
            .copied()
            .collect::<Vec<_>>()
            .join("\n"),
    );
    if body.is_empty()
        || title.chars().count() > MAX_TITLE_CHARS
        || body.chars().count() > MAX_BODY_CHARS
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "memory block {} in {source_path} exceeds content limits",
                ordinal + 1
            ),
        ));
    }
    let session_id = metadata_value(lines, "session")
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let record = if v1 {
        let memory_id = parse_required(lines, "memory_id", source_path, ordinal)?.to_string();
        if memory_id.len() != 36
            || !memory_id.starts_with("mem_")
            || !memory_id[4..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "memory block {} in {source_path} has invalid memory_id",
                    ordinal + 1
                ),
            ));
        }
        let source_from_seq = parse_required(lines, "source_from_seq", source_path, ordinal)?
            .parse()
            .map_err(|_| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!(
                        "memory block {} in {source_path} has invalid source_from_seq",
                        ordinal + 1
                    ),
                )
            })?;
        let source_until_seq = parse_required(lines, "source_until_seq", source_path, ordinal)?
            .parse()
            .map_err(|_| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!(
                        "memory block {} in {source_path} has invalid source_until_seq",
                        ordinal + 1
                    ),
                )
            })?;
        if source_from_seq > source_until_seq {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "memory block {} in {source_path} has invalid source range",
                    ordinal + 1
                ),
            ));
        }
        if source_from_seq > i64::MAX as u64 || source_until_seq > i64::MAX as u64 {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                "memory block {} in {source_path} has source range outside SQLite integer bounds",
                ordinal + 1
            ),
            ));
        }
        MemoryRecord {
            memory_id,
            title,
            body,
            kind: parse_memory_kind(parse_required(lines, "kind", source_path, ordinal)?)
                .ok_or_else(|| {
                    TalosError::public(
                        talos_error::ErrorCode::InvalidRequest,
                        format!(
                            "memory block {} in {source_path} has invalid kind",
                            ordinal + 1
                        ),
                    )
                })?,
            importance: parse_memory_importance(parse_required(
                lines,
                "importance",
                source_path,
                ordinal,
            )?)
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!(
                        "memory block {} in {source_path} has invalid importance",
                        ordinal + 1
                    ),
                )
            })?,
            occurred_at: parse_required(lines, "occurred_at", source_path, ordinal)?
                .parse()
                .map_err(|_| {
                    TalosError::public(
                        talos_error::ErrorCode::InvalidRequest,
                        format!(
                            "memory block {} in {source_path} has invalid occurred_at",
                            ordinal + 1
                        ),
                    )
                })?,
            source_date: source_date.to_string(),
            session_id: Some(parse_required(lines, "session", source_path, ordinal)?.to_string()),
            source_from_seq: Some(source_from_seq),
            source_until_seq: Some(source_until_seq),
        }
    } else {
        MemoryRecord {
            memory_id: legacy_memory_id(
                user_id,
                source_date,
                session_id.as_deref(),
                ordinal,
                &title,
                &body,
            )?,
            title,
            body,
            kind: MemoryKind::Episode,
            importance: MemoryImportance::Normal,
            occurred_at: legacy_occurred_at(date, heading.time, timezone)?,
            source_date: source_date.to_string(),
            session_id,
            source_from_seq: None,
            source_until_seq: None,
        }
    };
    Ok(ParsedMemoryRecord {
        record,
        source_path: source_path.to_string(),
    })
}

pub(super) fn parse_source(
    user_id: &str,
    source_path: &str,
    content: &str,
    timezone: Tz,
) -> TalosResult<ParsedMemorySource> {
    let source_date = std::path::Path::new(source_path)
        .file_name()
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_suffix(".md"))
        .ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "invalid memory source path".to_string(),
            )
        })?;
    let date = NaiveDate::parse_from_str(source_date, "%Y-%m-%d").map_err(|_| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid memory source date: {source_path}"),
        )
    })?;
    let lines = content.lines().collect::<Vec<_>>();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| parse_heading(line).is_some().then_some(index))
        .collect::<Vec<_>>();
    let mut records = Vec::with_capacity(starts.len());
    for (ordinal, start) in starts.iter().copied().enumerate() {
        let end = starts.get(ordinal + 1).copied().unwrap_or(lines.len());
        records.push(parse_block(
            user_id,
            source_path,
            source_date,
            date,
            timezone,
            ordinal,
            &lines[start..end],
        )?);
    }
    Ok(ParsedMemorySource {
        source_path: source_path.to_string(),
        fingerprint: sha256_hex(content.as_bytes()),
        records,
    })
}
