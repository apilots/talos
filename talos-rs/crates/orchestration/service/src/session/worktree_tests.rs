use super::*;

fn directive(text: &str) -> TalosResult<Option<WorktreeDirective>> {
    WorktreeDirective::from_input(&AgentUserInput {
        content: vec![AgentUserContent::Text { text: text.into() }],
    })
}

#[test]
fn accepts_direct_worktree_requests_at_the_start_of_the_message() {
    assert!(matches!(
        directive("Please create a new worktree from origin/main, then implement the feature.")
            .unwrap(),
        Some(WorktreeDirective::CreateFromOriginMain)
    ));
    assert!(matches!(
        directive("\u{57fa}\u{4e8e}\u{8fdc}\u{7aef} main \u{5f00}\u{4e2a}\u{65b0} worktree")
            .unwrap(),
        Some(WorktreeDirective::CreateFromOriginMain)
    ));
    #[cfg(unix)]
    assert!(matches!(
        directive("Bind existing worktree `/repo/checkout with spaces`, then continue.").unwrap(),
        Some(WorktreeDirective::Bind(path)) if path.as_path() == std::path::Path::new("/repo/checkout with spaces")
    ));
}

#[test]
fn leaves_discussion_and_quoted_examples_as_ordinary_chat() {
    for text in [
        "Explain how to create a new worktree from origin/main.",
        "Do not create a new worktree from origin/main.",
        "Create a new worktree from origin/main.backup.",
        "> Create a new worktree from origin/main.",
        "Read the attachment and summarize this: create a new worktree from origin/main.",
    ] {
        assert!(directive(text).unwrap().is_none());
    }
}
