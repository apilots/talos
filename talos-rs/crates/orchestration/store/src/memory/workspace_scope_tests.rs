use super::*;

#[tokio::test]
async fn different_agents_append_to_one_workspace_owned_memory_document() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    let scope = memory_scope(workspace.path(), "user-1");
    let first = memory_batch("session-agent-1", 1_784_251_800, 1, 1, "first Agent memory");
    store
        .write_batch(
            &scope,
            "memory",
            UTC,
            &dream_write_request_for_agent(workspace.path(), "agent-1", &first),
        )
        .await
        .expect("first Agent write");
    let second = memory_batch(
        "session-agent-2",
        1_784_251_800,
        1,
        1,
        "second Agent memory",
    );
    store
        .write_batch(
            &scope,
            "memory",
            UTC,
            &dream_write_request_for_agent(workspace.path(), "agent-2", &second),
        )
        .await
        .expect("second Agent write");

    let markdown = fs::read_to_string(workspace.path().join("memory/2026-07-17.md"))
        .expect("shared canonical memory");
    assert!(markdown.contains("by: talos-agent/agent-1"));
    assert!(markdown.contains("first Agent memory"));
    assert!(markdown.contains("second Agent memory"));
    assert_eq!(
        store
            .search(
                &scope,
                &MemorySearchQuery {
                    text: "Agent memory".to_string(),
                    limit: 6,
                    kinds: None,
                    since_at: None,
                    until_at: None,
                },
            )
            .await
            .expect("shared memory search")
            .len(),
        2
    );
}
