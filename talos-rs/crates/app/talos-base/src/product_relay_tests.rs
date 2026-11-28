//! Local two-product acceptance: real product routers over the outbound relay socket.
use super::*;
use crate::TalosApplicationRuntime;
use agent_client::{AgentNodeLeaseUpdate, AgentTlsTrust};
use agent_protocol::{CreateNodeRequest, Deployment, NodeAuth, RegisterParams};
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use std::fs;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use talos_gateway_http::{HttpServer, ProductRequestRouter, ProductRoute};
use talos_types::*;
use tower::ServiceExt;

async fn runtime(label: &str, owner: NodeProductOwner) -> TalosApplicationRuntime {
    let mut config = Config::default();
    isolate_runtime_workspace(&mut config, label);
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".into();
    config.llm.anthropic.api_key = None;
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    config.storage.files.preview_base_url = Some("https://preview.example.test/".into());
    config.agent.kernel.plugins.sources.clear();
    config.agent.kernel.plugins.roots.clear();
    super::mcp_tests::save(&config);
    crate::application_runtime::build_application_runtime_with_policy(
        config,
        &test_migrations_dir(),
        crate::SessionOwnershipPolicy::default(),
        owner,
    )
    .await
    .unwrap()
}
struct ProductAuth(&'static str);
#[async_trait::async_trait]
impl talos_auth::AuthService for ProductAuth {
    fn local_identity(&self, token: &str) -> talos_error::TalosResult<talos_auth::RequestIdentity> {
        if token != self.0 && token != "foreign-token" {
            return Err(talos_error::TalosError::new(
                talos_error::ErrorCode::Unauthenticated,
                "Unknown product credential.",
            ));
        }
        Ok(talos_auth::RequestIdentity {
            user_id: if token == "foreign-token" {
                "other-account".into()
            } else {
                "anonymous".into()
            },
            username: Some("anonymous".into()),
            role: Default::default(),
            user_type: Default::default(),
            auth_disabled: false,
            access_token: Some(token.into()),
        })
    }
    async fn verify_access_token(
        &self,
        token: &str,
    ) -> talos_error::TalosResult<talos_auth::RequestIdentity> {
        self.local_identity(token)
    }
    async fn logout(&self, _: &talos_auth::RequestIdentity) -> talos_error::TalosResult<()> {
        Ok(())
    }
}
fn http(runtime: &TalosApplicationRuntime, token: &'static str) -> HttpServer {
    HttpServer::new(
        runtime.service.clone(),
        runtime.event_service.clone(),
        Arc::new(ProductAuth(token)),
        runtime.config.clone(),
        "127.0.0.1",
        0,
    )
    .with_auth_disabled(false)
}
async fn call<T: serde::de::DeserializeOwned>(
    router: axum::Router,
    path: &str,
    target: Option<&str>,
    params: impl serde::Serialize,
) -> T {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("authorization", "Bearer server-token");
    if let Some(target) = target {
        request = request.header("x-talos-computer", target)
    }
    let response = router
        .oneshot(
            request
                .body(Body::from(serde_json::to_vec(&params).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    match serde_json::from_slice::<ApiResponse<T>>(&body).unwrap() {
        ApiResponse::Success { data } => data,
        ApiResponse::Error { error } => panic!(
            "Product API {path} ({}) failed with {:?}",
            std::any::type_name::<T>(),
            error.code
        ),
    }
}
#[tokio::test]
async fn server_routes_personal_product_and_preview_without_copying_its_business_data() {
    let server = runtime("relay-server-product", NodeProductOwner::Server).await;
    let host = runtime("relay-personal-product", NodeProductOwner::Computer).await;
    let actor = talos_auth::Actor::regular("anonymous").with_username(Some("anonymous".into()));
    let created = server
        .node_service
        .as_ref()
        .expect("Server authority")
        .create_node(
            &actor,
            CreateNodeRequest {
                display_name: "Personal product".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let credential = NodeAuth {
        node_id: created.credential.node_id,
        token: created.credential.token,
    };
    let principal = server
        .node_service
        .as_ref()
        .expect("Server authority")
        .authenticate_registration(&credential)
        .await
        .unwrap();
    let registration = server
        .node_service
        .as_ref()
        .expect("Server authority")
        .register(
            &principal,
            RegisterParams {
                product_owner: NodeProductOwner::Computer,
                deployments: vec![Deployment {
                    agent_id: "default".into(),
                }],
                system_info: None,
            },
        )
        .await
        .unwrap();
    let relay = talos_relay_server::RelayServer::new(
        server.node_service.clone().expect("Server authority"),
    );
    let product = Arc::new(
        crate::server_product_routing::ServerProductRouter::with_relay(
            server.node_service.clone().expect("Server authority"),
            server.service.clone(),
            server.host_id.clone(),
            relay.clone(),
        ),
    );
    let shutdown = CancellationToken::new();
    let server_router = http(&server, "server-token")
        .with_product_router(product.clone())
        .with_routes(relay.routes(shutdown.clone()))
        .router();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let served = server_router.clone();
    let ending = shutdown.clone();
    let listener_task = tokio::spawn(async move {
        axum::serve(listener, served)
            .with_graceful_shutdown(ending.cancelled_owned())
            .await
            .unwrap()
    });
    let (leases_tx, leases) = tokio::sync::watch::channel(Some(AgentNodeLeaseUpdate {
        auth: NodeAuth {
            node_id: credential.node_id.clone(),
            token: registration.lease_token,
        },
        owner_user_id: actor.user_id.clone(),
        validity: registration.validity,
        deadline: tokio::time::Instant::now() + Duration::from_secs(300),
    }));
    // The fixture creates the registration directly; mirror its admitted local account.
    *host.host.local_owner.write().unwrap() = Some(actor.user_id.clone());
    let mut client = crate::relay_host::spawn(
        endpoint.clone(),
        AgentTlsTrust::SystemRoots,
        host.host_id.clone(),
        leases,
        http(&host, "native-token").router(),
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let request = Request::builder()
                .uri("/v1/files")
                .body(Body::empty())
                .unwrap();
            if relay
                .forward(&actor, &credential.node_id, request)
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // Product selectors name the registered computer outside, and its protected
    // embedded Worker inside. REST and generic dispatch share this translation.
    for query in [String::new(), format!("?nodeId={}", credential.node_id)] {
        let response = server_router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/workspace/info{query}"))
                    .header("authorization", "Bearer server-token")
                    .header("x-talos-computer", &credential.node_id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let info: ApiResponse<WorkspaceInfoResponse> = serde_json::from_slice(&bytes).unwrap();
        let ApiResponse::Success { data } = info else {
            panic!("Personal workspace was not routed to its embedded Worker")
        };
        assert_eq!(
            data.user_workspace_dir,
            host.service
                .workspace_info(&actor, WorkspaceInfoRequest::default())
                .await
                .unwrap()
                .user_workspace_dir
        );
    }
    let _: WorkspaceInfoResponse = call(
        server_router.clone(),
        "/v1/app/request",
        Some(&credential.node_id),
        talos_app_protocol::requests::ClientRequestParams::WorkspaceInfo(WorkspaceInfoRequest {
            node_id: Some(credential.node_id.clone()),
        }),
    )
    .await;
    let file = host
        .service
        .files
        .upload(
            &actor,
            FileUploadRequest {
                filename: "picture.png".into(),
                scope: FileScope::Personal,
                tags: vec![],
                metadata: Default::default(),
            },
            Box::pin(futures_util::stream::iter([Ok(
                axum::body::Bytes::from_static(b"\x89PNG\r\n\x1a\n"),
            )])),
        )
        .await
        .unwrap()
        .file;
    assert!(server
        .service
        .files
        .read(
            &actor,
            FileReadRequest {
                file_id: file.file_id.clone()
            }
        )
        .await
        .is_err());
    use talos_sdk::GatewayClient;
    let mut source = talos_sdk::AppServerClient::connect_source_product_http(
        &endpoint,
        "server-token".into(),
        host.host_id.clone(),
        None,
    )
    .unwrap();
    let cloud_file = server
        .service
        .files
        .upload(
            &actor,
            FileUploadRequest {
                filename: "cloud-source.png".into(),
                scope: FileScope::Personal,
                tags: vec![],
                metadata: Default::default(),
            },
            Box::pin(futures_util::stream::iter([Ok(
                axum::body::Bytes::from_static(b"\x89PNG\r\n\x1a\ncloud-source"),
            )])),
        )
        .await
        .unwrap()
        .file;
    let mut cloud_source = talos_sdk::AppServerClient::connect_source_product_http(
        &endpoint,
        "server-token".into(),
        server.host_id.clone(),
        None,
    )
    .unwrap();
    assert_eq!(
        cloud_source
            .read_file_content(FileReadRequest {
                file_id: cloud_file.file_id
            })
            .await
            .unwrap()
            .data
            .as_ref(),
        b"\x89PNG\r\n\x1a\ncloud-source"
    );
    let from_source = source
        .read_file(FileReadRequest {
            file_id: file.file_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(from_source.file.file_id, file.file_id);
    assert_eq!(
        source
            .read_product_file_range(&file.file_id, 1..4)
            .await
            .unwrap()
            .data
            .as_ref(),
        b"PNG"
    );
    assert!(source
        .read_product_file_range(&file.file_id, 0..u64::from(FILE_PREVIEW_MAX_READ_BYTES) + 1)
        .await
        .is_err());
    let full = source
        .read_file_content(FileReadRequest {
            file_id: file.file_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(full.data.as_ref(), b"\x89PNG\r\n\x1a\n");
    let foreign = host
        .service
        .files
        .upload(
            &talos_auth::Actor::regular("foreign-file-owner"),
            FileUploadRequest {
                filename: "foreign.png".into(),
                scope: FileScope::Personal,
                tags: vec![],
                metadata: Default::default(),
            },
            Box::pin(futures_util::stream::iter([Ok(
                axum::body::Bytes::from_static(b"\x89PNG\r\n\x1a\nprivate"),
            )])),
        )
        .await
        .unwrap()
        .file;
    assert!(source
        .read_file(FileReadRequest {
            file_id: foreign.file_id.clone()
        })
        .await
        .is_err());
    assert!(source
        .update_file(FileUpdateRequest {
            file_id: foreign.file_id.clone(),
            tags: Some(vec!["forbidden".into()]),
            metadata: None
        })
        .await
        .is_err());
    assert!(source
        .delete_file(FileDeleteRequest {
            file_id: foreign.file_id
        })
        .await
        .is_err());
    let mut foreign_source = talos_sdk::AppServerClient::connect_source_product_http(
        &endpoint,
        "foreign-token".into(),
        host.host_id.clone(),
        None,
    )
    .unwrap();
    assert!(foreign_source
        .read_file(FileReadRequest {
            file_id: file.file_id.clone()
        })
        .await
        .is_err());
    let forged = http(&host, "native-token")
        .router()
        .oneshot(
            Request::builder()
                .uri("/v1/files")
                .header("authorization", "Bearer server-token")
                .header("x-talos-relay-identity", "anonymous")
                .header("x-talos-actor", "anonymous")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forged.status(), axum::http::StatusCode::UNAUTHORIZED);
    let changed = source
        .update_file(FileUpdateRequest {
            file_id: file.file_id.clone(),
            tags: Some(vec!["source-peer-update".into()]),
            metadata: None,
        })
        .await
        .unwrap();
    assert_eq!(changed.file.tags, vec!["source-peer-update"]);
    let updated: FileUpdateResponse = call(
        server_router.clone(),
        "/v1/app/request",
        Some(&credential.node_id),
        talos_app_protocol::requests::ClientRequestParams::FileUpdate(FileUpdateRequest {
            file_id: file.file_id.clone(),
            tags: Some(vec!["changed-through-server".into()]),
            metadata: None,
        }),
    )
    .await;
    assert_eq!(updated.file.tags, vec!["changed-through-server"]);
    assert_eq!(
        host.service
            .files
            .read(
                &actor,
                FileReadRequest {
                    file_id: file.file_id.clone()
                }
            )
            .await
            .unwrap()
            .file
            .tags,
        vec!["changed-through-server"]
    );
    let descriptor: FilePreviewDescriptor = call(
        server_router.clone(),
        "/v1/app/request",
        Some(&credential.node_id),
        talos_app_protocol::requests::ClientRequestParams::FilePreviewPrepare(
            FilePreviewPrepareRequest {
                source: FilePreviewSource::File {
                    node_id: None,
                    file_id: file.file_id.clone(),
                },
            },
        ),
    )
    .await;
    let base = reqwest::Url::parse(descriptor.resource_base_url.as_ref().unwrap()).unwrap();
    let routing =
        talos_gateway_http::PersonalPreviewRoute::new(&credential.node_id, &descriptor.preview_id)
            .unwrap();
    assert_eq!(
        base.host_str().unwrap(),
        format!("{}.preview.example.test", routing.host_label())
    );
    assert_eq!(base.path(), "/");
    let response = server_router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("{}{}", base.path(), descriptor.entry_path))
                .header("host", base.host_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        b"\x89PNG\r\n\x1a\n"
    );
    let other = talos_auth::Actor::regular("another-account");
    assert!(product
        .route(
            other,
            credential.node_id.clone(),
            Request::builder()
                .uri("/v1/files")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .is_err());
    assert!(relay
        .forward_preview(
            &credential.node_id,
            Request::builder()
                .method("POST")
                .uri("/v1/file-previews/resources/guess/path")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .is_err());
    let closed: FilePreviewCloseResponse = call(
        server_router.clone(),
        "/v1/file-previews/close",
        Some(&credential.node_id),
        FilePreviewCloseRequest {
            preview_id: descriptor.preview_id.clone(),
        },
    )
    .await;
    assert!(closed.closed);
    let response = server_router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("{}{}", base.path(), descriptor.entry_path))
                .header("host", base.host_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    assert!(server
        .service
        .files
        .read(
            &actor,
            FileReadRequest {
                file_id: file.file_id.clone()
            }
        )
        .await
        .is_err());
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("index.html"),
        br#"<html><script src="/asset.js"></script></html>"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("asset.js"),
        b"window.relayAssetLoaded=true;",
    )
    .unwrap();
    host.session_store
        .insert_session_record(&talos_store::session::SessionRecord {
            id: "personal-html-preview".into(),
            session_kind: SessionKind::Standard,
            agent_id: "talos".into(),
            execution_node_id: "embedded".into(),
            tree_identity: talos_store::session::SessionTreeIdentity::Primary,
            workspace_id: "default".into(),
            title: "HTML preview".into(),
            user_id: actor.user_id.clone(),
            source_type: talos_types::talos::SessionSourceType::System,
            source_provider: talos_types::talos::SessionSourceProvider::System,
            channel_id: None,
            thread_id: None,
            environment: None,
            created_at: 1,
            last_activity: 1,
            message_count: 0,
            latest_turn_seq: 0,
            memory_processed_until_event_seq: 0,
            usage: Default::default(),
            conf: Default::default(),
            project_id: None,
            current_directory: Some(directory.path().to_owned()),
            worktree_binding: None,
            pinned: false,
        })
        .await
        .unwrap();
    let html: FilePreviewDescriptor = call(
        server_router.clone(),
        "/v1/file-previews",
        Some(&credential.node_id),
        FilePreviewPrepareRequest {
            source: FilePreviewSource::SessionPath {
                session_id: "personal-html-preview".into(),
                path: "index.html".into(),
            },
        },
    )
    .await;
    let origin = reqwest::Url::parse(html.resource_base_url.as_ref().unwrap()).unwrap();
    let response = server_router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/asset.js")
                .header("host", origin.host_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        b"window.relayAssetLoaded=true;"
    );
    let forged = talos_gateway_http::PersonalPreviewRoute::new(
        &credential.node_id,
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    let response = server_router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/asset.js")
                .header(
                    "host",
                    format!("{}.preview.example.test", forged.host_label()),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    #[cfg(unix)]
    {
        let oauth_home = tempfile::tempdir().unwrap();
        let _issuer = super::mcp_credentials_tests::HttpPeer(
            std::process::Command::new("/usr/bin/python3")
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../action/mcp/tests/fixtures/oauth_server.py"),
                )
                .arg(oauth_home.path())
                .spawn()
                .unwrap(),
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while !oauth_home.path().join("port").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let issuer = format!(
            "http://127.0.0.1:{}/bearer",
            fs::read_to_string(oauth_home.path().join("port")).unwrap()
        );
        let imported = host
            .worker
            .as_ref()
            .unwrap()
            .plugin_manager()
            .import_mcp_server(McpServerImportRequest {
                node_id: None,
                json: serde_json::json!({"name":"relayed_oauth","url":issuer}).to_string(),
                name: None,
                enable: Some(true),
                conflict_policy: PluginInstallConflictPolicy::Reject,
            })
            .await
            .unwrap();
        let started: McpAuthenticationStartResponse = call(
            server_router.clone(),
            "/v1/app/request",
            Some(&credential.node_id),
            talos_app_protocol::requests::ClientRequestParams::McpAuthenticationStart(
                McpAuthenticationStartRequest {
                    node_id: None,
                    server: McpDefinitionRef::Plugin {
                        install_id: imported.install.id.clone(),
                    },
                    config: McpOAuthConfig {
                        scopes: vec!["read".into()],
                        ..Default::default()
                    },
                    client_secret: None,
                },
            ),
        )
        .await;
        let browser = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let authorization = browser
            .get(&started.authorization_url)
            .send()
            .await
            .unwrap();
        let callback =
            reqwest::Url::parse(authorization.headers()["location"].to_str().unwrap()).unwrap();
        let expected = reqwest::Url::parse(&server.config.cloud.agent_base_url).unwrap();
        assert_eq!(callback.host_str(), expected.host_str());
        let mut wrong = callback.clone();
        let pairs: Vec<_> = wrong
            .query_pairs()
            .map(|(key, value)| {
                (
                    key.to_string(),
                    if key == "iss" {
                        "https://wrong.invalid".into()
                    } else {
                        value.to_string()
                    },
                )
            })
            .collect();
        wrong.query_pairs_mut().clear().extend_pairs(pairs);
        let response = server_router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("{}?{}", wrong.path(), wrong.query().unwrap()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let response = server_router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("{}?{}", callback.path(), callback.query().unwrap()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            fs::read_to_string(oauth_home.path().join("exchange-count")).unwrap(),
            "1"
        );
        let server_credentials = server
            .config
            .talos_paths()
            .talos_home
            .join("mcp-credentials");
        assert!(
            !server_credentials.exists()
                || fs::read_dir(server_credentials).unwrap().next().is_none()
        );
        let host_credentials = host.config.talos_paths().talos_home.join("mcp-credentials");
        assert!(fs::read_dir(host_credentials).unwrap().next().is_some());
    }
    // Reattach the same registered Host without copying personal state to the Server.
    client.abort();
    let _ = client.await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while relay
            .resolve_source_peer(&actor, &host.host_id)
            .await
            .is_ok()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(source
        .read_file(FileReadRequest {
            file_id: file.file_id.clone()
        })
        .await
        .is_err());
    assert!(server
        .service
        .files
        .read(
            &actor,
            FileReadRequest {
                file_id: file.file_id.clone()
            }
        )
        .await
        .is_err());
    client = crate::relay_host::spawn(
        endpoint.clone(),
        AgentTlsTrust::SystemRoots,
        host.host_id.clone(),
        leases_tx.subscribe(),
        http(&host, "native-token").router(),
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if source
                .read_file(FileReadRequest {
                    file_id: file.file_id.clone(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let cloud = server
        .node_service
        .as_ref()
        .expect("Server authority")
        .create_node(
            &actor,
            CreateNodeRequest {
                display_name: "Cloud product".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let cloud_auth = NodeAuth {
        node_id: cloud.credential.node_id,
        token: cloud.credential.token,
    };
    let cloud_principal = server
        .node_service
        .as_ref()
        .expect("Server authority")
        .authenticate_registration(&cloud_auth)
        .await
        .unwrap();
    server
        .node_service
        .as_ref()
        .expect("Server authority")
        .register(
            &cloud_principal,
            RegisterParams {
                product_owner: NodeProductOwner::Server,
                deployments: vec![Deployment {
                    agent_id: "default".into(),
                }],
                system_info: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        product
            .route(
                actor.clone(),
                cloud_auth.node_id,
                Request::builder()
                    .uri("/v1/files")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap(),
        ProductRoute::Local(_)
    ));
    source
        .delete_file(FileDeleteRequest {
            file_id: file.file_id.clone(),
        })
        .await
        .unwrap();
    assert!(host
        .service
        .files
        .read(
            &actor,
            FileReadRequest {
                file_id: file.file_id.clone()
            }
        )
        .await
        .is_err());
    let retained_file = host
        .service
        .files
        .upload(
            &actor,
            FileUploadRequest {
                filename: "retained-after-revoke.png".into(),
                scope: FileScope::Personal,
                tags: vec![],
                metadata: Default::default(),
            },
            Box::pin(futures_util::stream::iter([Ok(
                axum::body::Bytes::from_static(b"\x89PNG\r\n\x1a\n"),
            )])),
        )
        .await
        .unwrap()
        .file;
    assert_eq!(
        source
            .read_file(FileReadRequest {
                file_id: retained_file.file_id.clone()
            })
            .await
            .unwrap()
            .file
            .file_id,
        retained_file.file_id
    );
    let lease = leases_tx.borrow().as_ref().unwrap().auth.clone();
    let principal = server
        .node_service
        .as_ref()
        .expect("Server authority")
        .authenticate_lease(&lease)
        .await
        .unwrap();
    server
        .node_service
        .as_ref()
        .expect("Server authority")
        .unregister(&principal)
        .await
        .unwrap();
    let revoked = source
        .read_file(FileReadRequest {
            file_id: retained_file.file_id.clone(),
        })
        .await
        .unwrap_err();
    assert_eq!(
        revoked.error_code(),
        talos_error::ErrorCode::ResourceNotFound
    );
    assert_eq!(
        host.service
            .files
            .read_content(
                &actor,
                FileReadRequest {
                    file_id: retained_file.file_id,
                }
            )
            .await
            .unwrap()
            .bytes
            .as_ref(),
        b"\x89PNG\r\n\x1a\n"
    );
    leases_tx.send(None).unwrap();
    client.abort();
    shutdown.cancel();
    listener_task.await.unwrap();
    host.shutdown().await.unwrap();
    server.shutdown().await.unwrap();
}
