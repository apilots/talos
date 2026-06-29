use super::*;

#[tokio::test]
async fn web_fetch_rejects_zero_max_chars() {
    let error = run(&WebFetchRequest {
        url: "https://example.com".to_string(),
        prompt: None,
        max_chars: 0,
    })
    .await
    .expect_err("zero max chars should fail");
    assert!(error
        .to_string()
        .contains("max_chars must be greater than zero"));
}

#[test]
fn web_fetch_warns_for_private_urls() {
    let url = reqwest::Url::parse("http://127.0.0.1:3000").expect("url should parse");
    assert!(private_url_warning(&url).is_some());
}
