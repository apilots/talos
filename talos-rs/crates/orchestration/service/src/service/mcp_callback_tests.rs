use super::*;
use crate::McpPersonalCallback;
use agent_protocol::McpOAuthCallbackOutcome;

struct Callback {
    calls: Arc<Mutex<Vec<(String, String, String)>>>,
    invalid: std::sync::atomic::AtomicBool,
}
#[async_trait]
impl McpPersonalCallback for Callback {
    async fn complete(
        &self,
        actor: &crate::Actor,
        computer: &str,
        state: String,
        _issuer: Option<String>,
        _outcome: McpOAuthCallbackOutcome,
    ) -> TalosResult<()> {
        assert!(actor.access_token.is_none());
        if self
            .invalid
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Callback did not pass validation.",
            ));
        }
        self.calls
            .lock()
            .unwrap()
            .push((actor.user_id.clone(), computer.into(), state));
        Ok(())
    }
}
fn response(state: &str, expires_at: i64) -> talos_types::McpAuthenticationStartResponse {
    talos_types::McpAuthenticationStartResponse {
        server: talos_types::McpDefinitionRef::Plugin {
            install_id: talos_types::PluginInstallId("callback-plugin".into()),
        },
        attempt_id: "attempt".into(),
        authorization_url: format!("https://provider.example/authorize?state={state}"),
        expires_at,
    }
}
#[tokio::test]
async fn personal_callback_uses_recorded_identity_and_original_state_once() {
    let (service, _) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        TalosConfig::default(),
    )
    .await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let callback: Arc<dyn McpPersonalCallback> = Arc::new(Callback {
        calls: calls.clone(),
        invalid: std::sync::atomic::AtomicBool::new(false),
    });
    let mut actor = crate::Actor::regular("alice");
    actor.access_token = Some("must-not-retain".into());
    service
        .remember_personal_mcp_callback(
            &actor,
            "original-computer".into(),
            &response("original-state", chrono::Utc::now().timestamp() + 300),
            &callback,
        )
        .unwrap();
    service
        .complete_mcp_oauth_callback(
            "original-state".into(),
            None,
            McpOAuthCallbackOutcome::AuthorizationCode("code".into()),
        )
        .await
        .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        vec![(
            "alice".into(),
            "original-computer".into(),
            "original-state".into()
        )]
    );
    assert!(service
        .complete_mcp_oauth_callback(
            "original-state".into(),
            None,
            McpOAuthCallbackOutcome::AuthorizationCode("code".into())
        )
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::ResourceNotFound));
}
#[tokio::test]
async fn personal_callback_rejects_expiry_and_preserves_an_invalid_attempt() {
    let (service, _) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        TalosConfig::default(),
    )
    .await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let callback: Arc<dyn McpPersonalCallback> = Arc::new(Callback {
        calls: calls.clone(),
        invalid: std::sync::atomic::AtomicBool::new(true),
    });
    let actor = crate::Actor::regular("alice");
    let now = chrono::Utc::now().timestamp();
    assert!(service
        .remember_personal_mcp_callback(
            &actor,
            "computer".into(),
            &response("expired", now - 1),
            &callback
        )
        .is_err());
    assert!(service
        .remember_personal_mcp_callback(
            &actor,
            "computer".into(),
            &response("too-far", now + 601),
            &callback
        )
        .is_err());
    service
        .remember_personal_mcp_callback(
            &actor,
            "computer".into(),
            &response("state", now + 300),
            &callback,
        )
        .unwrap();
    assert!(service
        .complete_mcp_oauth_callback(
            "state".into(),
            None,
            McpOAuthCallbackOutcome::AuthorizationCode("invalid".into())
        )
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::InvalidRequest));
    service
        .complete_mcp_oauth_callback(
            "state".into(),
            None,
            McpOAuthCallbackOutcome::AuthorizationCode("valid".into()),
        )
        .await
        .unwrap();
    assert_eq!(calls.lock().unwrap().len(), 1);
}
