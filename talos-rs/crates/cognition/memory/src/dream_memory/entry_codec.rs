use std::collections::BTreeMap;

use talos_knowledge_format::KnowledgeEntry;
use talos_types::{MemoryImportance, MemoryKind};

use super::{
    DreamMemoryDocumentContext, DreamMemoryEntry, DreamMemoryError, DreamMemoryErrorKind,
    DreamMemoryProvenance, DreamMemoryResult, DreamMemorySourceRange, MemoryId, MemorySessionId,
};

const MEMORIES_SECTION: &str = "memories";

pub(super) fn to_knowledge_entry(entry: &DreamMemoryEntry) -> DreamMemoryResult<KnowledgeEntry> {
    KnowledgeEntry::builder()
        .key(entry.memory_id().as_str())
        .section(MEMORIES_SECTION)
        .title(entry.title())
        .body(render_entry_body(entry))
        .build()
        .map_err(Into::into)
}

pub(super) fn from_knowledge_entry(
    context: &DreamMemoryDocumentContext,
    entry: &KnowledgeEntry,
) -> DreamMemoryResult<DreamMemoryEntry> {
    if entry.section.as_str() != MEMORIES_SECTION {
        return Err(invalid_document("unknown Dream Memory section"));
    }
    let (field_block, body) =
        entry.body.as_str().split_once("\n\n").ok_or_else(|| {
            invalid_document("Dream Memory entry body is missing field separator")
        })?;
    let mut fields = BTreeMap::new();
    let mut field_order = Vec::new();
    for line in field_block.lines() {
        let (name, value) = parse_field(line)?;
        field_order.push(name.clone());
        if fields.insert(name, value).is_some() {
            return Err(invalid_document("duplicate Dream Memory entry field"));
        }
    }
    let occurred_at = required_field(&fields, "Occurred At")?
        .parse::<i64>()
        .map_err(|_| invalid_document("Dream Memory occurred_at is invalid"))?;
    let kind = parse_kind(required_field(&fields, "Kind")?)?;
    let importance = parse_importance(required_field(&fields, "Importance")?)?;
    let provenance = match (
        fields.get("Session"),
        fields.get("Source From Seq"),
        fields.get("Source Until Seq"),
    ) {
        (None, None, None) => DreamMemoryProvenance::UserAuthored,
        (Some(session), Some(from), Some(until)) => DreamMemoryProvenance::Generated {
            session_id: MemorySessionId::parse(session.clone())?,
            source_range: DreamMemorySourceRange::new(
                from.parse()
                    .map_err(|_| invalid_document("Dream Memory source range is invalid"))?,
                until
                    .parse()
                    .map_err(|_| invalid_document("Dream Memory source range is invalid"))?,
            )?,
        },
        _ => {
            return Err(invalid_document(
                "Dream Memory provenance fields must be complete",
            ));
        }
    };
    let expected_order = match &provenance {
        DreamMemoryProvenance::Generated { .. } => vec![
            "Occurred At",
            "Session",
            "Kind",
            "Importance",
            "Source From Seq",
            "Source Until Seq",
        ],
        DreamMemoryProvenance::UserAuthored => vec!["Occurred At", "Kind", "Importance"],
    };
    if field_order != expected_order {
        return Err(invalid_document(
            "Dream Memory entry fields are not in canonical order",
        ));
    }
    let parsed = DreamMemoryEntry::builder()
        .memory_id(MemoryId::parse(entry.key.as_str())?)
        .title(entry.title.clone())
        .body(body.to_string())
        .occurred_at(occurred_at)
        .kind(kind)
        .importance(importance)
        .provenance(provenance)
        .build()?;
    parsed.validate_for(context)?;
    Ok(parsed)
}

fn render_entry_body(entry: &DreamMemoryEntry) -> String {
    let mut fields = vec![format!("- **Occurred At:** {}", entry.occurred_at())];
    if let DreamMemoryProvenance::Generated { session_id, .. } = entry.provenance() {
        fields.push(format!("- **Session:** {}", session_id.as_str()));
    }
    fields.push(format!("- **Kind:** {}", kind_name(entry.kind())));
    fields.push(format!(
        "- **Importance:** {}",
        importance_name(entry.importance())
    ));
    if let DreamMemoryProvenance::Generated { source_range, .. } = entry.provenance() {
        fields.push(format!(
            "- **Source From Seq:** {}",
            source_range.from_seq()
        ));
        fields.push(format!(
            "- **Source Until Seq:** {}",
            source_range.until_seq()
        ));
    }
    format!("{}\n\n{}", fields.join("\n"), entry.body())
}

fn parse_field(line: &str) -> DreamMemoryResult<(String, String)> {
    let content = line
        .strip_prefix("- **")
        .and_then(|value| value.split_once(":** "))
        .ok_or_else(|| invalid_document("invalid Dream Memory entry field"))?;
    let allowed = [
        "Occurred At",
        "Session",
        "Source From Seq",
        "Source Until Seq",
        "Kind",
        "Importance",
    ];
    if !allowed.contains(&content.0) || content.1.is_empty() || content.1.lines().count() != 1 {
        return Err(invalid_document("unknown Dream Memory entry field"));
    }
    Ok((content.0.to_string(), content.1.to_string()))
}

fn required_field<'a>(
    fields: &'a BTreeMap<String, String>,
    name: &str,
) -> DreamMemoryResult<&'a str> {
    fields
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| invalid_document(format!("Dream Memory entry is missing {name}")))
}

fn parse_kind(value: &str) -> DreamMemoryResult<MemoryKind> {
    match value {
        "preference" => Ok(MemoryKind::Preference),
        "fact" => Ok(MemoryKind::Fact),
        "decision" => Ok(MemoryKind::Decision),
        "task" => Ok(MemoryKind::Task),
        "episode" => Ok(MemoryKind::Episode),
        _ => Err(invalid_document("Dream Memory kind is invalid")),
    }
}

fn kind_name(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Preference => "preference",
        MemoryKind::Fact => "fact",
        MemoryKind::Decision => "decision",
        MemoryKind::Task => "task",
        MemoryKind::Episode => "episode",
    }
}

fn parse_importance(value: &str) -> DreamMemoryResult<MemoryImportance> {
    match value {
        "low" => Ok(MemoryImportance::Low),
        "normal" => Ok(MemoryImportance::Normal),
        "high" => Ok(MemoryImportance::High),
        _ => Err(invalid_document("Dream Memory importance is invalid")),
    }
}

fn importance_name(importance: MemoryImportance) -> &'static str {
    match importance {
        MemoryImportance::Low => "low",
        MemoryImportance::Normal => "normal",
        MemoryImportance::High => "high",
    }
}

fn invalid_document(message: impl Into<String>) -> DreamMemoryError {
    DreamMemoryError::new(DreamMemoryErrorKind::InvalidDocument, message)
}
