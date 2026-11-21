use super::*;

#[tokio::test]
async fn directory_reads_remain_local_while_control_plane_observations_are_blocked() {
    let mut fixture = Fixture::start().await;
    let id = "status-blocked";
    fixture
        .runtime
        .sync_host
        .sync_stores
        .nodes
        .create(talos_store::node::CreateNodeParams {
            node_id: id.into(),
            owner_user_id: Some("account".into()),
            display_name: "Local synchronized name".into(),
            mode: talos_types::NodeMode::Remote,
            endpoint: Some("https://local-metadata.invalid".into()),
            credential_hash: None,
            credential_expires_at: None,
            now_at: 7,
        })
        .await
        .unwrap();
    let actor = talos_service::Actor::regular("account").with_access_token("credential");
    let host = fixture.runtime.sync_host.clone();
    let refresh_actor = actor.clone();
    let refresh =
        tokio::spawn(async move { host.refresh_computer_observations(&refresh_actor).await });
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), fixture.entered.recv())
            .await
            .unwrap()
            .unwrap(),
        format!("status:{id}")
    );
    let page = tokio::time::timeout(
        Duration::from_millis(500),
        fixture
            .runtime
            .service
            .list_computers(&actor, talos_types::ComputerListRequest::default()),
    )
    .await
    .expect("metadata list waited for the blocked control plane")
    .unwrap();
    let local = page
        .data
        .into_iter()
        .find(|node| node.node_id == id)
        .unwrap();
    assert_eq!(local.display_name, "Local synchronized name");
    assert!(!local.available);
    assert_eq!(local.health, talos_types::NodeHealth::Unknown);
    fixture.release.add_permits(1);
    refresh.await.unwrap().unwrap();
    let observed = fixture
        .runtime
        .sync_host
        .computer_view(&actor, local.clone())
        .await
        .unwrap();
    assert!(observed.available);
    assert_eq!(observed.health, talos_types::NodeHealth::Healthy);
    assert_eq!(observed.display_name, local.display_name);
    assert_eq!(observed.endpoint, local.endpoint);
    assert_eq!(observed.created_at, local.created_at);
    assert_eq!(observed.updated_at, local.updated_at);
    assert_eq!(observed.owner_user_id, local.owner_user_id);
    let other = fixture
        .runtime
        .sync_host
        .computer_view(
            &talos_service::Actor::regular("another-account").with_access_token("credential"),
            local,
        )
        .await
        .unwrap();
    assert!(
        !other.available,
        "observations must be scoped to the authenticated account"
    );
    fixture.finish().await;
}
