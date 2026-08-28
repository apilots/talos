use chrono::{TimeZone, Utc};
use chrono_tz::Asia::Shanghai;
use talos_types::{MemoryImportance, MemoryKind};

use super::*;
use crate::{MemoryCandidate, MemoryWriteBatch};

fn context(date: &str) -> DreamMemoryDocumentContext {
    DreamMemoryDocumentContext::new(
        MemoryAgentId::parse("agent-1").expect("valid agent"),
        MemoryUserId::parse("user-1").expect("valid user"),
        DreamMemoryDate::parse(date).expect("valid date"),
        MemoryTimezone::new(Shanghai),
    )
}

fn timestamp(year: i32, month: u32, day: u32, hour: u32) -> i64 {
    Shanghai
        .with_ymd_and_hms(year, month, day, hour, 30, 0)
        .single()
        .expect("valid timestamp")
        .with_timezone(&Utc)
        .timestamp()
}

fn batch(from_seq: u64, until_seq: u64) -> MemoryWriteBatch {
    MemoryWriteBatch {
        session_id: "session-1".to_string(),
        occurred_at: timestamp(2026, 8, 27, 9),
        source_from_seq: from_seq,
        source_until_seq: until_seq,
        candidates: vec![MemoryCandidate {
            title: "Stable preference".to_string(),
            body: "The user prefers concise answers.".to_string(),
            kind: MemoryKind::Preference,
            importance: MemoryImportance::Normal,
        }],
    }
}

#[test]
fn document_round_trip_is_canonical() {
    let context = context("2026-08-27");
    let mut document = DreamMemoryDocument::new(context.clone(), batch(10, 24).occurred_at)
        .expect("create document");
    let result = document.append_batch(&batch(10, 24)).expect("append batch");
    assert_eq!(result, DreamMemoryAppendResult::Appended);
    let memory_id = document.entries()[0].memory_id();
    let markdown = document.render().expect("render document");
    assert!(markdown.contains("type: DreamMemory"));
    assert!(markdown.contains("generated:"));
    assert!(markdown.contains("by: talos-agent/agent-1"));
    assert!(markdown.contains("<!-- talos:dream-memory:start -->"));
    assert!(markdown.contains(&format!(
        "<!-- talos:dream-memory:entry key=\"{}\" -->",
        memory_id.as_str()
    )));
    assert!(markdown.contains(
        "- **Occurred At:** 1787794200\n- **Session:** session-1\n- **Kind:** preference\n- **Importance:** normal\n- **Source From Seq:** 10\n- **Source Until Seq:** 24\n\nThe user prefers concise answers."
    ));

    let parsed = DreamMemoryDocument::parse(context, &markdown).expect("parse round trip");
    assert_eq!(parsed.entries(), document.entries());

    let date = DreamMemoryDate::parse("2026-08-27").expect("date");
    let concept = DreamMemoryBundleConcept::from_content(date, &markdown, parsed.entries())
        .expect("build bundle concept");
    let first = DreamMemoryBundle::reconcile(
        std::slice::from_ref(&concept),
        None,
        DreamMemoryBundleEventKind::Creation,
        date,
    )
    .expect("build bundle");
    assert_eq!(
        DreamMemoryBundle::parse_index_dates(&first.render_index()).expect("parse index"),
        vec![date]
    );
    assert!(first
        .render_index()
        .contains("Contains 1 durable memory: Stable preference."));
    let first_log = first.render_log().expect("non-empty bundle log");
    assert!(first_log.contains("**Creation**: [Dream Memory 2026-08-27](2026-08-27.md) revision `"));
    assert!(first_log.contains("Contains 1 durable memory: Stable preference."));
    let earlier_wording = first_log.replace(
        "Contains 1 durable memory: Stable preference.",
        "Earlier summary wording.",
    );
    let retried = DreamMemoryBundle::reconcile(
        std::slice::from_ref(&concept),
        Some(&earlier_wording),
        DreamMemoryBundleEventKind::Recovery,
        date,
    )
    .expect("reconcile bundle");
    assert_eq!(retried.render_log(), Some(earlier_wording));

    let recovered = DreamMemoryBundle::reconcile(
        std::slice::from_ref(&concept),
        Some("invalid log"),
        DreamMemoryBundleEventKind::Recovery,
        date,
    )
    .expect("recover corrupt log");
    let recovered_log = recovered.render_log().expect("recovered bundle log");
    assert_eq!(recovered_log.matches(" revision `").count(), 1);
    assert!(recovered_log.contains("**Recovery**:"));

    let empty = DreamMemoryBundle::reconcile(
        &[],
        Some(&first_log),
        DreamMemoryBundleEventKind::Recovery,
        date,
    )
    .expect("build empty bundle");
    assert!(empty.render_log().is_none());

    let bounded = DreamMemoryBundleConcept::new(
        date,
        DreamMemoryConceptRevision::from_content("bounded"),
        vec![
            "First durable fact".to_string(),
            "Second durable preference".to_string(),
            "Third durable decision".to_string(),
            "Fourth omitted title".to_string(),
        ],
    )
    .expect("build bounded summary");
    let bounded_index =
        DreamMemoryBundle::reconcile(&[bounded], None, DreamMemoryBundleEventKind::Creation, date)
            .expect("build bounded index")
            .render_index();
    assert!(bounded_index.contains("and 1 more."));
    assert!(!bounded_index.contains("Fourth omitted title"));
}

#[test]
fn append_is_idempotent_and_rejects_overlap_or_wrong_partition() {
    let mut document = DreamMemoryDocument::new(context("2026-08-27"), batch(10, 24).occurred_at)
        .expect("create document");
    document.append_batch(&batch(10, 24)).expect("first append");
    assert!(matches!(
        document.append_batch(&batch(10, 24)).expect("retry"),
        DreamMemoryAppendResult::AlreadyApplied
    ));
    let overlap = document.append_batch(&batch(20, 30)).expect_err("overlap");
    assert_eq!(overlap.kind(), DreamMemoryErrorKind::Conflict);

    let mut wrong_date = batch(30, 40);
    wrong_date.occurred_at = timestamp(2026, 8, 28, 9);
    let error = document
        .append_batch(&wrong_date)
        .expect_err("wrong date must fail");
    assert_eq!(error.kind(), DreamMemoryErrorKind::InvalidInput);

    let mut colliding_batch = batch(30, 40);
    colliding_batch.candidates.push(MemoryCandidate {
        title: "Stable constraint".to_string(),
        body: "The user requires deterministic updates.".to_string(),
        kind: MemoryKind::Fact,
        importance: MemoryImportance::High,
    });
    let mut probe = DreamMemoryDocument::new(context("2026-08-27"), colliding_batch.occurred_at)
        .expect("probe document");
    probe
        .append_batch(&colliding_batch)
        .expect("derive stable ids");
    let colliding_id = probe.entries()[1].memory_id().clone();
    document
        .create_entry(
            DreamMemoryEntry::builder()
                .memory_id(colliding_id)
                .title("Existing user memory")
                .body("This entry already owns the generated id.")
                .occurred_at(timestamp(2026, 8, 27, 9))
                .kind(MemoryKind::Fact)
                .importance(MemoryImportance::Normal)
                .provenance(DreamMemoryProvenance::UserAuthored)
                .build()
                .expect("existing entry"),
        )
        .expect("seed id collision");
    let before = document.clone();
    let collision = document
        .append_batch(&colliding_batch)
        .expect_err("later id collision must fail atomically");
    assert_eq!(collision.kind(), DreamMemoryErrorKind::Conflict);
    assert_eq!(document, before);
}
