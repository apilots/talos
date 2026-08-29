use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use talos_error::TalosResult;
use talos_memory::{MemoryReader, MemoryScope};
use talos_types::{
    MemoryGetRequest, MemoryGetResponse, MemoryImportance, MemoryInfo, MemoryKind, MemorySearchHit,
    MemorySearchRequest, MemorySearchResponse, MemorySourceInfo,
};

use crate::{MemoryGetTool, MemorySearchTool, Tool, ToolContext, ToolOutputPayload};

struct TestMemoryReader;

impl MemoryReader for TestMemoryReader {
    fn search<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _request: MemorySearchRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<MemorySearchResponse>> + Send + 'a>> {
        Box::pin(async { Ok(MemorySearchResponse { data: vec![hit()] }) })
    }

    fn get<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _request: MemoryGetRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<MemoryGetResponse>> + Send + 'a>> {
        Box::pin(async {
            Ok(MemoryGetResponse {
                memory: memory_info(),
            })
        })
    }
}

#[tokio::test]
async fn memory_tools_return_shared_dtos() {
    let context = ToolContext::new("session-1", "user-1")
        .with_memory_scope(MemoryScope {
            workspace_id: "default".to_string(),
            user_id: "user-1".to_string(),
            workspace_root: std::env::temp_dir(),
        })
        .with_memory_reader(Arc::new(TestMemoryReader));

    let search = MemorySearchTool
        .execute_payload(serde_json::json!({ "query": "tea" }), context.clone())
        .await
        .expect("search should succeed");
    let get = MemoryGetTool
        .execute_payload(serde_json::json!({ "memoryId": "mem-1" }), context)
        .await
        .expect("get should succeed");

    assert_eq!(payload(search), serde_json::json!({ "data": [hit()] }));
    assert_eq!(payload(get), serde_json::json!({ "memory": memory_info() }));
}

fn payload(output: ToolOutputPayload) -> serde_json::Value {
    match output {
        ToolOutputPayload::Function {
            structured_content: Some(value),
            ..
        } => value,
        _ => panic!("expected structured tool output"),
    }
}

fn source() -> MemorySourceInfo {
    MemorySourceInfo {
        date: "2026-03-24".to_string(),
        session_id: Some("session-1".to_string()),
        from_seq: Some(1),
        until_seq: Some(2),
    }
}

fn hit() -> MemorySearchHit {
    MemorySearchHit {
        memory_id: "mem-1".to_string(),
        title: "Tea preference".to_string(),
        preview: "Prefers green tea".to_string(),
        kind: MemoryKind::Preference,
        importance: MemoryImportance::Normal,
        occurred_at: 1,
        rank: 1,
        source: source(),
    }
}

fn memory_info() -> MemoryInfo {
    MemoryInfo {
        memory_id: "mem-1".to_string(),
        title: "Tea preference".to_string(),
        body: "Prefers green tea".to_string(),
        kind: MemoryKind::Preference,
        importance: MemoryImportance::Normal,
        occurred_at: 1,
        source: source(),
    }
}
