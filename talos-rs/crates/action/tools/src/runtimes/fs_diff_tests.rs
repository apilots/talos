use super::*;

#[test]
fn builds_focused_hunk_for_single_line_edit() {
    let item = build_file_diff_item(FileDiffInput {
        path: "src/lib.rs",
        old_path: None,
        original: "one\ntwo\nthree\nfour\nfive\n",
        updated: "one\ntwo\nTHREE\nfour\nfive\n",
    });

    let TalosContentItem::Diff { files } = item else {
        panic!("expected diff item");
    };
    let hunk = &files[0].hunks[0];
    let removed = hunk
        .lines
        .iter()
        .filter(|line| matches!(line, TalosToolDiffLine::Removed { .. }))
        .count();
    let added = hunk
        .lines
        .iter()
        .filter(|line| matches!(line, TalosToolDiffLine::Added { .. }))
        .count();

    assert_eq!(removed, 1);
    assert_eq!(added, 1);
    assert!(hunk
        .lines
        .iter()
        .any(|line| matches!(line, TalosToolDiffLine::Removed { text } if text == "three")));
    assert!(hunk
        .lines
        .iter()
        .any(|line| matches!(line, TalosToolDiffLine::Added { text } if text == "THREE")));
}

#[test]
fn builds_separate_hunks_for_distant_edits() {
    let original = (1..=20)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let updated = (1..=20)
        .map(|index| match index {
            3 => "line THREE".to_string(),
            18 => "line EIGHTEEN".to_string(),
            _ => format!("line {index}"),
        })
        .collect::<Vec<_>>()
        .join("\n");

    let item = build_file_diff_item(FileDiffInput {
        path: "src/lib.rs",
        old_path: None,
        original: &original,
        updated: &updated,
    });

    let TalosContentItem::Diff { files } = item else {
        panic!("expected diff item");
    };
    assert_eq!(files[0].hunks.len(), 2);
    assert!(!files[0].hunks.iter().any(|hunk| hunk
        .lines
        .iter()
        .any(|line| matches!(line, TalosToolDiffLine::Removed { text } if text == "line 10"))));
    assert!(!files[0].hunks.iter().any(|hunk| hunk
        .lines
        .iter()
        .any(|line| matches!(line, TalosToolDiffLine::Added { text } if text == "line 10"))));
}

#[test]
fn large_diff_uses_bounded_truncated_hunk() {
    let original = (1..=1_200)
        .map(|index| format!("old line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let updated = (1..=1_200)
        .map(|index| format!("new line {index}"))
        .collect::<Vec<_>>()
        .join("\n");

    let item = build_file_diff_item(FileDiffInput {
        path: "large.txt",
        old_path: None,
        original: &original,
        updated: &updated,
    });

    let TalosContentItem::Diff { files } = item else {
        panic!("expected diff item");
    };
    assert!(files[0].truncated);
    assert_eq!(files[0].hunks.len(), 1);
    assert!(files[0].hunks[0].lines.len() <= MAX_DIFF_LINES_PER_SIDE);
}
