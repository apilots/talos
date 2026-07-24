use std::collections::BTreeMap;

use serde_json::json;

use super::{FileInfo, FileListRequest, FileScope, FileUpdateRequest};

#[test]
fn file_info_serializes_the_shared_camel_case_contract() {
    let file = FileInfo {
        file_id: "file-1".to_string(),
        scope: FileScope::Project {
            project_id: "project-1".to_string(),
        },
        original_filename: "report.pdf".to_string(),
        mime_type: "application/pdf".to_string(),
        size_bytes: 42,
        tags: vec!["reviewed".to_string()],
        metadata: BTreeMap::from([("source".to_string(), "chat".to_string())]),
        created_at: 100,
        updated_at: 200,
    };

    assert_eq!(
        serde_json::to_value(file).unwrap(),
        json!({
            "fileId": "file-1",
            "scope": { "type": "project", "projectId": "project-1" },
            "originalFilename": "report.pdf",
            "mimeType": "application/pdf",
            "sizeBytes": 42,
            "tags": ["reviewed"],
            "metadata": { "source": "chat" },
            "createdAt": 100,
            "updatedAt": 200
        })
    );
}

#[test]
fn optional_file_request_fields_preserve_omitted_and_empty_semantics() {
    let list = FileListRequest {
        cursor: None,
        limit: None,
        scope: None,
        tags: None,
    };
    let update = FileUpdateRequest {
        file_id: "file-1".to_string(),
        tags: Some(Vec::new()),
        metadata: Some(BTreeMap::new()),
    };

    assert_eq!(
        (
            serde_json::to_value(list).unwrap(),
            serde_json::to_value(update).unwrap()
        ),
        (
            json!({
                "cursor": null,
                "limit": null,
                "scope": null,
                "tags": null
            }),
            json!({
                "fileId": "file-1",
                "tags": [],
                "metadata": {}
            })
        )
    );
}
