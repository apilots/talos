use std::fmt;

use chrono::{NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use talos_types::{MemoryImportance, MemoryKind};

use super::{DreamMemoryError, DreamMemoryErrorKind, DreamMemoryResult};

const MAX_ID_CHARS: usize = 256;
const MAX_TITLE_CHARS: usize = 160;
const MAX_BODY_CHARS: usize = 10_000;

macro_rules! validated_id {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> DreamMemoryResult<Self> {
                let value = value.into();
                if value.trim() != value
                    || value.is_empty()
                    || value.chars().count() > MAX_ID_CHARS
                    || value.chars().any(char::is_control)
                {
                    return Err(DreamMemoryError::new(
                        DreamMemoryErrorKind::InvalidInput,
                        concat!($label, " is invalid"),
                    ));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

validated_id!(MemoryUserId, "memory user id");
validated_id!(MemorySessionId, "memory session id");
validated_id!(MemoryAgentId, "memory agent id");

/// Stable identifier of one Dream Memory entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemoryId(String);

impl MemoryId {
    pub fn parse(value: impl Into<String>) -> DreamMemoryResult<Self> {
        let value = value.into();
        if value.len() != 36
            || !value.starts_with("mem_")
            || !value[4..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidInput,
                "memory id is invalid",
            ));
        }
        Ok(Self(value))
    }

    pub(crate) fn generated(value: String) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated calendar partition for one Dream Memory document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DreamMemoryDate(NaiveDate);

impl DreamMemoryDate {
    pub fn parse(value: &str) -> DreamMemoryResult<Self> {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| {
                DreamMemoryError::new(
                    DreamMemoryErrorKind::InvalidInput,
                    "Dream Memory date must use YYYY-MM-DD",
                )
            })
    }

    pub(crate) fn from_timestamp(
        timestamp: i64,
        timezone: MemoryTimezone,
    ) -> DreamMemoryResult<Self> {
        let occurred_at = Utc.timestamp_opt(timestamp, 0).single().ok_or_else(|| {
            DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidInput,
                "Dream Memory occurred_at is invalid",
            )
        })?;
        Ok(Self(occurred_at.with_timezone(&timezone.0).date_naive()))
    }
}

impl fmt::Display for DreamMemoryDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.format("%Y-%m-%d").fmt(formatter)
    }
}

/// Explicit timezone used to validate Dream Memory date partitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryTimezone(Tz);

impl MemoryTimezone {
    #[must_use]
    pub const fn new(timezone: Tz) -> Self {
        Self(timezone)
    }
}

/// Immutable context that owns one Dream Memory document partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMemoryDocumentContext {
    agent_id: MemoryAgentId,
    user_id: MemoryUserId,
    date: DreamMemoryDate,
    timezone: MemoryTimezone,
}

impl DreamMemoryDocumentContext {
    #[must_use]
    pub const fn new(
        agent_id: MemoryAgentId,
        user_id: MemoryUserId,
        date: DreamMemoryDate,
        timezone: MemoryTimezone,
    ) -> Self {
        Self {
            agent_id,
            user_id,
            date,
            timezone,
        }
    }

    #[must_use]
    pub fn agent_id(&self) -> &MemoryAgentId {
        &self.agent_id
    }

    #[must_use]
    pub fn user_id(&self) -> &MemoryUserId {
        &self.user_id
    }

    #[must_use]
    pub const fn date(&self) -> DreamMemoryDate {
        self.date
    }

    #[must_use]
    pub const fn timezone(&self) -> MemoryTimezone {
        self.timezone
    }
}

/// Inclusive source sequence range for a generated memory batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DreamMemorySourceRange {
    from_seq: u64,
    until_seq: u64,
}

impl DreamMemorySourceRange {
    pub fn new(from_seq: u64, until_seq: u64) -> DreamMemoryResult<Self> {
        if from_seq > until_seq {
            return Err(DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidInput,
                "Dream Memory source range is invalid",
            ));
        }
        Ok(Self {
            from_seq,
            until_seq,
        })
    }

    #[must_use]
    pub const fn from_seq(self) -> u64 {
        self.from_seq
    }

    #[must_use]
    pub const fn until_seq(self) -> u64 {
        self.until_seq
    }

    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.from_seq <= other.until_seq && self.until_seq >= other.from_seq
    }
}

/// Provenance is either complete generated metadata or explicitly user-authored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DreamMemoryProvenance {
    Generated {
        session_id: MemorySessionId,
        source_range: DreamMemorySourceRange,
    },
    UserAuthored,
}

/// One validated business entry in a Dream Memory document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMemoryEntry {
    memory_id: MemoryId,
    title: String,
    body: String,
    occurred_at: i64,
    kind: MemoryKind,
    importance: MemoryImportance,
    provenance: DreamMemoryProvenance,
}

impl DreamMemoryEntry {
    #[must_use]
    pub fn builder() -> DreamMemoryEntryBuilder {
        DreamMemoryEntryBuilder::default()
    }

    pub(crate) fn validate_for(
        &self,
        context: &DreamMemoryDocumentContext,
    ) -> DreamMemoryResult<()> {
        validate_text(
            &self.title,
            "Dream Memory title",
            MAX_TITLE_CHARS,
            DreamMemoryTextShape::SingleLine,
        )?;
        validate_text(
            &self.body,
            "Dream Memory body",
            MAX_BODY_CHARS,
            DreamMemoryTextShape::Multiline,
        )?;
        if DreamMemoryDate::from_timestamp(self.occurred_at, context.timezone)? != context.date {
            return Err(DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidInput,
                "Dream Memory occurred_at does not belong to the document date",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    #[must_use]
    pub const fn occurred_at(&self) -> i64 {
        self.occurred_at
    }

    #[must_use]
    pub const fn kind(&self) -> MemoryKind {
        self.kind
    }

    #[must_use]
    pub const fn importance(&self) -> MemoryImportance {
        self.importance
    }

    #[must_use]
    pub fn provenance(&self) -> &DreamMemoryProvenance {
        &self.provenance
    }
}

/// Builder for one validated Dream Memory entry.
#[derive(Debug, Clone, Default)]
pub struct DreamMemoryEntryBuilder {
    memory_id: Option<MemoryId>,
    title: Option<String>,
    body: Option<String>,
    occurred_at: Option<i64>,
    kind: Option<MemoryKind>,
    importance: Option<MemoryImportance>,
    provenance: Option<DreamMemoryProvenance>,
}

impl DreamMemoryEntryBuilder {
    #[must_use]
    pub fn memory_id(mut self, value: MemoryId) -> Self {
        self.memory_id = Some(value);
        self
    }

    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }

    #[must_use]
    pub fn body(mut self, value: impl Into<String>) -> Self {
        self.body = Some(value.into());
        self
    }

    #[must_use]
    pub const fn occurred_at(mut self, value: i64) -> Self {
        self.occurred_at = Some(value);
        self
    }

    #[must_use]
    pub const fn kind(mut self, value: MemoryKind) -> Self {
        self.kind = Some(value);
        self
    }

    #[must_use]
    pub const fn importance(mut self, value: MemoryImportance) -> Self {
        self.importance = Some(value);
        self
    }

    #[must_use]
    pub fn provenance(mut self, value: DreamMemoryProvenance) -> Self {
        self.provenance = Some(value);
        self
    }

    pub fn build(self) -> DreamMemoryResult<DreamMemoryEntry> {
        let entry = DreamMemoryEntry {
            memory_id: required(self.memory_id, "Dream Memory id")?,
            title: required(self.title, "Dream Memory title")?,
            body: required(self.body, "Dream Memory body")?,
            occurred_at: required(self.occurred_at, "Dream Memory occurred_at")?,
            kind: required(self.kind, "Dream Memory kind")?,
            importance: required(self.importance, "Dream Memory importance")?,
            provenance: required(self.provenance, "Dream Memory provenance")?,
        };
        validate_text(
            &entry.title,
            "Dream Memory title",
            MAX_TITLE_CHARS,
            DreamMemoryTextShape::SingleLine,
        )?;
        validate_text(
            &entry.body,
            "Dream Memory body",
            MAX_BODY_CHARS,
            DreamMemoryTextShape::Multiline,
        )?;
        Ok(entry)
    }
}

/// Result of applying one idempotent formation batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DreamMemoryAppendResult {
    Appended,
    AlreadyApplied,
}

fn required<T>(value: Option<T>, label: &str) -> DreamMemoryResult<T> {
    value.ok_or_else(|| {
        DreamMemoryError::new(
            DreamMemoryErrorKind::InvalidInput,
            format!("{label} is required"),
        )
    })
}

#[derive(Clone, Copy)]
enum DreamMemoryTextShape {
    SingleLine,
    Multiline,
}

fn validate_text(
    value: &str,
    label: &str,
    max_chars: usize,
    shape: DreamMemoryTextShape,
) -> DreamMemoryResult<()> {
    if value.trim() != value
        || value.is_empty()
        || value.chars().count() > max_chars
        || value.contains('\0')
        || (matches!(shape, DreamMemoryTextShape::SingleLine) && value.lines().count() != 1)
    {
        return Err(DreamMemoryError::new(
            DreamMemoryErrorKind::InvalidInput,
            format!("{label} is invalid"),
        ));
    }
    Ok(())
}
