use super::*;

#[tokio::test]
async fn rejecting_reader_accepts_only_empty_references() {
    let reader = RejectingAttachmentReader;
    let scope = TurnFileScope {
        owner_user_id: "user-1".to_string(),
        project_id: None,
    };

    assert_eq!(reader.read(&scope, &[]).await.unwrap(), Vec::new());
    assert!(reader.read(&scope, &["file-1".to_string()]).await.is_err());
}
