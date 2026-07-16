use super::*;

#[test]
fn finalize_hits_applies_domain_filters_and_rank() {
    let request = WebSearchRequest {
        query: "rust tools".to_string(),
        allowed_domains: vec!["rust-lang.org".to_string()],
        blocked_domains: vec!["blocked.rust-lang.org".to_string()],
        recency_days: None,
        limit: 2,
    };
    let hits = finalize_hits(
        &request,
        vec![
            SearchCandidate {
                title: "Blocked".to_string(),
                url: "https://blocked.rust-lang.org/tools".to_string(),
                snippet: "Blocked result".to_string(),
            },
            SearchCandidate {
                title: "Rust Tools".to_string(),
                url: "https://www.rust-lang.org/tools".to_string(),
                snippet: "Official tools page".to_string(),
            },
            SearchCandidate {
                title: "Rust Learn".to_string(),
                url: "https://www.rust-lang.org/learn".to_string(),
                snippet: "Official learning page".to_string(),
            },
        ],
    )
    .expect("search candidates should normalize");

    assert_eq!(
        hits,
        vec![
            WebSearchHit {
                title: "Rust Tools".to_string(),
                url: "https://www.rust-lang.org/tools".to_string(),
                snippet: "Official tools page".to_string(),
                source_domain: Some("rust-lang.org".to_string()),
                rank: 1,
            },
            WebSearchHit {
                title: "Rust Learn".to_string(),
                url: "https://www.rust-lang.org/learn".to_string(),
                snippet: "Official learning page".to_string(),
                source_domain: Some("rust-lang.org".to_string()),
                rank: 2,
            },
        ]
    );
}
