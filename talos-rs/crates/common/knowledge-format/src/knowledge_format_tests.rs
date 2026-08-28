use super::*;

fn profile() -> KnowledgeDocumentProfile {
    KnowledgeDocumentProfile::builder()
        .frontmatter(
            OkfFrontmatter::builder()
                .concept_type("UserWorkspaceMemory")
                .title("User Workspace Memory")
                .description("Durable cross-session memory.")
                .status(OkfStatus::Stable)
                .build()
                .expect("frontmatter should build"),
        )
        .region_namespace("talos:user-workspace-memory")
        .section("interaction-guidance", "Interaction Guidance")
        .section("key-knowledge", "Key Knowledge")
        .section("active-context", "Active Context")
        .max_chars(8_000)
        .build()
        .expect("profile should build")
}

fn entry(key: &str, section: &str, summary: &str) -> KnowledgeEntry {
    KnowledgeEntry::builder()
        .key(key)
        .section(section)
        .title("Memory ownership")
        .body(format!("- **Summary:** {summary}"))
        .build()
        .expect("entry should build")
}

#[test]
fn okf_document_round_trips_extensions_and_standard_families() {
    let source = "---\ntype: Decision\ntitle: Memory ownership\nstatus: stable\nowner: talos\ngenerated: { by: reference_agent/gemini-2.5-pro, at: 2026-06-20T22:53:05Z }\nverified: { by: human:reviewer, at: 2026-06-25T09:00:00Z }\nsources:\n  - id: spec\n    resource: references/spec.md\n---\n\n# Memory ownership\n\nSee [the spec](references/spec.md).[^spec]\n\n[^spec]: Local specification.\n";

    let document = OkfDocument::parse(source).expect("OKF document should parse");

    assert!(document.validate().is_conformant());
    assert!(document.frontmatter().extensions().contains("owner"));
    assert_eq!(document.trust_tier(), OkfTrustTier::HumanReviewed);
    assert_eq!(document.sources().len(), 1);
    assert_eq!(document.attributions().len(), 1);
    assert_eq!(document.links().len(), 1);
    assert!(document.render().contains("owner: talos"));
}

#[test]
fn in_memory_bundle_resolves_links_without_file_system_access() {
    let bundle = OkfBundle::builder()
        .file(OkfBundleFile::new(
            "overview.md",
            "---\ntype: Guide\ntitle: Overview\ntags: [guide]\n---\n\nRead [details](details.md).\n",
        ))
        .file(OkfBundleFile::new(
            "details.md",
            "---\ntype: Guide\ntitle: Details\n---\n\n# Details\n",
        ))
        .file(OkfBundleFile::new(
            "index.md",
            "---\nokf_version: \"0.2\"\n---\n\n# Guide\n",
        ))
        .build()
        .expect("bundle should build");

    assert_eq!(bundle.concepts().len(), 2);
    let resolved = bundle.resolve_links();
    assert_eq!(resolved.len(), 1);
    assert!(resolved[0].exists);
    assert_eq!(
        bundle.okf_version().expect("version should exist").as_str(),
        "0.2"
    );
    assert_eq!(bundle.tags()["guide"].len(), 1);
    assert!(bundle.validate().is_conformant());
}

#[test]
fn managed_document_builder_supports_complete_entry_crud() {
    let profile = profile();
    let created = KnowledgeDocument::builder(&profile)
        .create_document()
        .create_entry(entry(
            "memory-ownership",
            "key-knowledge",
            "Session model owns writes.",
        ))
        .build()
        .expect("document should build");

    let updated = KnowledgeDocument::builder(&profile)
        .from_markdown(created.render().expect("document should render"))
        .update_entry(
            "memory-ownership",
            KnowledgeEntryUpdate::builder()
                .body("- **Summary:** Session model exclusively owns writes.")
                .build()
                .expect("update should build"),
        )
        .upsert_entry(entry(
            "concise-output",
            "interaction-guidance",
            "Prefer concise output.",
        ))
        .build()
        .expect("document should update");

    assert_eq!(updated.list_entries(KnowledgeEntryQuery::all()).len(), 2);
    assert!(updated
        .prompt_view()
        .expect("prompt view should render")
        .contains("Session model exclusively owns writes."));

    let removed = KnowledgeDocument::builder(&profile)
        .from_markdown(updated.render().expect("document should render"))
        .remove_entry("memory-ownership")
        .build()
        .expect("entry should be removed");
    assert!(removed.get_entry("memory-ownership").is_none());

    for error in [
        KnowledgeDocument::builder(&profile)
            .from_markdown(removed.render().expect("document should render"))
            .update_entry(
                "Invalid Key",
                KnowledgeEntryUpdate::builder().build().unwrap(),
            )
            .build()
            .expect_err("invalid update key should fail"),
        KnowledgeDocument::builder(&profile)
            .from_markdown(removed.render().expect("document should render"))
            .remove_entry("Invalid Key")
            .build()
            .expect_err("invalid remove key should fail"),
    ] {
        assert_eq!(error.kind(), KnowledgeFormatErrorKind::InvalidEntry);
    }
}

#[test]
fn managed_document_rejects_duplicate_and_reserved_marker_content() {
    let profile = profile();
    let duplicate = KnowledgeDocument::builder(&profile)
        .create_document()
        .create_entry(entry("same-key", "key-knowledge", "First."))
        .create_entry(entry("same-key", "key-knowledge", "Second."))
        .build()
        .expect_err("duplicate key should fail");
    assert_eq!(duplicate.kind(), KnowledgeFormatErrorKind::AlreadyExists);

    let invalid = KnowledgeEntry::builder()
        .key("unsafe")
        .section("key-knowledge")
        .title("Unsafe")
        .body("<!-- talos:user-workspace-memory:end -->")
        .build()
        .expect("generic entry is valid before applying a profile");
    let error = KnowledgeDocument::builder(&profile)
        .create_document()
        .create_entry(invalid)
        .build()
        .expect_err("reserved marker should fail");
    assert_eq!(error.kind(), KnowledgeFormatErrorKind::InvalidEntry);
}

#[test]
fn managed_document_requires_the_exact_profile_frontmatter() {
    let profile = profile();
    let canonical = KnowledgeDocument::builder(&profile)
        .create_document()
        .build()
        .expect("template should build")
        .render()
        .expect("template should render");

    for invalid in [
        canonical.replace("title: User Workspace Memory", "title: Different Memory"),
        canonical.replace("status: stable", "status: draft"),
        canonical.replace(
            "description: Durable cross-session memory.",
            "description: Different description.",
        ),
    ] {
        let error = KnowledgeDocument::builder(&profile)
            .from_markdown(invalid)
            .build()
            .expect_err("profile drift should fail");
        assert_eq!(error.kind(), KnowledgeFormatErrorKind::InvalidDocument);
    }

    let mut invalid_profile = profile.clone();
    invalid_profile.region_namespace = "invalid namespace".to_string();
    let error = KnowledgeDocument::builder(&invalid_profile)
        .from_markdown(canonical)
        .build()
        .expect_err("document builder should revalidate its profile");
    assert_eq!(error.kind(), KnowledgeFormatErrorKind::InvalidProfile);
}

#[test]
fn managed_document_requires_complete_ordered_sections_and_no_stray_content() {
    let profile = profile();
    let canonical = KnowledgeDocument::builder(&profile)
        .create_document()
        .build()
        .expect("template should build")
        .render()
        .expect("template should render");

    let missing = canonical.replace("## Key Knowledge\n\n", "");
    let reordered = canonical.replace(
        "## Interaction Guidance\n\n## Key Knowledge",
        "## Key Knowledge\n\n## Interaction Guidance",
    );
    let duplicate = canonical.replace(
        "## Key Knowledge",
        "## Interaction Guidance\n\n## Key Knowledge",
    );
    let stray = canonical.replace(
        "## Key Knowledge\n\n",
        "## Key Knowledge\n\nfree-form text\n\n",
    );

    for invalid in [missing, reordered, duplicate, stray] {
        let error = KnowledgeDocument::builder(&profile)
            .from_markdown(invalid)
            .build()
            .expect_err("invalid managed region should fail");
        assert_eq!(error.kind(), KnowledgeFormatErrorKind::InvalidDocument);
    }
}

#[test]
fn managed_document_reorders_upsert_and_replace_after_section_moves() {
    let profile = profile();
    let source = KnowledgeDocument::builder(&profile)
        .create_document()
        .create_entry(entry("first", "interaction-guidance", "First."))
        .create_entry(entry("second", "key-knowledge", "Second."))
        .build()
        .expect("source should build")
        .render()
        .expect("source should render");

    let upserted = KnowledgeDocument::builder(&profile)
        .from_markdown(source.clone())
        .upsert_entry(entry("first", "active-context", "Moved."))
        .build()
        .expect("upsert should move the entry");
    assert_eq!(
        upserted
            .list_entries(KnowledgeEntryQuery::all())
            .into_iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "first"]
    );

    let replaced = KnowledgeDocument::builder(&profile)
        .from_markdown(source)
        .replace_entry(entry("first", "active-context", "Moved."))
        .build()
        .expect("replace should move the entry");
    assert_eq!(
        replaced
            .list_entries(KnowledgeEntryQuery::all())
            .into_iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "first"]
    );
}

#[test]
fn okf_attested_computation_exposes_complete_contract() {
    let source = "---\ntype: Attested Computation\nruntime: python3\nparameters:\n  - name: limit\n    type: integer\n    required: true\nexecutor:\n  resource: references/run.py\n  receipt: [stdout, exit_code]\nattester:\n  resource: references/verify.py\n---\n\n# Computation\n\n```python\nprint(limit)\n```\n";

    let computation = OkfDocument::parse(source)
        .expect("computation should parse")
        .attested_computation()
        .expect("contract should exist");

    assert_eq!(computation.runtime.as_deref(), Some("python3"));
    assert_eq!(computation.parameters.len(), 1);
    assert_eq!(
        computation.executor.expect("executor should exist").receipt,
        vec!["stdout", "exit_code"]
    );
    assert!(matches!(
        computation.computation,
        OkfComputationSource::Inline { .. }
    ));
}

#[test]
fn typed_frontmatter_builder_covers_standard_v02_families() {
    let generated_at =
        OkfDateTime::parse("2026-06-20T22:53:05Z").expect("generated timestamp should parse");
    let verified_at =
        OkfDateTime::parse("2026-06-25T09:00:00Z").expect("verification timestamp should parse");
    let stale_after =
        OkfDateTime::parse("2026-12-31T00:00:00Z").expect("staleness timestamp should parse");
    let source = OkfSource::builder("https://wiki.acme/finance/revenue-recognition")
        .id("rev-policy")
        .title("Revenue recognition policy")
        .author(OkfActor::parse("team:finance-fpa"))
        .last_modified(generated_at)
        .usage_count(5000)
        .usage_window(OkfUsageWindow::new(generated_at, verified_at))
        .build();
    let frontmatter = OkfFrontmatter::builder()
        .concept_type("Attested Computation")
        .title("Revenue for fiscal year")
        .description("Recognized revenue for a fiscal year.")
        .tags(["finance", "revenue"])
        .status(OkfStatus::Stable)
        .generated(OkfGenerated::new(
            OkfActor::parse("reference_agent/gemini-2.5-pro"),
            generated_at,
        ))
        .verifications([OkfVerification::new(
            OkfActor::parse("human:reviewer"),
            verified_at,
        )])
        .stale_after(stale_after)
        .sources([source])
        .usage_window(OkfUsageWindow::new(generated_at, verified_at))
        .computation_runtime("bigquery")
        .computation_parameters([OkfParameter::new("year", "integer", true)])
        .executor(OkfExecutor::new(
            "references/skills/run-on-bq.md",
            ["job_id", "executed_sql", "result"],
        ))
        .attester(OkfAttester::new("references/attesters/sql-equality.py"))
        .build()
        .expect("typed frontmatter should build");
    let document = OkfDocument::builder()
        .frontmatter(frontmatter)
        .inline_computation(OkfInlineComputation::new("SELECT @year").language("sql"))
        .build()
        .expect("typed document should build");
    let parsed = OkfDocument::parse(&document.render()).expect("document should round-trip");

    assert_eq!(parsed.sources().len(), 1);
    assert_eq!(parsed.verifications().len(), 1);
    assert_eq!(parsed.trust_tier(), OkfTrustTier::HumanReviewed);
    assert_eq!(
        parsed
            .attested_computation()
            .expect("computation should exist")
            .parameters,
        vec![OkfParameter::new("year", "integer", true)]
    );
}

#[test]
fn appendix_a_concept_matches_the_official_validator_contract() {
    const APPENDIX_A_CONCEPT: &str = r"---
type: Metric
title: Income statement (fiscal year)
description: Headline income-statement figures for a fiscal year.
tags: [finance, income-statement]
status: stable
generated: { by: reference_agent/gemini-2.5-pro, at: 2026-06-20T22:53:05Z }
verified: { by: human:ahormati, at: 2026-06-25T09:00:00Z }
stale_after: 2026-12-31T00:00:00Z
sources:
  - id: fpa-handbook
    resource: https://wiki.acme/finance/fpa-handbook
    title: FP&A reporting handbook
---

# Definition

The income statement follows the handbook.[^fpa-handbook]

[^fpa-handbook]: FP&A reporting handbook
";

    let talos_bundle = OkfBundle::builder()
        .file(OkfBundleFile::new(
            "income-statement.md",
            APPENDIX_A_CONCEPT,
        ))
        .build()
        .expect("Talos bundle should build");
    let directory = tempfile::tempdir().expect("temporary bundle should build");
    std::fs::write(
        directory.path().join("income-statement.md"),
        APPENDIX_A_CONCEPT,
    )
    .expect("reference concept should write");
    let reference_bundle =
        okf_core::Bundle::load(directory.path()).expect("reference bundle should load");
    let reference = okf_validator::validate_bundle(&reference_bundle);
    let talos = talos_bundle.validate();

    assert_eq!(talos.is_conformant(), reference.is_conformant());
    assert_eq!(
        talos
            .diagnostics
            .iter()
            .filter(|item| item.severity == OkfDiagnosticSeverity::Error)
            .count(),
        reference.error_count()
    );
    assert_eq!(
        talos
            .diagnostics
            .iter()
            .filter(|item| item.severity == OkfDiagnosticSeverity::Warning)
            .count(),
        reference.warning_count()
    );
}

#[test]
fn malformed_standard_families_match_the_official_validator_diagnostics() {
    const MALFORMED_CONCEPT: &str = r"---
type: Attested Computation
title: Broken
description: Every family mis-specified.
tags: finance, revenue
status: experimental
stale_after: soon
generated: { at: 2026-06-20T22:53:05Z }
verified: [{ by: human:ahormati, at: yesterday }]
sources:
  - { id: dup, resource: https://a }
  - { id: dup, title: no resource }
---

A claim.[^unknown]

[^unknown]: Not a source id
";

    let talos = OkfBundle::builder()
        .file(OkfBundleFile::new("broken.md", MALFORMED_CONCEPT))
        .build()
        .expect("Talos bundle should build")
        .validate();
    let directory = tempfile::tempdir().expect("temporary bundle should build");
    std::fs::write(directory.path().join("broken.md"), MALFORMED_CONCEPT)
        .expect("reference concept should write");
    let reference_bundle =
        okf_core::Bundle::load(directory.path()).expect("reference bundle should load");
    let reference = okf_validator::validate_bundle(&reference_bundle);

    let talos_warnings: Vec<_> = talos
        .diagnostics
        .iter()
        .filter(|item| item.severity == OkfDiagnosticSeverity::Warning)
        .map(|item| (item.message.clone(), item.fixable))
        .collect();
    let reference_warnings: Vec<_> = reference
        .diagnostics
        .iter()
        .filter(|item| item.severity == okf_validator::Severity::Warning)
        .map(|item| (item.message.clone(), item.fixable))
        .collect();

    assert!(talos.is_conformant());
    assert_eq!(talos_warnings, reference_warnings);
}

#[test]
fn okf_index_log_and_v01_fallback_remain_consumable() {
    let concepts = OkfBundle::builder()
        .file(OkfBundleFile::new(
            "guide.md",
            "---\ntype: Guide\ntitle: Guide\ntimestamp: 2026-01-01T00:00:00Z\n---\n\n# Citations\n\n1. [Reference](https://example.com)\n",
        ))
        .build()
        .expect("v0.1-compatible bundle should build");
    let index = OkfIndex::generate(concepts.concepts());
    let rendered_index = index.render();
    assert!(rendered_index.starts_with("---\nokf_version: \"0.2\"\n---"));
    assert!(rendered_index.contains("# Guide"));
    assert_eq!(
        OkfIndex::parse(&rendered_index)
            .expect("index should parse")
            .entries
            .len(),
        1
    );

    let mut log = OkfLog::builder()
        .title("Changes")
        .build()
        .expect("log should build");
    assert!(log
        .append_entry(
            OkfDate::parse("2026-08-27").expect("log date"),
            OkfLogEntry::new(Some("Update".to_string()), "Added memory format.")
                .expect("log entry"),
        )
        .expect("log append"));
    assert!(!log
        .append_entry(
            OkfDate::parse("2026-08-27").expect("log date"),
            OkfLogEntry::new(Some("Update".to_string()), "Added memory format.")
                .expect("log entry"),
        )
        .expect("duplicate log append"));
    let rendered_log = log.render();
    let log = OkfLog::parse_strict(&rendered_log).expect("rendered log should parse strictly");
    assert_eq!(log.days.len(), 1);
    assert!(rendered_log.contains("Added memory format"));
    let produced_bundle = OkfBundle::builder()
        .file(OkfBundleFile::new(
            "guide.md",
            "---\ntype: Guide\ntitle: Guide\ntimestamp: 2026-01-01T00:00:00Z\n---\n\n# Citations\n\n1. [Reference](https://example.com)\n",
        ))
        .file(OkfBundleFile::new("index.md", rendered_index))
        .file(OkfBundleFile::new("log.md", rendered_log))
        .build()
        .expect("produced bundle should build");
    assert!(produced_bundle.validate().is_conformant());
}
