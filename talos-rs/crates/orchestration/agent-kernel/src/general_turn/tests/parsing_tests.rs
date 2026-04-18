use super::*;

#[test]
fn test_parse_retrieved_memory_items_from_memory_search() {
    let payload = json!(MemorySearchResponse {
        results: vec![talos_memory::MemorySearchResult {
            path: "memory/2026-03-24.md".to_string(),
            snippet: "launch on friday".to_string(),
            start_line: 1,
            end_line: 1,
        }],
    });

    let items = parse_retrieved_memory_items("MemorySearch", &payload);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].path, "memory/2026-03-24.md");
}

#[test]
fn test_parse_retrieved_memory_items_ignores_invalid_search_rows() {
    let payload = json!({
        "results": [
            {
                "path": "memory/2026-03-25.md",
                "snippet": "valid snippet",
                "start_line": 1,
                "end_line": 1
            }
        ]
    });

    let items = parse_retrieved_memory_items("MemorySearch", &payload);
    assert_eq!(items.len(), 1);
}

#[test]
fn test_parse_retrieved_memory_items_from_memory_get() {
    let payload = json!(MemoryGetResult {
        path: "MEMORY.md".to_string(),
        text: "remember tea preference".to_string(),
        from: 1,
        lines: 5,
    });

    let items = parse_retrieved_memory_items("MemoryGet", &payload);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].path, "MEMORY.md");
}
