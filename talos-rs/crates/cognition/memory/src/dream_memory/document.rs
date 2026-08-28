use chrono::{SecondsFormat, TimeZone, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use talos_knowledge_format::{
    KnowledgeDocument, KnowledgeEntryQuery, OkfActor, OkfDateTime, OkfDocument, OkfGenerated,
};

use super::entry_codec::{from_knowledge_entry, to_knowledge_entry};
use super::profile::dream_memory_profile;
use super::{
    DreamMemoryAppendResult, DreamMemoryDocumentContext, DreamMemoryEntry, DreamMemoryError,
    DreamMemoryErrorKind, DreamMemoryProvenance, DreamMemoryResult, DreamMemorySourceRange,
    MemoryId, MemorySessionId,
};
use crate::MemoryWriteBatch;

const MAX_BATCH_CANDIDATES: usize = 8;
const MAX_CANDIDATE_BODY_CHARS: usize = 1_000;

/// Typed Dream Memory document backed by the shared Knowledge Format engine.
#[derive(Debug, Clone, PartialEq)]
pub struct DreamMemoryDocument {
    context: DreamMemoryDocumentContext,
    generated: OkfGenerated,
    document: KnowledgeDocument,
    entries: Vec<DreamMemoryEntry>,
}

impl DreamMemoryDocument {
    /// Creates an empty canonical document for one date partition.
    pub fn new(context: DreamMemoryDocumentContext, generated_at: i64) -> DreamMemoryResult<Self> {
        let generated = generated(&context, generated_at)?;
        let profile = dream_memory_profile(context.date(), generated.clone())?;
        let document = KnowledgeDocument::builder(&profile)
            .create_document()
            .build()?;
        Ok(Self {
            context,
            generated,
            document,
            entries: Vec::new(),
        })
    }

    /// Parses and fully validates one canonical Dream Memory file.
    pub fn parse(context: DreamMemoryDocumentContext, markdown: &str) -> DreamMemoryResult<Self> {
        let parsed = OkfDocument::parse(markdown)?;
        let generated = parsed.generated().ok_or_else(|| {
            DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidDocument,
                "Dream Memory generated metadata is required",
            )
        })?;
        validate_generated(&context, &generated)?;
        let profile = dream_memory_profile(context.date(), generated.clone())?;
        let document = KnowledgeDocument::builder(&profile)
            .from_markdown(markdown)
            .build()?;
        let entries = document
            .list_entries(KnowledgeEntryQuery::all())
            .into_iter()
            .map(|entry| from_knowledge_entry(&context, entry))
            .collect::<DreamMemoryResult<Vec<_>>>()?;
        validate_generated_ranges(&entries)?;
        Ok(Self {
            context,
            generated,
            document,
            entries,
        })
    }

    /// Appends one idempotent generated batch.
    pub fn append_batch(
        &mut self,
        batch: &MemoryWriteBatch,
    ) -> DreamMemoryResult<DreamMemoryAppendResult> {
        validate_batch(batch)?;
        let session_id = MemorySessionId::parse(batch.session_id.clone())?;
        let source_range =
            DreamMemorySourceRange::new(batch.source_from_seq, batch.source_until_seq)?;
        let already_applied = self.entries.iter().any(|entry| {
            matches!(
                entry.provenance(),
                DreamMemoryProvenance::Generated {
                    session_id: existing_session,
                    source_range: existing_range,
                } if existing_session == &session_id && *existing_range == source_range
            )
        });
        if already_applied {
            return Ok(DreamMemoryAppendResult::AlreadyApplied);
        }
        if batch.candidates.is_empty() {
            return Ok(DreamMemoryAppendResult::AlreadyApplied);
        }
        if self.entries.iter().any(|entry| {
            matches!(
                entry.provenance(),
                DreamMemoryProvenance::Generated {
                    session_id: existing_session,
                    source_range: existing_range,
                } if existing_session == &session_id && existing_range.overlaps(source_range)
            )
        }) {
            return Err(DreamMemoryError::new(
                DreamMemoryErrorKind::Conflict,
                "Dream Memory batch overlaps an existing session range",
            ));
        }

        let expected = expected_entries(&self.context, batch, session_id, source_range)?;
        for (index, entry) in expected.iter().enumerate() {
            if self.get_entry(entry.memory_id()).is_some()
                || expected[..index]
                    .iter()
                    .any(|existing| existing.memory_id() == entry.memory_id())
            {
                return Err(DreamMemoryError::new(
                    DreamMemoryErrorKind::Conflict,
                    format!("duplicate Dream Memory id: {}", entry.memory_id().as_str()),
                ));
            }
        }
        let mut updated = self.clone();
        for entry in expected {
            updated.insert_entry(entry)?;
        }
        *self = updated;
        Ok(DreamMemoryAppendResult::Appended)
    }

    fn get_entry(&self, memory_id: &MemoryId) -> Option<&DreamMemoryEntry> {
        self.entries
            .iter()
            .find(|entry| entry.memory_id() == memory_id)
    }

    /// Creates one validated entry and rejects an existing stable id.
    pub fn create_entry(&mut self, entry: DreamMemoryEntry) -> DreamMemoryResult<()> {
        if self.get_entry(entry.memory_id()).is_some() {
            return Err(DreamMemoryError::new(
                DreamMemoryErrorKind::AlreadyExists,
                format!(
                    "Dream Memory entry already exists: {}",
                    entry.memory_id().as_str()
                ),
            ));
        }
        self.insert_entry(entry)
    }

    /// Returns all typed entries in canonical file order.
    #[must_use]
    pub fn entries(&self) -> &[DreamMemoryEntry] {
        &self.entries
    }

    /// Renders canonical OKF Markdown.
    pub fn render(&self) -> DreamMemoryResult<String> {
        self.document.render().map_err(Into::into)
    }

    fn insert_entry(&mut self, entry: DreamMemoryEntry) -> DreamMemoryResult<()> {
        entry.validate_for(&self.context)?;
        let mut entries = self.entries.clone();
        entries.push(entry.clone());
        validate_generated_ranges(&entries)?;
        let profile = dream_memory_profile(self.context.date(), self.generated.clone())?;
        let current = self.document.render()?;
        self.document = KnowledgeDocument::builder(&profile)
            .from_markdown(current)
            .create_entry(to_knowledge_entry(&entry)?)
            .build()?;
        self.entries.push(entry);
        Ok(())
    }
}

fn generated(
    context: &DreamMemoryDocumentContext,
    generated_at: i64,
) -> DreamMemoryResult<OkfGenerated> {
    let timestamp = Utc.timestamp_opt(generated_at, 0).single().ok_or_else(|| {
        DreamMemoryError::new(
            DreamMemoryErrorKind::InvalidInput,
            "Dream Memory generated_at is invalid",
        )
    })?;
    let value = timestamp.to_rfc3339_opts(SecondsFormat::Secs, true);
    let at = OkfDateTime::parse(&value).ok_or_else(|| {
        DreamMemoryError::new(
            DreamMemoryErrorKind::InvalidInput,
            "Dream Memory generated_at cannot be represented as OKF",
        )
    })?;
    Ok(OkfGenerated::new(
        OkfActor::parse(producer_actor(context)),
        at,
    ))
}

fn validate_generated(
    context: &DreamMemoryDocumentContext,
    generated: &OkfGenerated,
) -> DreamMemoryResult<()> {
    let complete = generated
        .by
        .as_ref()
        .is_some_and(|actor| actor.as_str() == producer_actor(context))
        && generated
            .at
            .as_ref()
            .and_then(talos_knowledge_format::OkfDateTimeValue::parsed)
            .is_some();
    if !complete {
        return Err(DreamMemoryError::new(
            DreamMemoryErrorKind::InvalidDocument,
            "Dream Memory generated metadata must identify its owning Agent and timestamp",
        ));
    }
    Ok(())
}

fn producer_actor(context: &DreamMemoryDocumentContext) -> String {
    format!("talos-agent/{}", context.agent_id().as_str())
}

#[derive(Serialize)]
struct MemoryIdKey<'a> {
    schema_version: u8,
    user_id: &'a str,
    session_id: &'a str,
    source_from_seq: u64,
    source_until_seq: u64,
    candidate_ordinal: usize,
}

fn expected_entries(
    context: &DreamMemoryDocumentContext,
    batch: &MemoryWriteBatch,
    session_id: MemorySessionId,
    source_range: DreamMemorySourceRange,
) -> DreamMemoryResult<Vec<DreamMemoryEntry>> {
    batch
        .candidates
        .iter()
        .enumerate()
        .map(|(ordinal, candidate)| {
            let key = MemoryIdKey {
                schema_version: 1,
                user_id: context.user_id().as_str(),
                session_id: session_id.as_str(),
                source_from_seq: source_range.from_seq(),
                source_until_seq: source_range.until_seq(),
                candidate_ordinal: ordinal,
            };
            let bytes = serde_json::to_vec(&key).map_err(|error| {
                DreamMemoryError::new(
                    DreamMemoryErrorKind::InvalidInput,
                    format!("serialize Dream Memory id: {error}"),
                )
            })?;
            let digest = Sha256::digest(bytes);
            let hex = digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let memory_id = MemoryId::generated(format!("mem_{}", &hex[..32]));
            let entry = DreamMemoryEntry::builder()
                .memory_id(memory_id)
                .title(normalize_text(&candidate.title))
                .body(normalize_text(&candidate.body))
                .occurred_at(batch.occurred_at)
                .kind(candidate.kind)
                .importance(candidate.importance)
                .provenance(DreamMemoryProvenance::Generated {
                    session_id: session_id.clone(),
                    source_range,
                })
                .build()?;
            entry.validate_for(context)?;
            Ok(entry)
        })
        .collect()
}

fn validate_batch(batch: &MemoryWriteBatch) -> DreamMemoryResult<()> {
    MemorySessionId::parse(batch.session_id.clone())?;
    DreamMemorySourceRange::new(batch.source_from_seq, batch.source_until_seq)?;
    if batch.candidates.len() > MAX_BATCH_CANDIDATES
        || batch.candidates.iter().any(|candidate| {
            let title = normalize_text(&candidate.title);
            let body = normalize_text(&candidate.body);
            title.is_empty()
                || body.is_empty()
                || title.chars().count() > 160
                || body.chars().count() > MAX_CANDIDATE_BODY_CHARS
        })
    {
        return Err(DreamMemoryError::new(
            DreamMemoryErrorKind::InvalidInput,
            "invalid Dream Memory write batch",
        ));
    }
    Ok(())
}

fn validate_generated_ranges(entries: &[DreamMemoryEntry]) -> DreamMemoryResult<()> {
    for (index, entry) in entries.iter().enumerate() {
        let DreamMemoryProvenance::Generated {
            session_id,
            source_range,
        } = entry.provenance()
        else {
            continue;
        };
        for other in &entries[index + 1..] {
            let DreamMemoryProvenance::Generated {
                session_id: other_session_id,
                source_range: other_source_range,
            } = other.provenance()
            else {
                continue;
            };
            if session_id == other_session_id
                && source_range != other_source_range
                && source_range.overlaps(*other_source_range)
            {
                return Err(DreamMemoryError::new(
                    DreamMemoryErrorKind::Conflict,
                    "Dream Memory entries contain overlapping session ranges",
                ));
            }
        }
    }
    Ok(())
}

fn normalize_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}
