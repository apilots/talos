//! Tree deletion retires every source, including safe retry after partial cleanup.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct RetirementFault {
    delegate: Arc<TestAgentCoordinator>,
    enabled: Arc<AtomicBool>,
    calls: AtomicUsize,
}

#[async_trait]
impl AgentCoordinator for RetirementFault {
    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        self.delegate.resolve_agent(params).await
    }
    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()> {
        self.delegate.ensure_agent(agent_id).await
    }
    async fn select_execution_node(
        &self,
        agent_id: &str,
        user_id: &str,
        preferred: Option<&str>,
    ) -> TalosResult<String> {
        self.delegate
            .select_execution_node(agent_id, user_id, preferred)
            .await
    }
    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        self.delegate.start(params).await
    }
    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        self.delegate.submit_input(params).await
    }
    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        self.delegate.interrupt(params).await
    }
    async fn compact_session(&self, context: AgentContext) -> TalosResult<AgentRunExecution> {
        self.delegate.compact_session(context).await
    }
    async fn clear_session(
        &self,
        scope: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        if self.enabled.load(Ordering::SeqCst) && self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
            return Err(TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "injected child retirement failure",
            ));
        }
        self.delegate.clear_session(scope, expected).await
    }
    async fn close_session(
        &self,
        scope: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        self.delegate.close_session(scope, expected).await
    }
    async fn shutdown(&self, timeout: Duration) -> TalosResult<()> {
        self.delegate.shutdown(timeout).await
    }
}

async fn tree(fixture: &Fixture) -> Vec<Source> {
    let root = fixture.finished_source("tree-root").await;
    let mut child = fixture
        .product
        .get_session_record("tree-root")
        .await
        .unwrap()
        .unwrap();
    child.id = "tree-child".into();
    child.tree_identity = talos_types::SessionTreeIdentity::Subagent {
        parent: root.identity.clone(),
        tree_root_session_id: root.identity.session_id.clone(),
        path: "/root/child".parse().unwrap(),
    };
    fixture.product.insert_session_record(&child).await.unwrap();
    let child_source = fixture.finished_source("tree-child").await;
    child.id = "tree-grandchild".into();
    child.tree_identity = talos_types::SessionTreeIdentity::Subagent {
        parent: child_source.identity.clone(),
        tree_root_session_id: root.identity.session_id.clone(),
        path: "/root/child/grandchild".parse().unwrap(),
    };
    fixture.product.insert_session_record(&child).await.unwrap();
    let grandchild = fixture.finished_source("tree-grandchild").await;
    vec![root, child_source, grandchild]
}

async fn assert_tree_removed(fixture: &Fixture, sources: &[Source]) {
    for source in sources {
        fixture.assert_source_removed(source, true).await;
        assert!(fixture
            .product
            .get_session_record(&source.identity.session_id)
            .await
            .unwrap()
            .is_none());
        assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
    }
}

#[tokio::test]
async fn deleting_root_retires_parent_child_and_grandchild_payloads_and_checkpoints() {
    let fixture = Fixture::new(coordinator()).await;
    let sources = tree(&fixture).await;
    assert!(fixture
        .service
        .delete_session(&fixture.actor, "tree-root")
        .await
        .unwrap());
    assert_tree_removed(&fixture, &sources).await;
    fixture.service.reconcile_worker_events().await.unwrap();
    for source in &sources {
        assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
    }
}

#[tokio::test]
async fn partial_retirement_keeps_product_tree_and_retries_already_retired_sibling() {
    let enabled = Arc::new(AtomicBool::new(true));
    let fixture = Fixture::new(Arc::new(RetirementFault {
        delegate: coordinator(),
        enabled: enabled.clone(),
        calls: AtomicUsize::new(0),
    }))
    .await;
    let sources = tree(&fixture).await;
    for source in &sources {
        fixture.project(source).await;
    }
    fixture
        .service
        .delete_session(&fixture.actor, "tree-root")
        .await
        .expect_err("partial cleanup must retain product tree");
    let mut retired = 0;
    for source in &sources {
        retired += usize::from(
            fixture
                .journal
                .is_retired(&source.identity.session_id)
                .await
                .unwrap(),
        );
        assert!(fixture
            .product
            .get_session_record(&source.identity.session_id)
            .await
            .unwrap()
            .is_some());
        assert!(!fixture
            .product
            .list_agent_turn_events(&source.identity, 100)
            .await
            .unwrap()
            .is_empty());
        if fixture
            .journal
            .is_retired(&source.identity.session_id)
            .await
            .unwrap()
        {
            assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
        }
    }
    assert_eq!(
        retired, 1,
        "the first source must be retired before the next cleanup fails"
    );
    enabled.store(false, Ordering::SeqCst);
    assert!(fixture
        .service
        .delete_session(&fixture.actor, "tree-root")
        .await
        .unwrap());
    assert_tree_removed(&fixture, &sources).await;
}

#[tokio::test]
async fn clear_retries_failed_product_transaction_after_source_clear_without_rebilling() {
    let fixture = Fixture::new(coordinator()).await;
    let source = fixture.finished_source("clear-commit-failure").await;
    fixture.project(&source).await;
    let projected = fixture
        .product
        .list_agent_turn_events(&source.identity, 100)
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_product_clear BEFORE UPDATE OF history_generation ON sessions WHEN OLD.session_key='clear-commit-failure' AND NEW.history_generation<>OLD.history_generation BEGIN SELECT RAISE(FAIL,'product-clear-unavailable'); END")
        .execute(fixture.storage.pool()).await.unwrap();
    fixture
        .service
        .clear_session(&fixture.actor, &source.identity.session_id)
        .await
        .expect_err("the failed product transaction must preserve its old generation");
    fixture.assert_source_removed(&source, false).await;
    assert_eq!(
        fixture
            .product
            .list_agent_turn_events(&source.identity, 100)
            .await
            .unwrap(),
        projected
    );
    assert_eq!(
        fixture
            .product
            .get_history_cursor(&source.identity.session_id)
            .await
            .unwrap()
            .unwrap()
            .history_generation,
        0
    );
    assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
    assert!(fixture
        .product
        .pending_worker_settlements(&source.identity.session_id)
        .await
        .unwrap()
        .is_empty());
    sqlx::query("DROP TRIGGER reject_product_clear")
        .execute(fixture.storage.pool())
        .await
        .unwrap();
    fixture
        .service
        .clear_session(&fixture.actor, &source.identity.session_id)
        .await
        .unwrap();
    fixture.assert_source_removed(&source, false).await;
    assert_eq!(
        fixture
            .product
            .get_history_cursor(&source.identity.session_id)
            .await
            .unwrap()
            .unwrap()
            .history_generation,
        1
    );
    fixture.service.reconcile_worker_events().await.unwrap();
    assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
}

#[tokio::test]
async fn failed_clear_then_failed_delete_can_retry_without_exhausting_source_generation() {
    let fixture = Fixture::new(coordinator()).await;
    let source = fixture.finished_source("changed-destruction").await;
    fixture.project(&source).await;
    sqlx::query("CREATE TRIGGER reject_changed_clear BEFORE UPDATE OF history_generation ON sessions WHEN OLD.session_key='changed-destruction' AND NEW.history_generation<>OLD.history_generation BEGIN SELECT RAISE(FAIL,'clear-product-unavailable'); END")
        .execute(fixture.storage.pool()).await.unwrap();
    fixture
        .service
        .clear_session(&fixture.actor, &source.identity.session_id)
        .await
        .expect_err("Source clear may commit before the product transaction fails");
    fixture.assert_source_removed(&source, false).await;
    assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
    sqlx::query("DROP TRIGGER reject_changed_clear")
        .execute(fixture.storage.pool())
        .await
        .unwrap();

    sqlx::query("CREATE TRIGGER reject_changed_delete BEFORE DELETE ON sessions WHEN OLD.session_key='changed-destruction' BEGIN SELECT RAISE(FAIL,'delete-product-unavailable'); END")
        .execute(fixture.storage.pool()).await.unwrap();
    fixture
        .service
        .delete_session(&fixture.actor, &source.identity.session_id)
        .await
        .expect_err("retirement may commit before the product deletion fails");
    assert!(fixture
        .journal
        .is_retired(&source.identity.session_id)
        .await
        .unwrap());
    assert!(fixture
        .product
        .get_session_record(&source.identity.session_id)
        .await
        .unwrap()
        .is_some());
    assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
    sqlx::query("DROP TRIGGER reject_changed_delete")
        .execute(fixture.storage.pool())
        .await
        .unwrap();

    assert!(fixture.service.delete_session(&fixture.actor, &source.identity.session_id)
        .await.expect("changing cleanup operations must not make the retained product permanently undeletable"));
    fixture.assert_source_removed(&source, true).await;
    assert!(fixture
        .product
        .get_session_record(&source.identity.session_id)
        .await
        .unwrap()
        .is_none());
    fixture.service.reconcile_worker_events().await.unwrap();
    assert_eq!(fixture.usage(&source.identity.session_id).await, (1, 5));
}
