use super::*;

#[test]
fn test_parse_retrieved_memory_items_from_memory_get() {
    let payload = json!(MemoryGetResponse {
        memory: talos_types::MemoryInfo {
            memory_id: "mem-1".to_string(),
            title: "Tea preference".to_string(),
            body: "remember tea preference".to_string(),
            kind: talos_types::MemoryKind::Preference,
            importance: talos_types::MemoryImportance::Normal,
            occurred_at: 1,
            source: talos_types::MemorySourceInfo {
                date: "2026-03-24".to_string(),
                session_id: None,
                from_seq: None,
                until_seq: None,
            },
        },
    });

    let items = parse_retrieved_memory_items("MemoryGet", &payload).expect("memory should parse");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].memory_id, "mem-1");
}
