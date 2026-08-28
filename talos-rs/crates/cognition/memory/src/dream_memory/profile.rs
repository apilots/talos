use talos_knowledge_format::{
    KnowledgeDocumentProfile, KnowledgeFormatResult, OkfFrontmatter, OkfGenerated, OkfStatus,
};

use super::DreamMemoryDate;

const DREAM_MEMORY_NAMESPACE: &str = "talos:dream-memory";

pub(super) fn dream_memory_profile(
    date: DreamMemoryDate,
    generated: OkfGenerated,
) -> KnowledgeFormatResult<KnowledgeDocumentProfile> {
    let title = format!("Dream Memory {date}");
    let frontmatter = OkfFrontmatter::builder()
        .concept_type("DreamMemory")
        .title(title)
        .description(format!("Durable memories formed for {date}."))
        .tags(["talos", "dream-memory"])
        .status(OkfStatus::Stable)
        .generated(generated)
        .build()?;
    KnowledgeDocumentProfile::builder()
        .frontmatter(frontmatter)
        .region_namespace(DREAM_MEMORY_NAMESPACE)
        .section("memories", "Memories")
        .max_chars(usize::MAX)
        .max_entries(usize::MAX)
        .build()
}
