use super::extract_story_lines_from_snapshot;

#[test]
fn extracts_story_titles_from_current_playwright_snapshot() {
    let snapshot = r#"
- row [ref=e32]:
  - cell "1." [ref=e33]
  - cell [ref=e34]:
    - link "upvote" [ref=e36] [cursor=pointer]:
      - /url: vote?id=1
  - cell [ref=e38]:
    - generic [ref=e39]:
      - link "Japan develops a method to recover lithium" [ref=e40] [cursor=pointer]:
        - /url: https://example.com/story
      - generic [ref=e41]:
        - link "example.com" [ref=e42] [cursor=pointer]
- row [ref=e53]:
  - cell "2." [ref=e54]
  - cell [ref=e55]:
    - link "upvote" [ref=e57] [cursor=pointer]:
      - /url: vote?id=2
  - cell [ref=e59]:
    - link "The Git history command deserves more attention" [ref=e61] [cursor=pointer]:
      - /url: https://example.org/story
"#;

    assert_eq!(
        extract_story_lines_from_snapshot(snapshot),
        vec![
            "Japan develops a method to recover lithium".to_string(),
            "The Git history command deserves more attention".to_string(),
        ]
    );
}
