//! Committed management responses preserve credentials and current directory facts.
use super::*;
use std::sync::atomic::Ordering;

fn create_request() -> talos_types::CreateNodeRequest {
    talos_types::CreateNodeRequest {
        display_name: "Committed create".into(),
        endpoint: None,
        owner_user_id: None,
        credential_expires_at: chrono::Utc::now().timestamp() + 3600,
    }
}

fn pending_create(
    fixture: &Fixture,
) -> tokio::task::JoinHandle<talos_error::TalosResult<talos_types::CreatedNode>> {
    let nodes = fixture.runtime.service.nodes.clone();
    tokio::spawn(async move { nodes.create_node(&actor(), create_request()).await })
}

async fn assert_created_cache_has_no_credential(fixture: &Fixture) {
    let cached = fixture
        .runtime
        .sync_host
        .sync_stores
        .nodes
        .get("created")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            cached.credential_hash,
            cached.credential_expires_at,
            cached.lease_token_hash
        ),
        (None, None, None)
    );
}

#[tokio::test]
async fn committed_create_delivers_credential_when_overlap_confirmation_is_unavailable() {
    let mut fixture = Fixture::start().await;
    fixture.block_mutation.store(true, Ordering::SeqCst);
    let pending = pending_create(&fixture);
    assert_eq!(fixture.entered.recv().await.unwrap(), "before-commit");
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor())
        .await
        .unwrap();
    *fixture.get_node_failure.write().unwrap() = Some(talos_error::TalosError::new(
        talos_error::ErrorCode::ServiceUnavailable,
        "Controlled confirmation outage.",
    ));
    fixture.release.add_permits(1);
    let created = pending.await.unwrap().unwrap();
    assert_eq!(
        (
            created.node.node_id.as_str(),
            created.credential.node_id.as_str(),
            created.credential.token.as_str()
        ),
        ("created", "created", "fixture-credential")
    );
    assert!(!created.node.available);
    assert!(fixture
        .runtime
        .sync_host
        .sync_stores
        .nodes
        .get("created")
        .await
        .unwrap()
        .is_none());
    *fixture.get_node_failure.write().unwrap() = None;
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor())
        .await
        .unwrap();
    assert_created_cache_has_no_credential(&fixture).await;
    assert_eq!(
        fixture
            .directory
            .read()
            .unwrap()
            .iter()
            .filter(|node| node.node_id == "created")
            .count(),
        1
    );
    fixture.finish().await;
}

#[tokio::test]
async fn committed_create_delivers_credential_when_local_cache_persistence_fails() {
    let fixture = Fixture::start().await;
    let storage = &fixture.runtime.sync_host.sync_stores.storage;
    storage.execute("CREATE TRIGGER reject_created_cache BEFORE INSERT ON agent_nodes WHEN NEW.node_id='created' BEGIN SELECT RAISE(ABORT, 'controlled cache write failure'); END", &[]).await.unwrap();
    let created = fixture
        .runtime
        .service
        .nodes
        .create_node(&actor(), create_request())
        .await
        .unwrap();
    assert_eq!(created.credential.token, "fixture-credential");
    assert!(!created.node.available);
    assert!(fixture
        .runtime
        .sync_host
        .sync_stores
        .nodes
        .get("created")
        .await
        .unwrap()
        .is_none());
    storage
        .execute("DROP TRIGGER reject_created_cache", &[])
        .await
        .unwrap();
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor())
        .await
        .unwrap();
    assert_created_cache_has_no_credential(&fixture).await;
    fixture.finish().await;
}

#[tokio::test]
async fn committed_create_delivers_credential_without_resurrecting_a_deleted_registration() {
    let mut fixture = Fixture::start().await;
    fixture.block_response.store(true, Ordering::SeqCst);
    let pending = pending_create(&fixture);
    assert_eq!(fixture.entered.recv().await.unwrap(), "create");
    fixture
        .runtime
        .service
        .nodes
        .delete_computer(
            &actor(),
            talos_types::DeleteComputerRequest {
                node_id: "created".into(),
            },
        )
        .await
        .unwrap();
    fixture.release.add_permits(1);
    let created = pending.await.unwrap().unwrap();
    assert_eq!(created.credential.token, "fixture-credential");
    assert!(!created.node.available);
    assert!(fixture
        .runtime
        .sync_host
        .sync_stores
        .nodes
        .get("created")
        .await
        .unwrap()
        .is_none());
    assert!(fixture
        .runtime
        .service
        .list_computers(&actor(), Default::default())
        .await
        .unwrap()
        .data
        .iter()
        .all(|node| node.node_id != "created"));
    fixture.finish().await;
}

#[tokio::test]
async fn malformed_create_responses_do_not_deliver_credentials_or_enter_the_cache() {
    for invalid in ["owner", "mode", "request"] {
        let fixture = Fixture::start().await;
        let request = create_request();
        let mut response = directory_node("created");
        response.display_name = request.display_name.clone();
        match invalid {
            "owner" => response.owner_user_id = Some("another-account".into()),
            "mode" => response.mode = talos_types::NodeMode::Embedded,
            "request" => response.endpoint = Some("https://unexpected.example.invalid".into()),
            _ => unreachable!(),
        }
        *fixture.created_response.write().unwrap() = Some(response);
        let result = fixture
            .runtime
            .service
            .nodes
            .create_node(&actor(), request)
            .await;
        assert!(
            matches!(result, Err(error) if error.is_code(talos_error::ErrorCode::PermissionDenied)),
            "invalid {invalid} must fail at the create response boundary"
        );
        assert!(fixture
            .runtime
            .sync_host
            .sync_stores
            .nodes
            .get("created")
            .await
            .unwrap()
            .is_none());
        fixture.finish().await;
    }
}

async fn resolves_overlap_confirmation(code: talos_error::ErrorCode) {
    let mut fixture = Fixture::start().await;
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor())
        .await
        .unwrap();
    fixture.block_response.store(true, Ordering::SeqCst);
    let nodes = fixture.runtime.service.nodes.clone();
    let delayed = tokio::spawn(async move {
        nodes
            .rename_computer(
                &actor(),
                talos_types::RenameComputerRequest {
                    node_id: "fast".into(),
                    display_name: "Delayed name".into(),
                },
            )
            .await
    });
    assert_eq!(fixture.entered.recv().await.unwrap(), "rename");
    fixture
        .runtime
        .service
        .nodes
        .rename_computer(
            &actor(),
            talos_types::RenameComputerRequest {
                node_id: "fast".into(),
                display_name: "Newer cached name".into(),
            },
        )
        .await
        .unwrap();
    // Another manager deletes the central registration after this local observation.
    fixture
        .directory
        .write()
        .unwrap()
        .retain(|node| node.node_id != "fast");
    if code != talos_error::ErrorCode::ResourceExpired {
        *fixture.get_node_failure.write().unwrap() = Some(talos_error::TalosError::new(
            code,
            "Controlled confirmation failure.",
        ));
    }
    fixture.release.add_permits(1);
    let error = delayed.await.unwrap().unwrap_err();
    let deleted = matches!(
        code,
        talos_error::ErrorCode::ResourceExpired | talos_error::ErrorCode::ResourceNotFound
    );
    let expected = if deleted {
        talos_error::ErrorCode::ResourceExpired
    } else {
        code
    };
    assert!(error.is_code(expected));
    let cached = fixture
        .runtime
        .sync_host
        .sync_stores
        .nodes
        .get("fast")
        .await
        .unwrap();
    let page = fixture
        .runtime
        .service
        .list_computers(&actor(), Default::default())
        .await
        .unwrap();
    if deleted {
        assert_eq!(
            error.to_api_error().localization,
            Some(talos_error::ErrorLocalization::ComputerRegistrationRemoved)
        );
        assert!(cached.is_none());
        assert!(page.data.iter().all(|node| node.node_id != "fast"));
    } else {
        assert_eq!(cached.unwrap().display_name, "Newer cached name");
        assert_eq!(
            page.data
                .iter()
                .find(|node| node.node_id == "fast")
                .unwrap()
                .display_name,
            "Newer cached name"
        );
    }
    fixture.finish().await;
}

#[tokio::test]
async fn expired_overlap_confirmation_prunes_a_still_cached_registration() {
    resolves_overlap_confirmation(talos_error::ErrorCode::ResourceExpired).await;
}

#[tokio::test]
async fn not_found_overlap_confirmation_prunes_a_still_cached_registration() {
    resolves_overlap_confirmation(talos_error::ErrorCode::ResourceNotFound).await;
}

#[tokio::test]
async fn unavailable_overlap_confirmation_preserves_cache_and_returns_the_error() {
    resolves_overlap_confirmation(talos_error::ErrorCode::ServiceUnavailable).await;
}

#[tokio::test]
async fn denied_overlap_confirmation_preserves_cache_and_returns_the_error() {
    resolves_overlap_confirmation(talos_error::ErrorCode::PermissionDenied).await;
}

#[tokio::test]
async fn creation_enforces_requested_owner_without_caching_admin_targets() {
    let fixture = Fixture::start().await;
    let mut request = create_request();
    request.owner_user_id = Some("another-account".into());
    let result = fixture
        .runtime
        .service
        .nodes
        .create_node(&actor(), request.clone())
        .await;
    assert!(
        matches!(result, Err(error) if error.is_code(talos_error::ErrorCode::PermissionDenied))
    );
    assert!(fixture
        .directory
        .read()
        .unwrap()
        .iter()
        .all(|node| node.node_id != "created"));
    let mut admin = actor();
    admin.role = talos_auth::UserRole::Admin;
    let created = fixture
        .runtime
        .service
        .nodes
        .create_node(&admin, request)
        .await
        .unwrap();
    assert_eq!(
        created.node.owner_user_id.as_deref(),
        Some("another-account")
    );
    assert_eq!(created.credential.token, "fixture-credential");
    assert!(fixture
        .runtime
        .sync_host
        .sync_stores
        .nodes
        .get("created")
        .await
        .unwrap()
        .is_none());
    fixture.finish().await;
}
