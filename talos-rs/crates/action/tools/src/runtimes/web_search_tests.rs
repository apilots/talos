use super::*;

#[test]
fn parse_search_results_filters_domains_from_json() {
    let body = serde_json::json!({
        "Results": [
            {
                "FirstURL": "https://www.rust-lang.org/tools",
                "Text": "Rust Tools - Official tools page"
            },
            {
                "FirstURL": "https://spam.example/bad",
                "Text": "Spam - Ignore me"
            }
        ]
    });
    let hits = parse_duckduckgo_results(
        &body,
        &[String::from("rust-lang.org")],
        &[String::from("spam.example")],
        5,
    )
    .expect("search results should parse");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Rust Tools");
    assert_eq!(hits[0].provider_rank, Some(1));
}

#[test]
fn brave_provider_requires_api_key() {
    let config = WebSearchToolConfig {
        provider: WebSearchProviderConfig::Brave,
        ..WebSearchToolConfig::default()
    };

    let error = match provider_for_config(&config) {
        Ok(_) => panic!("brave should require key"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("requires API key"));
}

#[test]
fn recency_maps_to_provider_freshness_values() {
    assert_eq!(brave_freshness(Some(1)), Some("pd"));
    assert_eq!(brave_freshness(Some(7)), Some("pw"));
    assert_eq!(tavily_time_range(Some(31)), Some("month"));
}

#[tokio::test]
async fn web_search_rejects_empty_query() {
    let error = run(&WebSearchRequest {
        query: "   ".to_string(),
        allowed_domains: Vec::new(),
        blocked_domains: Vec::new(),
        domains: Vec::new(),
        recency_days: None,
        limit: 5,
    })
    .await
    .expect_err("empty query should fail");
    assert!(error.to_string().contains("query must not be empty"));
}
