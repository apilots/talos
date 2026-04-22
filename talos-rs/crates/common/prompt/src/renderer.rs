use crate::{
    PromptAssembly, PromptBlock, PromptBlockCacheScope, PromptBlockKind, PromptSection,
    RegionEnvelopePolicy, SectionCacheScope,
};
use talos_llm_provider::{CacheControl, CacheControlKind, ChatMessage};

fn cache_control_from_scope(scope: PromptBlockCacheScope) -> Option<CacheControl> {
    match scope {
        PromptBlockCacheScope::StablePrefix | PromptBlockCacheScope::ScopedReusable => {
            Some(CacheControl {
                kind: CacheControlKind::Ephemeral,
            })
        }
        PromptBlockCacheScope::NoCache => None,
    }
}

fn block_cache_scope_for_sections(sections: &[PromptSection]) -> PromptBlockCacheScope {
    if sections.is_empty() {
        return PromptBlockCacheScope::NoCache;
    }

    if sections.iter().all(|section| {
        matches!(
            section.cache_scope(),
            SectionCacheScope::StaticPrefixEligible
        )
    }) {
        PromptBlockCacheScope::StablePrefix
    } else if sections
        .iter()
        .all(|section| matches!(section.cache_scope(), SectionCacheScope::WorkspaceStable))
    {
        PromptBlockCacheScope::ScopedReusable
    } else {
        PromptBlockCacheScope::NoCache
    }
}

fn block_cache_scope_for_section(section: &PromptSection) -> PromptBlockCacheScope {
    match section.cache_scope() {
        SectionCacheScope::StaticPrefixEligible => PromptBlockCacheScope::StablePrefix,
        SectionCacheScope::WorkspaceStable => PromptBlockCacheScope::ScopedReusable,
        SectionCacheScope::DynamicOnly => PromptBlockCacheScope::NoCache,
    }
}

pub(crate) fn render_plain_section(section: &PromptSection) -> String {
    let mut parts = vec![
        section.title().trim().to_string(),
        section.framing_prompt().trim().to_string(),
    ];
    if !section.payload.trim().is_empty() {
        parts.push(section.payload.trim().to_string());
    }
    parts.join("\n\n")
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_xml_section(section: &PromptSection) -> String {
    format!(
        "<prompt-region>\n<region-name>{}</region-name>\n<region-priority>{}</region-priority>\n<region-role>{}</region-role>\n{}\n<region-body>\n{}\n</region-body>\n</prompt-region>",
        escape_xml_text(section.stable_name()),
        escape_xml_text(section.priority().as_str()),
        escape_xml_text(section.region_role()),
        escape_xml_text(section.framing_prompt().trim()),
        escape_xml_text(section.payload.trim())
    )
}

pub(crate) fn render_section_text(section: &PromptSection) -> String {
    match section.envelope_policy() {
        RegionEnvelopePolicy::PlainTextRequired => render_plain_section(section),
        RegionEnvelopePolicy::XmlEnvelopeRequired => render_xml_section(section),
    }
}

pub(crate) fn render_prompt_blocks(assembly: &PromptAssembly) -> Vec<PromptBlock> {
    let mut blocks = Vec::new();

    push_section_blocks(
        &mut blocks,
        PromptBlockKind::StaticPrefix,
        &assembly.static_prefix_sections,
    );

    if !assembly.dynamic_suffix_sections.is_empty() {
        blocks.push(PromptBlock {
            kind: PromptBlockKind::DynamicSuffix,
            text: assembly
                .dynamic_suffix_sections
                .iter()
                .map(render_section_text)
                .collect::<Vec<_>>()
                .join("\n\n"),
            cache_scope: block_cache_scope_for_sections(&assembly.dynamic_suffix_sections),
        });
    }

    if let Some(text) = assembly
        .request_context_injection
        .appended_system_context
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        blocks.push(PromptBlock {
            kind: PromptBlockKind::AppendedSystemContext,
            text: text.clone(),
            cache_scope: PromptBlockCacheScope::NoCache,
        });
    }

    if let Some(text) = assembly
        .request_context_injection
        .prepended_user_context_message
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        blocks.push(PromptBlock {
            kind: PromptBlockKind::PrependedUserContextMessage,
            text: text.clone(),
            cache_scope: PromptBlockCacheScope::NoCache,
        });
    }

    blocks.push(PromptBlock {
        kind: PromptBlockKind::UserInput,
        text: assembly.user_input.clone(),
        cache_scope: PromptBlockCacheScope::NoCache,
    });

    blocks
}

fn push_section_blocks(
    blocks: &mut Vec<PromptBlock>,
    kind: PromptBlockKind,
    sections: &[PromptSection],
) {
    if sections.is_empty() {
        return;
    }

    let mut current_scope = block_cache_scope_for_section(&sections[0]);
    let mut current_sections = Vec::new();

    for section in sections {
        let next_scope = block_cache_scope_for_section(section);
        if !current_sections.is_empty() && next_scope != current_scope {
            blocks.push(PromptBlock {
                kind,
                text: current_sections.join("\n\n"),
                cache_scope: current_scope,
            });
            current_sections.clear();
            current_scope = next_scope;
        }
        current_sections.push(render_section_text(section));
    }

    if !current_sections.is_empty() {
        blocks.push(PromptBlock {
            kind,
            text: current_sections.join("\n\n"),
            cache_scope: current_scope,
        });
    }
}

pub(crate) fn render_chat_messages(blocks: &[PromptBlock]) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(blocks.len());
    for block in blocks {
        let mut message = match block.kind {
            PromptBlockKind::PrependedUserContextMessage | PromptBlockKind::UserInput => {
                ChatMessage::user(block.text.clone())
            }
            PromptBlockKind::Attribution
            | PromptBlockKind::StaticPrefix
            | PromptBlockKind::DynamicSuffix
            | PromptBlockKind::AppendedSystemContext => ChatMessage::system(block.text.clone()),
        };
        message.cache_control = cache_control_from_scope(block.cache_scope);
        messages.push(message);
    }
    messages
}

pub(crate) fn render_assembly(assembly: &PromptAssembly) -> Vec<ChatMessage> {
    let blocks = render_prompt_blocks(assembly);
    render_chat_messages(&blocks)
}
