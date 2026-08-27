use talos_types::MemoryKind;

use super::*;

fn entry() -> UserWorkspaceMemoryEntry {
    UserWorkspaceMemoryEntry::builder()
        .key("memory-ownership")
        .section(UserWorkspaceMemorySection::KeyKnowledge)
        .title("Memory ownership")
        .kind(MemoryKind::Decision)
        .summary("Session model exclusively maintains User Workspace Memory.")
        .why("Scheduled rewriting conflicts with concurrent Session updates.")
        .apply("Daily maintenance only writes Daily Memory.")
        .build()
        .expect("entry should build")
}

#[test]
fn template_is_canonical_okf_memory_document() {
    let document = UserWorkspaceMemoryDocument::template().expect("template should build");
    let markdown = document.render().expect("template should render");

    assert!(markdown.contains("type: UserWorkspaceMemory"));
    assert!(markdown.contains("status: stable"));
    assert!(markdown.contains("<!-- talos:user-workspace-memory:start -->"));
    assert!(markdown.contains("## Interaction Guidance"));
    assert!(markdown.contains("## Key Knowledge"));
    assert!(markdown.contains("## Active Context"));
    assert!(markdown.chars().count() <= talos_types::MEMORY_FILE_MAX_CHARS);
}

#[test]
fn typed_entry_round_trips_through_standard_document_api() {
    let created = UserWorkspaceMemoryDocument::template()
        .expect("template should build")
        .upsert_entry(entry())
        .expect("entry should upsert");
    let markdown = created.render().expect("document should render");
    let parsed = UserWorkspaceMemoryDocument::parse(&markdown).expect("document should parse");

    assert_eq!(parsed.get_entry("memory-ownership"), Some(entry()));
    assert!(parsed
        .prompt_view()
        .expect("prompt view should render")
        .contains("- **Kind:** Decision"));
    assert!(!parsed
        .prompt_view()
        .expect("prompt view should render")
        .contains("talos:user-workspace-memory"));
}

#[test]
fn optional_field_clear_removes_existing_value() {
    let document = UserWorkspaceMemoryDocument::template()
        .expect("template should build")
        .create_entry(entry())
        .expect("entry should create")
        .update_entry(
            "memory-ownership",
            UserWorkspaceMemoryEntryUpdate::builder()
                .why(OptionalField::Clear)
                .summary("The Session model owns all automatic updates.")
                .build()
                .expect("update should build"),
        )
        .expect("entry should update");

    let updated = document
        .get_entry("memory-ownership")
        .expect("entry should exist");
    assert_eq!(updated.why, None);
    assert_eq!(
        updated.summary,
        "The Session model owns all automatic updates."
    );
}

#[test]
fn entry_and_update_reject_multiline_structured_fields() {
    let entry_error = UserWorkspaceMemoryEntry::builder()
        .key("invalid-entry")
        .section(UserWorkspaceMemorySection::KeyKnowledge)
        .title("Invalid entry")
        .kind(MemoryKind::Fact)
        .summary("first line\nsecond line")
        .build()
        .expect_err("multiline summary should fail");
    assert_eq!(
        entry_error.kind(),
        talos_knowledge_format::KnowledgeFormatErrorKind::InvalidEntry
    );

    let update_error = UserWorkspaceMemoryEntryUpdate::builder()
        .why(OptionalField::Set("first line\nsecond line".to_string()))
        .build()
        .expect_err("multiline update field should fail");
    assert_eq!(
        update_error.kind(),
        talos_knowledge_format::KnowledgeFormatErrorKind::InvalidEntry
    );
}

#[test]
fn document_rejects_directly_constructed_invalid_entry() {
    let mut invalid = entry();
    invalid.summary = "first line\nsecond line".to_string();

    let error = UserWorkspaceMemoryDocument::template()
        .expect("template should build")
        .upsert_entry(invalid)
        .expect_err("document boundary should validate direct values");
    assert_eq!(
        error.kind(),
        talos_knowledge_format::KnowledgeFormatErrorKind::InvalidEntry
    );

    for error in [
        UserWorkspaceMemoryDocument::template()
            .expect("template should build")
            .update_entry("Invalid Key", UserWorkspaceMemoryEntryUpdate::default())
            .expect_err("invalid update key should fail"),
        UserWorkspaceMemoryDocument::template()
            .expect("template should build")
            .remove_entry("Invalid Key")
            .expect_err("invalid remove key should fail"),
    ] {
        assert_eq!(
            error.kind(),
            talos_knowledge_format::KnowledgeFormatErrorKind::InvalidEntry
        );
    }
}
