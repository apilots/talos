use super::super::{FileContent, FilePeerAccess};
use super::*;
use agent_protocol::AgentFuture;
use talos_error::TalosResult;
use talos_types::{FileDeleteResponse, FileUpdateResponse};

struct SourceFiles(Arc<TestFiles>);
impl FilePeerAccess for SourceFiles {
    fn read<'a>(
        &'a self,
        owner: &'a str,
        _: &'a str,
        request: FileReadRequest,
        max_bytes: u64,
    ) -> AgentFuture<'a, TalosResult<FileContent>> {
        Box::pin(async move {
            let content = self
                .0
                .service
                .read_content(&Actor::regular(owner), request)
                .await?;
            assert!(content.bytes.len() as u64 <= max_bytes);
            Ok(content)
        })
    }
    fn read_range<'a>(
        &'a self,
        owner: &'a str,
        _: &'a str,
        request: FileReadRequest,
        range: std::ops::Range<u64>,
    ) -> AgentFuture<'a, TalosResult<FileContent>> {
        Box::pin(async move {
            self.0
                .service
                .read_preview_range(&Actor::regular(owner), &request.file_id, range)
                .await
        })
    }
    fn update<'a>(
        &'a self,
        owner: &'a str,
        _: &'a str,
        request: FileUpdateRequest,
    ) -> AgentFuture<'a, TalosResult<FileUpdateResponse>> {
        Box::pin(async move { self.0.service.update(&Actor::regular(owner), request).await })
    }
    fn delete<'a>(
        &'a self,
        owner: &'a str,
        _: &'a str,
        request: FileDeleteRequest,
    ) -> AgentFuture<'a, TalosResult<FileDeleteResponse>> {
        Box::pin(async move { self.0.service.delete(&Actor::regular(owner), request).await })
    }
}

#[tokio::test]
async fn imported_attachment_uses_source_fileservice_and_never_deletes_a_local_object() {
    let source = Arc::new(TestFiles::new(|_| {}).await);
    let target = TestFiles::new(|_| {}).await;
    let actor = Actor::regular("file-owner");
    let file = source.upload(&actor, FileScope::Personal).await;
    let key = talos_store::file::FileOwnerKey {
        owner_user_id: actor.user_id.clone(),
        file_id: file.file_id.clone(),
    };
    let record = source
        .store
        .get_file_for_owner(&key)
        .await
        .unwrap()
        .unwrap();
    let (_, source_key) = record.local_location().unwrap();
    let source_key = source_key.to_owned();
    let sync = Arc::new(
        talos_sync::TalosSync::open(
            target.storage.clone(),
            talos_sync::SyncOptions {
                hub_endpoint: "wss://sync.example/v1/sync".into(),
            },
        )
        .await
        .unwrap(),
    );
    let binding = sync
        .bind_account(
            &talos_auth::RequestIdentity {
                user_id: actor.user_id.clone(),
                username: None,
                role: talos_auth::UserRole::User,
                auth_disabled: false,
                access_token: None,
            },
            talos_sync::SyncBindParams {
                scope: talos_sync::SyncScope {
                    hub_id: uuid::Uuid::new_v4().to_string(),
                    account_id: actor.user_id.clone(),
                },
                source_user_id: actor.user_id.clone(),
                computers: vec![],
            },
        )
        .await
        .unwrap();
    let adapter = talos_store::sync::StoreSyncAdapter::new(
        target.storage.clone(),
        sync,
        binding.clone(),
        Arc::new(|_| {
            Err(talos_error::TalosError::internal(
                "This file-only fixture has no Sessions.",
            ))
        }),
    );
    let change = talos_sync::SyncChange::Metadata {
        before: None,
        after: Box::new(talos_sync::SyncResourceSnapshot::File(
            talos_sync::SyncFileSnapshot {
                file: file.clone(),
                storage_peer_id: uuid::Uuid::new_v4().to_string(),
            },
        )),
    };
    let mut tx = talos_storage::Storage::begin_transaction(&*target.storage)
        .await
        .unwrap();
    talos_sync::SyncDataAdapter::apply(
        &adapter,
        &mut *tx,
        &talos_sync::SyncImportBatch {
            scope: binding.scope,
            resource: talos_sync::SyncResourceKey::File {
                file_id: file.file_id.clone(),
            },
            version: Default::default(),
            changes: vec![serde_json::from_value(serde_json::to_value(change).unwrap()).unwrap()],
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    // An unrelated local object with the same name must never be consulted.
    target
        .operator
        .write(&source_key, "target-owned-object")
        .await
        .unwrap();
    let request = FileReadRequest {
        file_id: file.file_id.clone(),
    };
    assert_eq!(
        target
            .service
            .read_content(&actor, request.clone())
            .await
            .err()
            .unwrap()
            .error_code(),
        ErrorCode::ServiceUnavailable
    );
    target
        .service
        .set_peer_access(Arc::new(SourceFiles(source.clone())));
    assert_eq!(
        target
            .service
            .read_content(&actor, request.clone())
            .await
            .unwrap()
            .bytes
            .as_ref(),
        PNG
    );
    assert!(target
        .service
        .read_content(&Actor::regular("another-owner"), request.clone())
        .await
        .is_err());
    let range = target
        .service
        .read_preview_range(&actor, &request.file_id, 1..4)
        .await
        .unwrap();
    let whole = source
        .service
        .read_content(&actor, request.clone())
        .await
        .unwrap();
    assert_eq!(range.bytes, whole.bytes.slice(1..4));
    assert!(target
        .service
        .read_preview_range(&actor, &request.file_id, 0..0)
        .await
        .unwrap()
        .bytes
        .is_empty());
    let update = target
        .service
        .update(
            &actor,
            FileUpdateRequest {
                file_id: file.file_id.clone(),
                tags: Some(vec!["source-update".into()]),
                metadata: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(update.file.tags, ["source-update"]);
    assert_eq!(
        source
            .service
            .read(&actor, request.clone())
            .await
            .unwrap()
            .file
            .tags,
        ["source-update"]
    );
    assert_eq!(
        target
            .service
            .read(&actor, request.clone())
            .await
            .unwrap()
            .file
            .tags,
        file.tags
    );
    target
        .service
        .delete(
            &actor,
            FileDeleteRequest {
                file_id: file.file_id.clone(),
            },
        )
        .await
        .unwrap();
    assert!(source.service.read(&actor, request.clone()).await.is_err());
    assert!(target.service.read_content(&actor, request).await.is_err());
    assert_eq!(
        target
            .operator
            .read(&source_key)
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        b"target-owned-object"
    );
    assert!(target
        .store
        .get_file_for_owner(&key)
        .await
        .unwrap()
        .is_some());
}
