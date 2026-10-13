//! Real WSS/SDK exchange between two original Talos management databases.
//! Run against an independently built talos-sync server and controlled Auth.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use talos_auth::{RequestIdentity, UserRole};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::SqliteStorage;
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_sync::{
    ComputerAlias, SyncBindParams, SyncBinding, SyncHandle, SyncOptions, SyncResourceKey,
    SyncStartParams, TalosSync,
};

#[path = "pair/projects.rs"]
mod projects;
use projects::{capture_project, delete_project, rename, ProjectAdapter};

struct Peer {
    storage: Arc<SqliteStorage>,
    sync: Arc<TalosSync>,
    binding: SyncBinding,
    adapter: Arc<ProjectAdapter>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let endpoint = arguments
        .get("endpoint")
        .ok_or("--endpoint is required")?
        .clone();
    let account = arguments
        .get("account")
        .ok_or("--account is required")?
        .clone();
    let home = PathBuf::from(arguments.get("home").ok_or("--home is required")?);
    let ca = std::fs::read(arguments.get("ca").ok_or("--ca is required")?)?;
    let token_file = PathBuf::from(
        arguments
            .get("token-file")
            .ok_or("--token-file is required")?,
    );
    let credential_account = account.clone();
    let credentials: talos_sync_sdk::CredentialCallback = Arc::new(move || {
        let account_id = credential_account.clone();
        let path = token_file.clone();
        Box::pin(async move {
            let access_token = std::fs::read_to_string(path)
                .map_err(|_| {
                    talos_sync_types::SyncError::new(
                        talos_sync_types::SyncErrorCode::Unauthenticated,
                    )
                })?
                .trim()
                .into();
            Ok(talos_sync_sdk::VerifiedCredential {
                account_id,
                access_token,
            })
        })
    });
    let mut probe_options = talos_sync_sdk::SyncOptions::new(
        endpoint.clone(),
        account.clone(),
        uuid::Uuid::new_v4().to_string(),
    );
    probe_options.trusted_ca_pem = Some(ca.clone());
    let probe = talos_sync_sdk::SyncClient::connect(probe_options, credentials.clone()).await?;
    let scope = probe.scope().await;
    probe.shutdown().await?;
    let computer = uuid::Uuid::new_v4().to_string();
    let a = peer(&home.join("a"), &endpoint, &account, &scope, &computer).await?;
    let b = peer(&home.join("b"), &endpoint, &account, &scope, &computer).await?;
    let projects_a = StorageProjectStore::new(a.storage.clone()).await?;
    let projects_b = StorageProjectStore::new(b.storage.clone()).await?;
    let project = projects_a
        .create_project_on_node(
            "Bootstrap",
            Path::new("/worker/project"),
            &account,
            &computer,
        )
        .await?;
    let project_id = project.project_id;
    let mut handle_a = start(&a, credentials.clone(), &ca)?;
    let handle_b = start(&b, credentials.clone(), &ca)?;
    wait_name(
        &projects_b,
        &project_id,
        Some("Bootstrap"),
        &handle_a,
        &handle_b,
    )
    .await?;
    let resource = SyncResourceKey::Project {
        project_id: project_id.clone(),
    };
    let mut observed = handle_b.subscribe_changes();
    rename(&a, &project_id, "Realtime").await?;
    let shared_cut = handle_a.flush(resource.clone()).await?;
    let imported = handle_b
        .catch_up(resource.clone(), Some(shared_cut.clone()))
        .await?;
    if !talos_sync_sdk::version_includes(&imported, &shared_cut)? {
        return Err("recovered facts do not include the durable shared cut".into());
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let change = observed.recv().await?;
            if change.resource == resource
                && talos_sync_sdk::version_includes(&change.version, &shared_cut)?
            {
                break Ok::<_, Box<dyn std::error::Error>>(());
            }
        }
    })
    .await??;
    wait_name(
        &projects_b,
        &project_id,
        Some("Realtime"),
        &handle_a,
        &handle_b,
    )
    .await?;
    handle_a.shutdown().await?;
    rename(&a, &project_id, "Offline").await?;
    let local_cut = a
        .sync
        .export_pending(&a.binding, resource.clone(), 128)
        .await?
        .ok_or("offline cut missing")?
        .version;
    let unavailable = handle_b
        .catch_up(resource.clone(), Some(local_cut))
        .await
        .unwrap_err();
    if unavailable.error_code() != ErrorCode::Conflict {
        return Err("unpublished minimum did not fail explicitly".into());
    }
    if projects_b.get_project(&project_id).await?.unwrap().name != "Realtime" {
        return Err("offline mutation leaked without transport".into());
    }
    let restarted_sync = Arc::new(
        TalosSync::open(
            a.storage.clone(),
            SyncOptions {
                hub_endpoint: endpoint,
            },
        )
        .await?,
    );
    if restarted_sync.peer_id() != a.sync.peer_id() {
        return Err("persistent management identity changed on restart".into());
    }
    let a = Peer {
        sync: restarted_sync,
        ..a
    };
    handle_a = start(&a, credentials.clone(), &ca)?;
    wait_name(
        &projects_b,
        &project_id,
        Some("Offline"),
        &handle_a,
        &handle_b,
    )
    .await?;
    rename(&b, &project_id, "ModifiedFromB").await?;
    handle_b.notify_committed();
    wait_name(
        &projects_a,
        &project_id,
        Some("ModifiedFromB"),
        &handle_a,
        &handle_b,
    )
    .await?;
    delete_project(&b, &project_id).await?;
    handle_b.notify_committed();
    wait_name(&projects_a, &project_id, None, &handle_a, &handle_b).await?;
    // A's offline patch must not repeatedly poison uploads after B publishes a
    // terminal tombstone. Another committed resource must continue synchronizing.
    let superseded = projects_a
        .create_project_on_node(
            "Superseded",
            Path::new("/worker/superseded"),
            &account,
            &computer,
        )
        .await?;
    capture_project(&a, &superseded.project_id).await?;
    handle_a.notify_committed();
    wait_name(
        &projects_b,
        &superseded.project_id,
        Some("Superseded"),
        &handle_a,
        &handle_b,
    )
    .await?;
    handle_a.shutdown().await?;
    rename(&a, &superseded.project_id, "Offline patch").await?;
    let independent = projects_a
        .create_project_on_node(
            "Independent",
            Path::new("/worker/independent"),
            &account,
            &computer,
        )
        .await?;
    capture_project(&a, &independent.project_id).await?;
    delete_project(&b, &superseded.project_id).await?;
    handle_b
        .flush(SyncResourceKey::Project {
            project_id: superseded.project_id.clone(),
        })
        .await?;
    handle_a = start(&a, credentials.clone(), &ca)?;
    wait_name(
        &projects_a,
        &superseded.project_id,
        None,
        &handle_a,
        &handle_b,
    )
    .await?;
    wait_name(
        &projects_b,
        &independent.project_id,
        Some("Independent"),
        &handle_a,
        &handle_b,
    )
    .await?;
    let rejected = handle_a
        .flush(SyncResourceKey::Project {
            project_id: superseded.project_id,
        })
        .await
        .unwrap_err();
    if rejected.error_code() != ErrorCode::Conflict {
        return Err("superseded offline patch received a false ACK".into());
    }

    // The opposite direction must keep A's own pending Delete until H actually
    // acknowledges it, despite B's concurrently published metadata patch.
    let local_delete = projects_a
        .create_project_on_node(
            "Local delete",
            Path::new("/worker/local-delete"),
            &account,
            &computer,
        )
        .await?;
    capture_project(&a, &local_delete.project_id).await?;
    handle_a.notify_committed();
    wait_name(
        &projects_b,
        &local_delete.project_id,
        Some("Local delete"),
        &handle_a,
        &handle_b,
    )
    .await?;
    handle_a.shutdown().await?;
    delete_project(&a, &local_delete.project_id).await?;
    rename(&b, &local_delete.project_id, "Concurrent patch").await?;
    handle_b
        .flush(SyncResourceKey::Project {
            project_id: local_delete.project_id.clone(),
        })
        .await?;
    handle_a = start(&a, credentials, &ca)?;
    wait_name(
        &projects_b,
        &local_delete.project_id,
        None,
        &handle_a,
        &handle_b,
    )
    .await?;
    handle_a
        .flush(SyncResourceKey::Project {
            project_id: local_delete.project_id,
        })
        .await?;
    handle_a.shutdown().await?;
    handle_b.shutdown().await?;
    let report = serde_json::json!({"passed": ["original_store_bootstrap", "realtime_incremental", "offline_outbox_restart", "same_account_bidirectional_edit", "durable_tombstone", "durable_flush_minimum_cut", "committed_resource_notifications", "offline_patch_superseded_by_remote_delete", "pending_delete_with_concurrent_remote_patch"], "sourcePeerId": a.sync.peer_id(), "targetPeerId": b.sync.peer_id(), "computerId": computer, "projectId": project_id, "scope": scope });
    std::fs::write(
        home.join("result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn arguments() -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut result = HashMap::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(name) = arguments.next() {
        let key = name.strip_prefix("--").ok_or("expected named argument")?;
        if !["endpoint", "ca", "account", "token-file", "home"].contains(&key) {
            return Err("unknown argument".into());
        }
        result.insert(
            key.into(),
            arguments.next().ok_or("missing argument value")?,
        );
    }
    Ok(result)
}

async fn peer(
    home: &Path,
    endpoint: &str,
    account: &str,
    scope: &talos_sync::SyncScope,
    computer: &str,
) -> TalosResult<Peer> {
    let storage = Arc::new(SqliteStorage::new(home.join("talos.db").to_str().unwrap()).await?);
    talos_store::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await?;
    let sync = Arc::new(
        TalosSync::open(
            storage.clone(),
            SyncOptions {
                hub_endpoint: endpoint.into(),
            },
        )
        .await?,
    );
    let actor = RequestIdentity {
        user_id: account.into(),
        username: None,
        role: UserRole::User,
        auth_disabled: false,
        access_token: None,
    };
    let binding = sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: scope.clone(),
                source_user_id: account.into(),
                computers: vec![ComputerAlias {
                    source_node_id: computer.into(),
                    computer_id: computer.into(),
                }],
            },
        )
        .await?;
    Ok(Peer {
        adapter: Arc::new(ProjectAdapter {
            storage: storage.clone(),
        }),
        storage,
        sync,
        binding,
    })
}

fn start(
    peer: &Peer,
    credentials: talos_sync_sdk::CredentialCallback,
    ca: &[u8],
) -> TalosResult<SyncHandle> {
    peer.sync.clone().start(
        peer.adapter.clone(),
        SyncStartParams {
            binding: peer.binding.clone(),
            credentials,
            trusted_ca_pem: Some(ca.to_vec()),
            reconnect_delay: Duration::from_millis(100),
        },
    )
}

async fn wait_name(
    projects: &StorageProjectStore,
    project_id: &str,
    expected: Option<&str>,
    a: &SyncHandle,
    b: &SyncHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let project = projects.get_project(project_id).await?;
            if project.as_ref().map(|project| project.name.as_str()) == expected {
                return Ok::<_, TalosError>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| {
        format!(
            "pair did not reach {expected:?}; A={:?}, B={:?}",
            a.status(),
            b.status()
        )
    })??;
    Ok(())
}
