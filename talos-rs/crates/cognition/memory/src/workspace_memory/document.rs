use talos_knowledge_format::{
    KnowledgeDocument, KnowledgeEntry, KnowledgeEntryQuery, KnowledgeEntryUpdate,
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult,
};
use talos_types::MemoryKind;

use super::{
    user_workspace_memory_profile, validate_memory_key, OptionalField, UserWorkspaceMemoryEntry,
    UserWorkspaceMemoryEntryQuery, UserWorkspaceMemoryEntryUpdate, UserWorkspaceMemorySection,
};

/// Typed User Workspace Memory document backed by the shared format engine.
#[derive(Debug, Clone, PartialEq)]
pub struct UserWorkspaceMemoryDocument {
    document: KnowledgeDocument,
}

impl UserWorkspaceMemoryDocument {
    /// Creates the canonical empty in-memory template.
    pub fn template() -> KnowledgeFormatResult<Self> {
        let profile = user_workspace_memory_profile()?;
        let document = KnowledgeDocument::builder(&profile)
            .create_document()
            .build()?;
        Ok(Self { document })
    }

    /// Parses and fully validates one canonical User Workspace Memory file.
    pub fn parse(content: &str) -> KnowledgeFormatResult<Self> {
        let profile = user_workspace_memory_profile()?;
        let document = KnowledgeDocument::builder(&profile)
            .from_markdown(content)
            .build()?;
        for entry in document.list_entries(KnowledgeEntryQuery::all()) {
            from_knowledge_entry(entry)?;
        }
        Ok(Self { document })
    }

    /// Creates a new entry and rejects an existing key.
    pub fn create_entry(self, entry: UserWorkspaceMemoryEntry) -> KnowledgeFormatResult<Self> {
        self.rebuild(|builder| Ok(builder.create_entry(to_knowledge_entry(entry)?)))
    }

    /// Applies a field-level update to an existing entry.
    pub fn update_entry(
        self,
        key: &str,
        update: UserWorkspaceMemoryEntryUpdate,
    ) -> KnowledgeFormatResult<Self> {
        validate_memory_key(key, "entry key")?;
        update.validate()?;
        let existing = self.get_entry(key).ok_or_else(|| {
            KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::NotFound,
                format!("User Workspace Memory entry not found: {key}"),
            )
        })?;
        let updated = apply_typed_update(existing, update);
        updated.validate()?;
        let body = entry_body(&updated);
        let generic_update = KnowledgeEntryUpdate::builder()
            .section(updated.section.key())
            .title(updated.title)
            .body(body)
            .build()?;
        self.rebuild(|builder| Ok(builder.update_entry(key, generic_update)))
    }

    /// Creates or completely replaces an entry by stable key.
    pub fn upsert_entry(self, entry: UserWorkspaceMemoryEntry) -> KnowledgeFormatResult<Self> {
        self.rebuild(|builder| Ok(builder.upsert_entry(to_knowledge_entry(entry)?)))
    }

    /// Completely replaces an existing entry.
    pub fn replace_entry(self, entry: UserWorkspaceMemoryEntry) -> KnowledgeFormatResult<Self> {
        self.rebuild(|builder| Ok(builder.replace_entry(to_knowledge_entry(entry)?)))
    }

    /// Idempotently removes an entry.
    pub fn remove_entry(self, key: &str) -> KnowledgeFormatResult<Self> {
        validate_memory_key(key, "entry key")?;
        self.rebuild(|builder| Ok(builder.remove_entry(key)))
    }

    /// Returns one typed entry.
    pub fn get_entry(&self, key: &str) -> Option<UserWorkspaceMemoryEntry> {
        self.document
            .get_entry(key)
            .and_then(|entry| from_knowledge_entry(entry).ok())
    }

    /// Lists typed entries in canonical section and file order.
    pub fn list_entries(
        &self,
        query: UserWorkspaceMemoryEntryQuery,
    ) -> KnowledgeFormatResult<Vec<UserWorkspaceMemoryEntry>> {
        let generic = KnowledgeEntryQuery::builder();
        let generic = match query.section {
            Some(section) => generic.section(section.key()),
            None => generic,
        };
        let generic = match query.key_prefix {
            Some(prefix) => generic.key_prefix(prefix),
            None => generic,
        };
        let generic = match query.limit {
            Some(limit) => generic.limit(limit),
            None => generic,
        };
        self.document
            .list_entries(generic.build()?)
            .into_iter()
            .map(from_knowledge_entry)
            .collect()
    }

    /// Renders canonical OKF Markdown.
    pub fn render(&self) -> KnowledgeFormatResult<String> {
        self.document.render()
    }

    /// Renders the dynamic Prompt projection.
    pub fn prompt_view(&self) -> KnowledgeFormatResult<String> {
        self.document.prompt_view()
    }

    fn rebuild(
        self,
        operation: impl FnOnce(
            talos_knowledge_format::KnowledgeDocumentBuilder,
        ) -> KnowledgeFormatResult<
            talos_knowledge_format::KnowledgeDocumentBuilder,
        >,
    ) -> KnowledgeFormatResult<Self> {
        let profile = user_workspace_memory_profile()?;
        let current = self.document.render()?;
        let builder = KnowledgeDocument::builder(&profile).from_markdown(current);
        let document = operation(builder)?.build()?;
        Ok(Self { document })
    }
}

fn to_knowledge_entry(entry: UserWorkspaceMemoryEntry) -> KnowledgeFormatResult<KnowledgeEntry> {
    entry.validate()?;
    let body = entry_body(&entry);
    KnowledgeEntry::builder()
        .key(entry.key)
        .section(entry.section.key())
        .title(entry.title)
        .body(body)
        .build()
}

fn from_knowledge_entry(entry: &KnowledgeEntry) -> KnowledgeFormatResult<UserWorkspaceMemoryEntry> {
    let section = UserWorkspaceMemorySection::parse(entry.section.as_str()).ok_or_else(|| {
        KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidDocument,
            "unknown User Workspace Memory section",
        )
    })?;
    let fields = parse_fields(entry.body.as_str())?;
    UserWorkspaceMemoryEntry::builder()
        .key(entry.key.as_str())
        .section(section)
        .title(entry.title.clone())
        .kind(parse_kind(required_field(&fields, "Kind")?)?)
        .summary(required_field(&fields, "Summary")?)
        .apply_optional("Why", &fields, UserWorkspaceMemoryEntryBuilderField::Why)
        .apply_optional(
            "Apply",
            &fields,
            UserWorkspaceMemoryEntryBuilderField::Apply,
        )
        .apply_optional(
            "Status",
            &fields,
            UserWorkspaceMemoryEntryBuilderField::Status,
        )
        .apply_optional("Next", &fields, UserWorkspaceMemoryEntryBuilderField::Next)
        .build()
}

trait OptionalBuilderFields {
    fn apply_optional(
        self,
        name: &str,
        fields: &std::collections::BTreeMap<String, String>,
        field: UserWorkspaceMemoryEntryBuilderField,
    ) -> Self;
}

#[derive(Debug, Clone, Copy)]
enum UserWorkspaceMemoryEntryBuilderField {
    Why,
    Apply,
    Status,
    Next,
}

impl OptionalBuilderFields for super::UserWorkspaceMemoryEntryBuilder {
    fn apply_optional(
        self,
        name: &str,
        fields: &std::collections::BTreeMap<String, String>,
        field: UserWorkspaceMemoryEntryBuilderField,
    ) -> Self {
        let Some(value) = fields.get(name) else {
            return self;
        };
        match field {
            UserWorkspaceMemoryEntryBuilderField::Why => self.why(value),
            UserWorkspaceMemoryEntryBuilderField::Apply => self.apply(value),
            UserWorkspaceMemoryEntryBuilderField::Status => self.status(value),
            UserWorkspaceMemoryEntryBuilderField::Next => self.next(value),
        }
    }
}

fn entry_body(entry: &UserWorkspaceMemoryEntry) -> String {
    let mut fields = vec![
        format!("- **Kind:** {}", kind_name(entry.kind)),
        format!("- **Summary:** {}", entry.summary),
    ];
    for (name, value) in [
        ("Why", entry.why.as_ref()),
        ("Apply", entry.apply.as_ref()),
        ("Status", entry.status.as_ref()),
        ("Next", entry.next.as_ref()),
    ] {
        if let Some(value) = value {
            fields.push(format!("- **{name}:** {value}"));
        }
    }
    fields.join("\n")
}

fn parse_fields(body: &str) -> KnowledgeFormatResult<std::collections::BTreeMap<String, String>> {
    let mut fields = std::collections::BTreeMap::new();
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("- **") else {
            return Err(invalid_body());
        };
        let Some((name, value)) = rest.split_once(":** ") else {
            return Err(invalid_body());
        };
        if value.trim().is_empty() || fields.insert(name.to_string(), value.to_string()).is_some() {
            return Err(invalid_body());
        }
    }
    Ok(fields)
}

fn required_field<'a>(
    fields: &'a std::collections::BTreeMap<String, String>,
    name: &str,
) -> KnowledgeFormatResult<&'a str> {
    fields
        .get(name)
        .map(String::as_str)
        .ok_or_else(invalid_body)
}

fn parse_kind(value: &str) -> KnowledgeFormatResult<MemoryKind> {
    match value {
        "Preference" => Ok(MemoryKind::Preference),
        "Fact" => Ok(MemoryKind::Fact),
        "Decision" => Ok(MemoryKind::Decision),
        "Task" => Ok(MemoryKind::Task),
        "Episode" => Ok(MemoryKind::Episode),
        _ => Err(invalid_body()),
    }
}

const fn kind_name(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Preference => "Preference",
        MemoryKind::Fact => "Fact",
        MemoryKind::Decision => "Decision",
        MemoryKind::Task => "Task",
        MemoryKind::Episode => "Episode",
    }
}

fn apply_typed_update(
    mut entry: UserWorkspaceMemoryEntry,
    update: UserWorkspaceMemoryEntryUpdate,
) -> UserWorkspaceMemoryEntry {
    if let Some(value) = update.section {
        entry.section = value;
    }
    if let Some(value) = update.title {
        entry.title = value;
    }
    if let Some(value) = update.kind {
        entry.kind = value;
    }
    if let Some(value) = update.summary {
        entry.summary = value;
    }
    apply_optional_field(&mut entry.why, update.why);
    apply_optional_field(&mut entry.apply, update.apply);
    apply_optional_field(&mut entry.status, update.status);
    apply_optional_field(&mut entry.next, update.next);
    entry
}

fn apply_optional_field(target: &mut Option<String>, update: OptionalField<String>) {
    match update {
        OptionalField::Unchanged => {}
        OptionalField::Set(value) => *target = Some(value),
        OptionalField::Clear => *target = None,
    }
}

fn invalid_body() -> KnowledgeFormatError {
    KnowledgeFormatError::new(
        KnowledgeFormatErrorKind::InvalidDocument,
        "invalid User Workspace Memory entry body",
    )
}
