pub mod admin;
pub mod catalog;
pub mod loader;
pub mod model;
pub mod prompt;
pub mod state;

pub use admin::{
    disable_skill, enable_skill, install_skill, uninstall_skill, SkillAdminContext,
    SkillInstallSource,
};
pub use catalog::{read_document, refresh, resolve, suggest};
pub use model::{
    SkillDocument, SkillLoadError, SkillMetadata, SkillRoot, SkillRoots, SkillScope, SkillSelector,
    SkillSnapshot, SkillSource, SkillSuggestion, SkillSummary, SkillWarning,
};
pub use prompt::{
    LocalSkillProvider, SkillPromptBudget, SkillPromptContext, SkillPromptInjection,
    SkillPromptRequest, SkillPromptWarning, SkillProvider,
};
pub use state::{load_skill_state, save_skill_state, SkillState};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_skill(root: &Path, name: &str, body: &str) {
        let skill_root = root.join(name);
        fs::create_dir_all(&skill_root).expect("skill dir should be created");
        fs::write(skill_root.join("SKILL.md"), body).expect("skill file should be written");
    }

    fn test_roots(temp: &TempDir) -> SkillRoots {
        let user_root = temp.path().join(".talos").join("skills");
        let external_root = temp.path().join(".agents").join("skills");
        SkillRoots {
            roots: vec![
                SkillRoot::user(&user_root),
                SkillRoot::external(&external_root),
            ],
            state_path: temp.path().join(".talos").join("skills-state.json"),
            install_root: user_root,
            max_skill_md_bytes: 64 * 1024,
            max_available_summary_bytes: 8 * 1024,
        }
    }

    #[test]
    fn loader_scans_user_and_external_roots_with_loose_frontmatter() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        write_skill(
            &roots.roots[0].path,
            "review",
            r#"---
name: review
description: Review code
metadata:
  author: tester
argument-hint: diff
---

Review the diff.
"#,
        );
        write_skill(
            &roots.roots[1].path,
            "docs",
            r#"---
name: docs
description: Write docs
unknown-field: ok
---

Write docs.
"#,
        );

        let snapshot = refresh(&roots).expect("skills should load");

        assert_eq!(snapshot.errors, Vec::new());
        assert_eq!(snapshot.skills.len(), 2);
        assert_eq!(snapshot.skills[0].scope, SkillScope::User);
        assert_eq!(snapshot.skills[1].scope, SkillScope::External);
        assert!(snapshot.skills[1].read_only);
        assert_eq!(
            snapshot.skills[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.author.as_deref()),
            Some("tester")
        );
        assert_eq!(
            snapshot.skills[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.argument_hint.as_deref()),
            Some("diff")
        );
    }

    #[test]
    fn loader_reports_invalid_yaml_and_missing_required_fields() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        write_skill(
            &roots.roots[0].path,
            "broken",
            r#"---
name: broken
metadata: [
---
Broken.
"#,
        );
        write_skill(
            &roots.roots[0].path,
            "missing-description",
            r#"---
name: missing-description
---
Broken.
"#,
        );

        let snapshot = refresh(&roots).expect("scan should not fail");

        assert_eq!(snapshot.skills.len(), 0);
        assert_eq!(snapshot.errors.len(), 2);
    }

    #[test]
    fn external_metadata_reads_talos_yaml_without_enforcing_dependencies() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        write_skill(
            &roots.roots[1].path,
            "docs",
            r#"---
name: docs
description: Write docs
---
Write docs.
"#,
        );
        let metadata_dir = roots.roots[1].path.join("docs").join("agents");
        fs::create_dir_all(&metadata_dir).expect("metadata dir should be created");
        fs::write(
            metadata_dir.join("talos.yaml"),
            r#"display_name: Documentation Helper
short_description: Draft concise docs
icon: book-open
dependencies:
  - should-not-install
unknown_field: ignored
"#,
        )
        .expect("host metadata should be written");

        let snapshot = refresh(&roots).expect("skills should load");

        assert_eq!(snapshot.errors, Vec::new());
        assert_eq!(snapshot.skills.len(), 1);
        assert_eq!(
            snapshot.skills[0].short_description.as_deref(),
            Some("Draft concise docs")
        );
        let metadata = snapshot.skills[0]
            .metadata
            .as_ref()
            .expect("metadata should be present");
        assert_eq!(
            metadata.display_name.as_deref(),
            Some("Documentation Helper")
        );
        assert_eq!(metadata.icon.as_deref(), Some("book-open"));

        write_skill(
            &roots.roots[0].path,
            "broken-host-metadata",
            r#"---
name: broken-host-metadata
description: Broken host metadata still loads
---
Still valid.
"#,
        );
        let broken_metadata_dir = roots.roots[0]
            .path
            .join("broken-host-metadata")
            .join("agents");
        fs::create_dir_all(&broken_metadata_dir).expect("metadata dir should be created");
        fs::write(broken_metadata_dir.join("talos.yaml"), "display_name: [")
            .expect("broken host metadata should be written");

        let snapshot = refresh(&roots).expect("skills should load with malformed host metadata");

        assert_eq!(snapshot.errors, Vec::new());
        assert!(snapshot
            .skills
            .iter()
            .any(|skill| skill.name == "broken-host-metadata"));
    }

    #[test]
    fn prompt_injection_contains_root_and_path_without_reading_bundled_resources() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        write_skill(
            &roots.roots[0].path,
            "review",
            r#"---
name: review
description: Review code
---

Read references/checklist.md only if needed.
"#,
        );
        let bundled = roots.roots[0].path.join("review").join("references");
        fs::create_dir_all(&bundled).expect("references dir should be created");
        fs::write(bundled.join("checklist.md"), "do not preload")
            .expect("resource should be written");
        let provider = LocalSkillProvider::new(roots.clone());

        let context = provider
            .prompt_context(SkillPromptRequest {
                text: "Use $review".to_string(),
                selected: Vec::new(),
                budget: SkillPromptBudget::default(),
            })
            .expect("prompt context should build");

        assert_eq!(context.injections.len(), 1);
        assert_eq!(
            context.injections[0].root,
            roots.roots[0].path.join("review")
        );
        assert_eq!(
            context.injections[0].path,
            roots.roots[0].path.join("review").join("SKILL.md")
        );
        assert!(!context.injections[0].contents.contains("do not preload"));
    }

    #[test]
    fn structured_selection_blocks_duplicate_plain_name_fallback() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        write_skill(
            &roots.roots[0].path,
            "docs-user",
            r#"---
name: docs
description: User docs
---
User docs skill.
"#,
        );
        write_skill(
            &roots.roots[1].path,
            "docs-external",
            r#"---
name: docs
description: External docs
---
External docs skill.
"#,
        );
        let selected_path = roots.roots[1].path.join("docs-external").join("SKILL.md");
        let provider = LocalSkillProvider::new(roots);

        let context = provider
            .prompt_context(SkillPromptRequest {
                text: "Use $docs".to_string(),
                selected: vec![SkillSelector::selection("docs", selected_path.clone())],
                budget: SkillPromptBudget::default(),
            })
            .expect("prompt context should build");

        assert_eq!(context.injections.len(), 1);
        assert_eq!(context.injections[0].path, selected_path);
        assert!(context.injections[0].contents.contains("External docs"));
    }

    #[test]
    fn mention_extraction_stops_at_sentence_punctuation() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        write_skill(
            &roots.roots[0].path,
            "e2e-sentinel",
            r#"---
name: e2e-sentinel
description: E2E sentinel
---
Sentinel skill.
"#,
        );
        let provider = LocalSkillProvider::new(roots);

        let context = provider
            .prompt_context(SkillPromptRequest {
                text: "Use $e2e-sentinel.".to_string(),
                selected: Vec::new(),
                budget: SkillPromptBudget::default(),
            })
            .expect("prompt context should build");

        assert_eq!(context.injections.len(), 1);
        assert_eq!(context.injections[0].name, "e2e-sentinel");
        assert_eq!(context.warnings, Vec::new());
    }

    #[test]
    fn install_copies_whole_directory_and_excludes_dependency_caches() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        let source = temp.path().join("source-review");
        fs::create_dir_all(source.join("templates")).expect("templates dir should be created");
        fs::create_dir_all(source.join("node_modules"))
            .expect("node_modules dir should be created");
        fs::write(
            source.join("SKILL.md"),
            r#"---
name: review
description: Review code
---
Use templates/base.md.
"#,
        )
        .expect("skill should be written");
        fs::write(source.join("templates").join("base.md"), "template")
            .expect("template should be written");
        fs::write(source.join("node_modules").join("ignored.txt"), "ignored")
            .expect("ignored cache should be written");
        let ctx = SkillAdminContext::new(roots.clone());

        let installed = install_skill(&ctx, SkillInstallSource::LocalPath(source))
            .expect("skill should install");

        assert_eq!(installed.scope, SkillScope::User);
        assert!(roots
            .install_root
            .join("review")
            .join("templates")
            .join("base.md")
            .exists());
        assert!(!roots
            .install_root
            .join("review")
            .join("node_modules")
            .join("ignored.txt")
            .exists());
    }

    #[test]
    fn install_rejects_name_that_already_exists_in_external_root() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        write_skill(
            &roots.roots[1].path,
            "external-review",
            r#"---
name: review
description: External review
---
External review skill.
"#,
        );
        let source = temp.path().join("source-review");
        fs::create_dir_all(&source).expect("source skill dir should be created");
        fs::write(
            source.join("SKILL.md"),
            r#"---
name: review
description: User review
---
User review skill.
"#,
        )
        .expect("source skill should be written");
        let ctx = SkillAdminContext::new(roots.clone());

        let error = install_skill(&ctx, SkillInstallSource::LocalPath(source))
            .expect_err("same-name external skill should block install");

        assert!(matches!(error, talos_error::TalosError::Conflict(_)));
        assert!(!roots.install_root.join("review").exists());
    }

    #[test]
    fn external_skill_cannot_be_uninstalled_but_can_be_disabled() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let roots = test_roots(&temp);
        write_skill(
            &roots.roots[1].path,
            "docs",
            r#"---
name: docs
description: Write docs
---
Write docs.
"#,
        );
        let ctx = SkillAdminContext::new(roots.clone());

        let error = uninstall_skill(&ctx, &SkillSelector::by_name("docs"))
            .expect_err("external skill should not uninstall");
        assert!(error.to_string().contains("Authorization denied"));

        let disabled = disable_skill(&ctx, &SkillSelector::by_name("docs"))
            .expect("external skill can be disabled");
        assert!(!disabled.enabled);
    }
}
