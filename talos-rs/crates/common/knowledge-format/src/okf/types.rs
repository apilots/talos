/// Opaque unknown producer fields retained during OKF round trips.
///
/// Callers can discover extension names, but cannot interpret or construct
/// arbitrary YAML values. Known OKF fields are exposed through typed APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OkfExtensionFields(Vec<String>);

impl OkfExtensionFields {
    pub(crate) fn from_frontmatter(frontmatter: &okf_core::Frontmatter) -> Self {
        let mut names: Vec<_> = frontmatter
            .extension_keys()
            .into_iter()
            .map(str::to_string)
            .collect();
        names.sort();
        Self(names)
    }

    /// Returns true when an opaque extension with this name is present.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.0.iter().any(|candidate| candidate == name)
    }

    /// Extension names in stable order.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.0
    }
}

/// An OKF specification version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfVersion(String);

impl OkfVersion {
    /// Returns the OKF version implemented by this facade.
    #[must_use]
    pub fn current() -> Self {
        Self(okf_core::OKF_VERSION.to_string())
    }

    /// Parses a supported OKF version.
    pub fn parse(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        super::SUPPORTED_OKF_VERSIONS
            .contains(&value.as_str())
            .then_some(Self(value))
    }

    /// Returns the version string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A strict ISO-8601 date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OkfDate {
    year: i32,
    month: u32,
    day: u32,
}

impl OkfDate {
    /// Parses a strict `YYYY-MM-DD` value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        okf_core::Date::parse(value).map(Into::into)
    }
}

impl std::fmt::Display for OkfDate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

impl From<okf_core::Date> for OkfDate {
    fn from(value: okf_core::Date) -> Self {
        Self {
            year: value.year,
            month: value.month,
            day: value.day,
        }
    }
}

/// An ISO-8601 date-time with its original time and offset semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OkfDateTime {
    date: OkfDate,
    hour: u32,
    minute: u32,
    second: u32,
    nanosecond: u32,
    offset_minutes: Option<i32>,
    has_time: bool,
}

/// Parsed OKF date-time or an explicitly retained non-canonical producer value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OkfDateTimeValue {
    Parsed(OkfDateTime),
    Raw(String),
}

impl OkfDateTimeValue {
    pub(crate) fn from_raw(value: String) -> Self {
        OkfDateTime::parse(&value).map_or(Self::Raw(value), Self::Parsed)
    }

    /// Returns the exact semantic value used for OKF serialization.
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::Parsed(value) => value.to_iso8601(),
            Self::Raw(value) => value.clone(),
        }
    }

    /// Returns the validated date-time when the producer value is canonical.
    #[must_use]
    pub const fn parsed(&self) -> Option<OkfDateTime> {
        match self {
            Self::Parsed(value) => Some(*value),
            Self::Raw(_) => None,
        }
    }
}

impl From<OkfDateTime> for OkfDateTimeValue {
    fn from(value: OkfDateTime) -> Self {
        Self::Parsed(value)
    }
}

impl OkfDateTime {
    /// Parses an OKF-compatible ISO-8601 date-time.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        okf_core::DateTime::parse(value).map(Into::into)
    }

    pub(crate) fn into_core(self) -> okf_core::DateTime {
        okf_core::DateTime {
            date: okf_core::Date {
                year: self.date.year,
                month: self.date.month,
                day: self.date.day,
            },
            hour: self.hour,
            minute: self.minute,
            second: self.second,
            nanosecond: self.nanosecond,
            offset_minutes: self.offset_minutes,
            has_time: self.has_time,
        }
    }

    /// Renders a canonical ISO-8601 value with its explicit offset.
    #[must_use]
    pub fn to_iso8601(self) -> String {
        let date = format!(
            "{:04}-{:02}-{:02}",
            self.date.year, self.date.month, self.date.day
        );
        if !self.has_time {
            return date;
        }
        let fraction = if self.nanosecond == 0 {
            String::new()
        } else {
            format!(".{:09}", self.nanosecond)
                .trim_end_matches('0')
                .to_string()
        };
        let offset = match self.offset_minutes {
            Some(0) => "Z".to_string(),
            Some(minutes) => {
                let sign = if minutes < 0 { '-' } else { '+' };
                let minutes = minutes.abs();
                format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
            }
            None => String::new(),
        };
        format!(
            "{date}T{:02}:{:02}:{:02}{fraction}{offset}",
            self.hour, self.minute, self.second
        )
    }
}

impl From<okf_core::DateTime> for OkfDateTime {
    fn from(value: okf_core::DateTime) -> Self {
        Self {
            date: value.date.into(),
            hour: value.hour,
            minute: value.minute,
            second: value.second,
            nanosecond: value.nanosecond,
            offset_minutes: value.offset_minutes,
            has_time: value.has_time,
        }
    }
}

/// OKF actor convention value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OkfActor {
    raw: String,
}

impl OkfActor {
    /// Parses and preserves an actor string.
    #[must_use]
    pub fn parse(value: impl Into<String>) -> Self {
        Self { raw: value.into() }
    }

    /// Returns the actor string exactly as supplied.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl From<okf_core::Actor> for OkfActor {
    fn from(value: okf_core::Actor) -> Self {
        Self::parse(value.as_str())
    }
}

/// Derived OKF trust tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OkfTrustTier {
    Unverified,
    MachineConfirmed,
    HumanReviewed,
}

impl From<okf_core::TrustTier> for OkfTrustTier {
    fn from(value: okf_core::TrustTier) -> Self {
        match value {
            okf_core::TrustTier::Unverified => Self::Unverified,
            okf_core::TrustTier::MachineConfirmed => Self::MachineConfirmed,
            okf_core::TrustTier::HumanReviewed => Self::HumanReviewed,
        }
    }
}

/// OKF lifecycle status. Unknown values remain consumable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OkfStatus {
    Draft,
    Stable,
    Deprecated,
    Other(String),
}

impl OkfStatus {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Draft => "draft",
            Self::Stable => "stable",
            Self::Deprecated => "deprecated",
            Self::Other(value) => value,
        }
    }
}

impl From<okf_core::Status> for OkfStatus {
    fn from(value: okf_core::Status) -> Self {
        match value {
            okf_core::Status::Draft => Self::Draft,
            okf_core::Status::Stable => Self::Stable,
            okf_core::Status::Deprecated => Self::Deprecated,
            okf_core::Status::Other(value) => Self::Other(value),
        }
    }
}

/// Producer identity and timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfGenerated {
    pub by: Option<OkfActor>,
    pub at: Option<OkfDateTimeValue>,
}

impl OkfGenerated {
    /// Creates a complete producer signal.
    #[must_use]
    pub fn new(by: OkfActor, at: OkfDateTime) -> Self {
        Self {
            by: Some(by),
            at: Some(at.into()),
        }
    }
}

/// One verification event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfVerification {
    pub by: Option<OkfActor>,
    pub at: Option<OkfDateTimeValue>,
    pub valid: bool,
}

impl OkfVerification {
    /// Creates a complete verification event.
    #[must_use]
    pub fn new(by: OkfActor, at: OkfDateTime) -> Self {
        Self {
            by: Some(by),
            at: Some(at.into()),
            valid: true,
        }
    }
}

/// Source usage interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfUsageWindow {
    pub from: Option<OkfDateTimeValue>,
    pub to: Option<OkfDateTimeValue>,
}

impl OkfUsageWindow {
    /// Creates a bounded usage window.
    #[must_use]
    pub fn new(from: OkfDateTime, to: OkfDateTime) -> Self {
        Self {
            from: Some(from.into()),
            to: Some(to.into()),
        }
    }
}

/// Classification of a source resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OkfResourceKind {
    Url,
    Path,
    Scope,
    Missing,
}

/// One provenance source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfSource {
    pub id: Option<String>,
    pub resource: Option<String>,
    pub title: Option<String>,
    pub author: Option<OkfActor>,
    pub usage_count: Option<i64>,
    pub last_modified: Option<OkfDateTimeValue>,
    pub usage_window: Option<OkfUsageWindow>,
    pub resource_kind: OkfResourceKind,
}

impl OkfSource {
    /// Starts a typed provenance source builder.
    #[must_use]
    pub fn builder(resource: impl Into<String>) -> OkfSourceBuilder {
        OkfSourceBuilder {
            source: Self {
                id: None,
                resource: Some(resource.into()),
                title: None,
                author: None,
                usage_count: None,
                last_modified: None,
                usage_window: None,
                resource_kind: OkfResourceKind::Missing,
            },
        }
    }
}

/// Builder for one typed provenance source.
#[derive(Debug, Clone)]
pub struct OkfSourceBuilder {
    source: OkfSource,
}

impl OkfSourceBuilder {
    #[must_use]
    pub fn id(mut self, value: impl Into<String>) -> Self {
        self.source.id = Some(value.into());
        self
    }

    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.source.title = Some(value.into());
        self
    }

    #[must_use]
    pub fn author(mut self, value: OkfActor) -> Self {
        self.source.author = Some(value);
        self
    }

    #[must_use]
    pub fn usage_count(mut self, value: i64) -> Self {
        self.source.usage_count = Some(value);
        self
    }

    #[must_use]
    pub fn last_modified(mut self, value: OkfDateTime) -> Self {
        self.source.last_modified = Some(value.into());
        self
    }

    #[must_use]
    pub fn usage_window(mut self, value: OkfUsageWindow) -> Self {
        self.source.usage_window = Some(value);
        self
    }

    /// Builds the source and derives its resource classification.
    #[must_use]
    pub fn build(mut self) -> OkfSource {
        self.source.resource_kind = classify_resource(self.source.resource.as_deref());
        self.source
    }
}

impl From<okf_core::Source> for OkfSource {
    fn from(value: okf_core::Source) -> Self {
        let resource_kind = match value.resource_kind() {
            okf_core::ResourceKind::Url => OkfResourceKind::Url,
            okf_core::ResourceKind::Path => OkfResourceKind::Path,
            okf_core::ResourceKind::Scope => OkfResourceKind::Scope,
            okf_core::ResourceKind::Missing => OkfResourceKind::Missing,
        };
        Self {
            id: value.id,
            resource: value.resource,
            title: value.title,
            author: value.author.map(Into::into),
            usage_count: value.usage_count,
            last_modified: value
                .last_modified
                .map(|value| OkfDateTimeValue::from_raw(value.raw)),
            usage_window: value.usage_window.map(|window| OkfUsageWindow {
                from: window
                    .from
                    .map(|value| OkfDateTimeValue::from_raw(value.raw)),
                to: window.to.map(|value| OkfDateTimeValue::from_raw(value.raw)),
            }),
            resource_kind,
        }
    }
}

fn classify_resource(resource: Option<&str>) -> OkfResourceKind {
    match resource.map(str::trim) {
        None | Some("") => OkfResourceKind::Missing,
        Some(value) if value.contains("://") || value.starts_with("mailto:") => {
            OkfResourceKind::Url
        }
        Some(value) if value.chars().any(char::is_whitespace) => OkfResourceKind::Scope,
        Some(_) => OkfResourceKind::Path,
    }
}

/// A body footnote joined to provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfAttribution {
    pub label: String,
    pub source: Option<OkfSource>,
    pub references: usize,
    pub definitions: usize,
}

/// A Markdown link found in a concept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfLink {
    pub text: String,
    pub target: String,
    pub kind: OkfLinkKind,
}

/// Structural kind of a Markdown link in an OKF concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OkfLinkKind {
    Absolute,
    Relative,
    External,
    Anchor,
    Other,
}
