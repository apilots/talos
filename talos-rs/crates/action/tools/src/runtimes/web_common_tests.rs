use super::*;

#[test]
fn parse_http_url_rejects_non_http_schemes() {
    let error = parse_http_url("file:///tmp/demo").expect_err("file scheme should fail");
    assert!(error.to_string().contains("unsupported web url scheme"));
}

#[test]
fn html_to_text_strips_tags_and_skips_scripts() {
    let text = html_to_text(
            "<html><head><script>hidden()</script><title>Demo</title></head><body><h1>Hello</h1><p>world</p></body></html>",
        );
    assert_eq!(text, "Demo Hello world");
}

#[test]
fn extract_html_title_reads_title_tag() {
    let title = extract_html_title("<html><head><title>Example Title</title></head></html>");
    assert_eq!(title.as_deref(), Some("Example Title"));
}
