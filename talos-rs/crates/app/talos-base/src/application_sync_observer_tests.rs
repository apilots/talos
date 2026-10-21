use super::*;

#[tokio::test]
async fn stopping_account_observers_cancels_pending_recovery_and_releases_resources() {
    let mut observers = SyncObservers::default();
    let (released, observed) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _resource = released;
        std::future::pending::<()>().await;
    });
    observers.tasks.insert("account".into(), task);
    join_observer(observers.remove("account").unwrap()).await;
    assert!(
        observed.await.is_err(),
        "account shutdown must release recovery resources"
    );
    assert!(observers.tasks.is_empty());

    let (released, observed) = tokio::sync::oneshot::channel::<()>();
    observers.tasks.insert(
        "other-account".into(),
        tokio::spawn(async move {
            let _resource = released;
            std::future::pending::<()>().await;
        }),
    );
    drop(observers);
    tokio::time::timeout(std::time::Duration::from_secs(1), observed)
        .await
        .expect("owner drop must cancel remaining observers")
        .unwrap_err();
}
