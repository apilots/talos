use talos_types::talos::{
    TalosContentItem, TalosToolDiffHunk, TalosToolDiffLine, TalosToolFileDiff,
};

const MAX_DIFF_LINES_PER_SIDE: usize = 200;
const DIFF_CONTEXT_LINES: usize = 3;
const MAX_LCS_CELLS: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct FileDiffInput<'a> {
    pub path: &'a str,
    pub old_path: Option<&'a str>,
    pub original: &'a str,
    pub updated: &'a str,
}

pub fn build_file_diff_item(input: FileDiffInput<'_>) -> TalosContentItem {
    let original_lines = input.original.lines().collect::<Vec<_>>();
    let updated_lines = input.updated.lines().collect::<Vec<_>>();
    let (hunks, truncated) = if should_use_lcs(&original_lines, &updated_lines) {
        let ops = diff_ops(&original_lines, &updated_lines);
        build_hunks(&ops)
    } else {
        build_bounded_hunk(&original_lines, &updated_lines)
    };

    TalosContentItem::Diff {
        files: vec![TalosToolFileDiff {
            path: input.path.to_string(),
            old_path: input.old_path.map(str::to_string),
            hunks,
            truncated,
        }],
    }
}

fn should_use_lcs(original: &[&str], updated: &[&str]) -> bool {
    (original.len() + 1)
        .checked_mul(updated.len() + 1)
        .map(|cells| cells <= MAX_LCS_CELLS)
        .unwrap_or(false)
}

fn build_bounded_hunk(
    original_lines: &[&str],
    updated_lines: &[&str],
) -> (Vec<TalosToolDiffHunk>, bool) {
    let mut prefix_len = 0usize;
    while prefix_len < original_lines.len()
        && prefix_len < updated_lines.len()
        && original_lines[prefix_len] == updated_lines[prefix_len]
    {
        prefix_len += 1;
    }

    let mut suffix_len = 0usize;
    while suffix_len + prefix_len < original_lines.len()
        && suffix_len + prefix_len < updated_lines.len()
        && original_lines[original_lines.len() - 1 - suffix_len]
            == updated_lines[updated_lines.len() - 1 - suffix_len]
    {
        suffix_len += 1;
    }

    let old_change_end = original_lines.len().saturating_sub(suffix_len);
    let new_change_end = updated_lines.len().saturating_sub(suffix_len);
    let context_start = prefix_len.saturating_sub(DIFF_CONTEXT_LINES);
    let after_context = DIFF_CONTEXT_LINES
        .min(original_lines.len().saturating_sub(old_change_end))
        .min(updated_lines.len().saturating_sub(new_change_end));
    let old_hunk_end = old_change_end + after_context;

    let mut lines = Vec::new();
    let mut old_lines = 0usize;
    let mut new_lines = 0usize;
    let mut line_budget = MAX_DIFF_LINES_PER_SIDE;

    for line in original_lines[context_start..prefix_len].iter() {
        if line_budget == 0 {
            break;
        }
        old_lines += 1;
        new_lines += 1;
        lines.push(TalosToolDiffLine::Context {
            text: (*line).to_string(),
        });
        line_budget -= 1;
    }
    for line in original_lines[prefix_len..old_change_end].iter() {
        if line_budget == 0 {
            break;
        }
        old_lines += 1;
        lines.push(TalosToolDiffLine::Removed {
            text: (*line).to_string(),
        });
        line_budget -= 1;
    }
    for line in updated_lines[prefix_len..new_change_end].iter() {
        if line_budget == 0 {
            break;
        }
        new_lines += 1;
        lines.push(TalosToolDiffLine::Added {
            text: (*line).to_string(),
        });
        line_budget -= 1;
    }
    for line in original_lines[old_change_end..old_hunk_end].iter() {
        if line_budget == 0 {
            break;
        }
        old_lines += 1;
        new_lines += 1;
        lines.push(TalosToolDiffLine::Context {
            text: (*line).to_string(),
        });
        line_budget -= 1;
    }

    if lines.is_empty() {
        return (Vec::new(), true);
    }

    (
        vec![TalosToolDiffHunk {
            old_start: Some((context_start + 1) as u32),
            old_lines: Some(old_lines as u32),
            new_start: Some((context_start + 1) as u32),
            new_lines: Some(new_lines as u32),
            lines,
        }],
        true,
    )
}

#[derive(Debug, Clone, Copy)]
enum DiffOp<'a> {
    Equal(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

impl DiffOp<'_> {
    fn is_change(&self) -> bool {
        !matches!(self, Self::Equal(_))
    }

    fn old_line_count(&self) -> usize {
        match self {
            Self::Equal(_) | Self::Removed(_) => 1,
            Self::Added(_) => 0,
        }
    }

    fn new_line_count(&self) -> usize {
        match self {
            Self::Equal(_) | Self::Added(_) => 1,
            Self::Removed(_) => 0,
        }
    }
}

fn diff_ops<'a>(original: &[&'a str], updated: &[&'a str]) -> Vec<DiffOp<'a>> {
    let width = updated.len() + 1;
    let mut table = vec![0usize; (original.len() + 1) * width];
    for old_index in (0..original.len()).rev() {
        for new_index in (0..updated.len()).rev() {
            let index = old_index * width + new_index;
            table[index] = if original[old_index] == updated[new_index] {
                table[(old_index + 1) * width + new_index + 1] + 1
            } else {
                table[(old_index + 1) * width + new_index]
                    .max(table[old_index * width + new_index + 1])
            };
        }
    }

    let mut ops = Vec::new();
    let mut old_index = 0usize;
    let mut new_index = 0usize;
    while old_index < original.len() && new_index < updated.len() {
        if original[old_index] == updated[new_index] {
            ops.push(DiffOp::Equal(original[old_index]));
            old_index += 1;
            new_index += 1;
        } else if table[(old_index + 1) * width + new_index]
            >= table[old_index * width + new_index + 1]
        {
            ops.push(DiffOp::Removed(original[old_index]));
            old_index += 1;
        } else {
            ops.push(DiffOp::Added(updated[new_index]));
            new_index += 1;
        }
    }
    while old_index < original.len() {
        ops.push(DiffOp::Removed(original[old_index]));
        old_index += 1;
    }
    while new_index < updated.len() {
        ops.push(DiffOp::Added(updated[new_index]));
        new_index += 1;
    }
    ops
}

fn build_hunks(ops: &[DiffOp<'_>]) -> (Vec<TalosToolDiffHunk>, bool) {
    let Some(first_change) = ops.iter().position(DiffOp::is_change) else {
        return (Vec::new(), false);
    };

    let mut groups = Vec::new();
    let mut group_start = first_change;
    let mut group_end = first_change;
    for change_index in ops
        .iter()
        .enumerate()
        .skip(first_change + 1)
        .filter_map(|(index, op)| op.is_change().then_some(index))
    {
        let unchanged_gap = ops[group_end + 1..change_index]
            .iter()
            .filter(|op| !op.is_change())
            .count();
        if unchanged_gap > DIFF_CONTEXT_LINES * 2 {
            groups.push((group_start, group_end));
            group_start = change_index;
        }
        group_end = change_index;
    }
    groups.push((group_start, group_end));

    let mut hunks = Vec::new();
    let mut line_budget = MAX_DIFF_LINES_PER_SIDE;
    let mut truncated = false;
    for (change_start, change_end) in groups {
        if line_budget == 0 {
            truncated = true;
            break;
        }
        let start = context_start(ops, change_start);
        let end = context_end(ops, change_end);
        let mut old_start = 1usize;
        let mut new_start = 1usize;
        for op in &ops[..start] {
            old_start += op.old_line_count();
            new_start += op.new_line_count();
        }

        let mut old_lines = 0usize;
        let mut new_lines = 0usize;
        let mut lines = Vec::new();
        for op in &ops[start..end] {
            if line_budget == 0 {
                truncated = true;
                break;
            }
            match op {
                DiffOp::Equal(text) => {
                    old_lines += 1;
                    new_lines += 1;
                    lines.push(TalosToolDiffLine::Context {
                        text: (*text).to_string(),
                    });
                }
                DiffOp::Removed(text) => {
                    old_lines += 1;
                    lines.push(TalosToolDiffLine::Removed {
                        text: (*text).to_string(),
                    });
                }
                DiffOp::Added(text) => {
                    new_lines += 1;
                    lines.push(TalosToolDiffLine::Added {
                        text: (*text).to_string(),
                    });
                }
            }
            line_budget -= 1;
        }
        if !lines.is_empty() {
            hunks.push(TalosToolDiffHunk {
                old_start: Some(old_start as u32),
                old_lines: Some(old_lines as u32),
                new_start: Some(new_start as u32),
                new_lines: Some(new_lines as u32),
                lines,
            });
        }
    }

    (hunks, truncated)
}

fn context_start(ops: &[DiffOp<'_>], change_start: usize) -> usize {
    let mut index = change_start;
    let mut context = 0usize;
    while index > 0 && context < DIFF_CONTEXT_LINES {
        if matches!(ops[index - 1], DiffOp::Equal(_)) {
            context += 1;
        }
        index -= 1;
    }
    index
}

fn context_end(ops: &[DiffOp<'_>], change_end: usize) -> usize {
    let mut index = change_end + 1;
    let mut context = 0usize;
    while index < ops.len() && context < DIFF_CONTEXT_LINES {
        if matches!(ops[index], DiffOp::Equal(_)) {
            context += 1;
        }
        index += 1;
    }
    index
}

#[cfg(test)]
#[path = "fs_diff_tests.rs"]
mod tests;
