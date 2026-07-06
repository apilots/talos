use crate::render::tool::{helpers, tool_title};
use crate::types::{ChatBlock, ToolCallStatus, ToolRenderData};
use talos_sdk::TalosContentItem;

pub(crate) fn build_task_render_data(
    block: &ChatBlock,
    items: &[TalosContentItem],
) -> ToolRenderData {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = None;
    let mut task_status = None;
    let mut elapsed_ms = None;
    let mut truncated = false;

    for item in items {
        if let TalosContentItem::BashOutput {
            status,
            exit_code: item_exit_code,
            stdout: item_stdout,
            stderr: item_stderr,
            truncated: item_truncated,
            elapsed_ms: item_elapsed_ms,
            ..
        } = item
        {
            append_output(&mut stdout, item_stdout);
            append_output(&mut stderr, item_stderr);
            exit_code = *item_exit_code;
            task_status = Some(*status);
            elapsed_ms = *item_elapsed_ms;
            truncated |= *item_truncated;
        }
    }

    let timing = helpers::timing_suffix(elapsed_ms.or(block.timing_ms));
    let status_text = task_status
        .map(helpers::task_status_text)
        .unwrap_or("Unknown");
    let evidence = bash_evidence(
        block.tool_status.as_ref(),
        exit_code,
        &stdout,
        &stderr,
        status_text,
        &timing,
    );
    let mut preview_lines = preview_lines(&stdout, &stderr);
    if truncated {
        preview_lines.push("output truncated".to_string());
    }
    let output_lines = combined_output_lines(&stdout, &stderr);

    ToolRenderData {
        title: tool_title(block, "Bash"),
        summary: evidence,
        detail_lines: Vec::new(),
        preview_lines,
        output_lines,
        force_show_output: false,
    }
}

pub(crate) fn build_bash_render_data(block: &ChatBlock) -> ToolRenderData {
    let parsed = serde_json::from_str::<serde_json::Value>(&block.content).ok();
    let exit_code = parsed
        .as_ref()
        .and_then(|value| value.get("exit_code"))
        .and_then(|value| value.as_i64())
        .and_then(|value| i32::try_from(value).ok());
    let output = parsed
        .as_ref()
        .and_then(|value| value.get("output"))
        .and_then(|value| value.as_str())
        .unwrap_or(block.content.as_str());
    let status = parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(|value| value.as_str())
        .unwrap_or("Done");
    let timing = helpers::timing_suffix(block.timing_ms);

    let evidence = bash_evidence(
        block.tool_status.as_ref(),
        exit_code,
        output,
        "",
        status,
        &timing,
    );
    let preview_lines = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .map(helpers::compact_preview_text)
        .collect::<Vec<_>>();

    ToolRenderData {
        title: tool_title(block, "Bash"),
        summary: evidence,
        detail_lines: bash_detail_lines(block, parsed.as_ref()),
        preview_lines,
        output_lines: output.lines().map(str::to_string).collect(),
        force_show_output: false,
    }
}

fn append_output(target: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(value);
}

fn bash_evidence(
    status: Option<&ToolCallStatus>,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    status_text: &str,
    timing: &str,
) -> String {
    match status {
        Some(ToolCallStatus::Queued) => "Queued...".to_string(),
        Some(ToolCallStatus::Running) => helpers::first_non_empty_line(stdout.lines())
            .map(|line| format!("Running... {}", helpers::compact_preview_text(&line)))
            .unwrap_or_else(|| format!("Running...{timing}")),
        Some(ToolCallStatus::PendingDynamicInput) => "Input needed".to_string(),
        Some(ToolCallStatus::Completed { exit_ok: false }) => {
            let detail = helpers::first_non_empty_line(stderr.lines().chain(stdout.lines()))
                .map(|line| format!(" {}", helpers::compact_preview_text(&line)))
                .unwrap_or_default();
            exit_code
                .map(|code| format!("exit {code}{detail}{timing}"))
                .unwrap_or_else(|| format!("Error{detail}{timing}"))
        }
        Some(ToolCallStatus::Completed { exit_ok: true }) => {
            let detail = helpers::first_non_empty_line(stdout.lines().chain(stderr.lines()))
                .map(|line| format!(" {}", helpers::compact_preview_text(&line)))
                .unwrap_or_default();
            if detail.is_empty() {
                format!("{status_text}{timing}")
            } else {
                format!("{detail}{timing}")
            }
        }
        None => status_text.to_string(),
    }
}

fn preview_lines(stdout: &str, stderr: &str) -> Vec<String> {
    stdout
        .lines()
        .chain(stderr.lines())
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .map(helpers::compact_preview_text)
        .collect()
}

fn combined_output_lines(stdout: &str, stderr: &str) -> Vec<String> {
    let mut output_lines = stdout.lines().map(str::to_string).collect::<Vec<_>>();
    if !stderr.trim().is_empty() {
        if !output_lines.is_empty() {
            output_lines.push(String::new());
        }
        output_lines.push("stderr:".to_string());
        output_lines.extend(stderr.lines().map(str::to_string));
    }
    output_lines
}

fn bash_detail_lines(block: &ChatBlock, parsed: Option<&serde_json::Value>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(dir) = block.tool_input.as_ref().and_then(|input| {
        input
            .as_inner()
            .get("workdir")
            .or_else(|| input.as_inner().get("cwd"))
            .and_then(|value| value.as_str())
    }) {
        lines.push(format!("dir: {dir}"));
    }
    if let Some(status) = parsed
        .and_then(|value| value.get("status"))
        .and_then(|value| value.as_str())
    {
        lines.push(format!("status: {status}"));
    }
    lines
}
