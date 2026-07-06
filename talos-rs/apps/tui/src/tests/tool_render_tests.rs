use super::*;
use talos_types::talos::{
    TalosToolDiffHunk, TalosToolDiffLine, TalosToolFileDiff, TalosToolSearchMatch,
    TalosToolTaskStatus,
};

#[test]
fn test_read_and_grep_render_as_separate_transcript_blocks() {
    let app = create_embedded_app();
    let mut read_block = ChatBlock::new_tool_call(
        "read-1".to_string(),
        "Read".to_string(),
        json!({"file_path":"talos-rs/apps/tui/src/render/tool_lines.rs"}),
        1,
        0,
        None,
    );
    read_block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    read_block.tool_content_items = vec![TalosContentItem::FileRead {
        path: "talos-rs/apps/tui/src/render/tool_lines.rs".to_string(),
        offset: 0,
        limit: 2,
        total_lines: 2,
        content: "use ratatui::text::Line;\nfn render() {}".to_string(),
        truncated: false,
    }];

    let mut grep_block = ChatBlock::new_tool_call(
        "grep-1".to_string(),
        "Grep".to_string(),
        json!({"pattern":"ToolRenderData"}),
        1,
        1,
        None,
    );
    grep_block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    grep_block.tool_content_items = vec![TalosContentItem::SearchResults {
        query: Some("ToolRenderData".to_string()),
        matches: vec![TalosToolSearchMatch {
            path: "talos-rs/apps/tui/src/types.rs".to_string(),
            line: 224,
            column: None,
            text: "pub(crate) struct ToolRenderData {".to_string(),
            before: Vec::new(),
            after: Vec::new(),
        }],
        offset: 0,
        limit: 20,
        total_matches: Some(1),
        more_available: false,
        partial: false,
    }];

    let read_text = app
        .tool_block_lines(&read_block, 120)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    let grep_text = app
        .tool_block_lines(&grep_block, 120)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(
        read_text,
        vec![
            "● Read(talos-rs/apps/tui/src/render/tool_lines.rs)",
            "  ⎿ Read 2 lines (ctrl+o to expand)",
        ]
    );
    assert_eq!(
        grep_text,
        vec![
            "● Grep(ToolRenderData)",
            "  ⎿ Found 1 matches (ctrl+o to expand)",
        ]
    );
}

#[test]
fn test_edit_diff_renders_full_file_changes() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new_tool_call(
        "edit-1".to_string(),
        "Edit".to_string(),
        json!({"file_path":"src/account.ts"}),
        1,
        0,
        None,
    );
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    block.tool_content_items = vec![TalosContentItem::Diff {
        files: vec![
            TalosToolFileDiff {
                path: "src/account.ts".to_string(),
                old_path: None,
                truncated: false,
                hunks: vec![TalosToolDiffHunk {
                    old_start: Some(1),
                    old_lines: Some(1),
                    new_start: Some(1),
                    new_lines: Some(1),
                    lines: vec![
                        TalosToolDiffLine::Removed {
                            text: "export type AccountType = \"free\";".to_string(),
                        },
                        TalosToolDiffLine::Added {
                            text: "export type AccountType = string;".to_string(),
                        },
                    ],
                }],
            },
            TalosToolFileDiff {
                path: "src/index.ts".to_string(),
                old_path: None,
                truncated: false,
                hunks: vec![TalosToolDiffHunk {
                    old_start: Some(4),
                    old_lines: Some(1),
                    new_start: Some(4),
                    new_lines: Some(1),
                    lines: vec![
                        TalosToolDiffLine::Removed {
                            text: "export { formatAccountType };".to_string(),
                        },
                        TalosToolDiffLine::Added {
                            text: "export type { AccountType };".to_string(),
                        },
                    ],
                }],
            },
        ],
    }];

    let texts = app
        .tool_block_lines(&block, 120)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(
        texts,
        vec![
            "● Edited 2 files (+2 -2)",
            "  ⎿ 2 files changed",
            "    src/account.ts (+1 -1)",
            "    @@ -1,1 +1,1 @@",
            "    -export type AccountType = \"free\";",
            "    +export type AccountType = string;",
            "",
            "    src/index.ts (+1 -1)",
            "    @@ -4,1 +4,1 @@",
            "    -export { formatAccountType };",
            "    +export type { AccountType };",
        ]
    );
}

#[test]
fn test_bash_failure_shows_exit_code_and_stderr_evidence() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new_tool_call(
        "bash-1".to_string(),
        "Bash".to_string(),
        json!({"cmd":"pnpm --dir talos-app check:repo"}),
        1,
        0,
        None,
    );
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: false });
    block.tool_content_items = vec![TalosContentItem::BashOutput {
        session_id: None,
        status: TalosToolTaskStatus::Failed,
        exit_code: Some(1),
        stdout: String::new(),
        stderr: "File exceeds legacy line baseline: src/shared/i18n/resources.ts (1116)"
            .to_string(),
        truncated: false,
        elapsed_ms: Some(368),
    }];

    let texts = app
        .tool_block_lines(&block, 120)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(
        texts,
        vec![
            "● Bash(pnpm --dir talos-app check:repo)",
            "  ⎿ exit 1 File exceeds legacy line baseline: src/shared/i18n/resources.ts (1116) · 0.4s (ctrl+o to expand)",
        ]
    );
}
