use talos_sdk::TalosContentItem;

use crate::render::tool::{helpers, tool_title};
use crate::types::{ChatBlock, ToolRenderData};

pub(crate) fn build_reference_render_data(
    block: &ChatBlock,
    items: &[TalosContentItem],
) -> ToolRenderData {
    let mut output_lines = Vec::new();
    for item in items {
        match item {
            TalosContentItem::WebReference {
                url,
                title,
                snippet,
                source_domain,
                ..
            } => {
                output_lines.push(title.clone().unwrap_or_else(|| url.clone()));
                output_lines.push(format!("  {}", source_domain.as_deref().unwrap_or(url)));
                if let Some(snippet) = snippet {
                    output_lines.push(format!("  {snippet}"));
                }
            }
            TalosContentItem::ArtifactRef {
                artifact_id,
                mime_type,
                label,
            } => {
                output_lines.push(format!(
                    "artifact: {}{}",
                    label.as_deref().unwrap_or(artifact_id),
                    mime_type
                        .as_ref()
                        .map(|mime| format!(" · {mime}"))
                        .unwrap_or_default()
                ));
            }
            TalosContentItem::McpResourceRef {
                server,
                uri,
                mime_type,
            } => {
                output_lines.push(format!(
                    "mcp: {server}:{uri}{}",
                    mime_type
                        .as_ref()
                        .map(|mime| format!(" · {mime}"))
                        .unwrap_or_default()
                ));
            }
            TalosContentItem::Text { text } => {
                output_lines.extend(text.lines().map(str::to_string))
            }
            _ => {}
        }
    }
    let count = output_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .count();
    let summary = helpers::status_summary(block, |_| format!("{count} references"));
    let preview_lines = output_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .cloned()
        .collect();

    ToolRenderData {
        title: tool_title(block, block.tool_name.as_deref().unwrap_or("Reference")),
        summary,
        detail_lines: Vec::new(),
        preview_lines,
        output_lines,
        force_show_output: false,
    }
}
