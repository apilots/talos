use talos_knowledge_format::{
    KnowledgeDocumentProfile, KnowledgeFormatResult, OkfFrontmatter, OkfGenerated, OkfStatus,
};
use talos_types::MEMORY_FILE_MAX_CHARS;

/// Stable marker namespace for the model-maintained region.
pub const USER_WORKSPACE_MEMORY_NAMESPACE: &str = "talos:user-workspace-memory";

/// Builds the canonical User Workspace Memory managed document profile.
pub fn user_workspace_memory_profile(
    generated: OkfGenerated,
) -> KnowledgeFormatResult<KnowledgeDocumentProfile> {
    let frontmatter = OkfFrontmatter::builder()
        .concept_type("UserWorkspaceMemory")
        .title("User Workspace Memory")
        .description("Durable cross-session memory scoped to one agent and one user.")
        .tags(["talos", "agent", "user-memory"])
        .status(OkfStatus::Stable)
        .generated(generated)
        .build()?;
    KnowledgeDocumentProfile::builder()
        .frontmatter(frontmatter)
        .region_namespace(USER_WORKSPACE_MEMORY_NAMESPACE)
        .guidance(
            "Keep only information that remains useful across Sessions.\n\
             Record conclusions instead of chronological activity.\n\
             Update or remove stale entries instead of accumulating history.",
        )
        .section_with_description(
            "interaction-guidance",
            "Interaction Guidance",
            "Model-learned collaboration preferences and confirmed feedback.",
        )
        .section_with_description(
            "key-knowledge",
            "Key Knowledge",
            "Durable facts, decisions, constraints, lessons, and references.",
        )
        .section_with_description(
            "active-context",
            "Active Context",
            "Cross-session goals, commitments, blockers, ownership, and next steps.",
        )
        .max_chars(MEMORY_FILE_MAX_CHARS)
        .max_entries(128)
        .build()
}
