//! Markdown copy projection for finalized tool transcript blocks.

use talos_sdk::TalosContentItem;

use crate::utils::display_tool_name;

pub(crate) fn tool_copy_markdown(
    name: &str,
    arguments: &str,
    result_content: &[TalosContentItem],
) -> String {
    let mut lines = vec![format!("### Tool `{}`", display_tool_name(name))];
    if let Some(input) = formatted_arguments(arguments) {
        lines.push(String::new());
        lines.push("Input:".to_string());
        lines.push("```json".to_string());
        lines.push(input);
        lines.push("```".to_string());
    }

    let output = result_content
        .iter()
        .map(TalosContentItem::display_text)
        .collect::<Vec<_>>()
        .join("\n");
    if !output.trim().is_empty() {
        lines.push(String::new());
        lines.push("Output:".to_string());
        if serde_json::from_str::<serde_json::Value>(&output).is_ok() {
            lines.push("```json".to_string());
        } else {
            lines.push("```text".to_string());
        }
        lines.push(output);
        lines.push("```".to_string());
    }

    lines.join("\n")
}

fn formatted_arguments(arguments: &str) -> Option<String> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Null) => None,
        Ok(serde_json::Value::Object(map)) if map.is_empty() => None,
        Ok(value) => Some(serde_json::to_string_pretty(&value).unwrap_or_else(|_| trimmed.into())),
        Err(_) => Some(serde_json::to_string_pretty(&trimmed).unwrap_or_else(|_| trimmed.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::tool_copy_markdown;
    use talos_sdk::TalosContentItem;

    #[test]
    fn includes_tool_input_and_output_as_markdown() {
        let markdown = tool_copy_markdown(
            "read",
            r#"{"file_path":"/tmp/demo.py"}"#,
            &[TalosContentItem::Text {
                text: "L1: print('hi')".to_string(),
            }],
        );

        assert!(markdown.contains("### Tool `Read`"));
        assert!(markdown.contains("\"file_path\": \"/tmp/demo.py\""));
        assert!(markdown.contains("Output:\n```text\nL1: print('hi')\n```"));
    }
}
