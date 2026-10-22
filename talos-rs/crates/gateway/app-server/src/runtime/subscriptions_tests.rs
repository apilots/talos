use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct Released(Arc<AtomicBool>);
impl Drop for Released {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn completed_streams_release_registry_entries_and_dropped_owners_abort_pending_streams() {
    let subscriptions = Subscriptions::new(16);
    let (finish, completion) = tokio::sync::oneshot::channel();
    let task = owned_subscription(&subscriptions.state, "completed".into(), async move {
        let _ = completion.await;
    });
    subscriptions
        .state
        .lock()
        .unwrap()
        .tasks
        .insert("completed".into(), task);
    finish.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !subscriptions.state.lock().unwrap().tasks.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("completed streams must not accumulate task handles");

    let released = Arc::new(AtomicBool::new(false));
    let resource = Released(released.clone());
    let task = owned_subscription(&subscriptions.state, "pending".into(), async move {
        let _resource = resource;
        std::future::pending::<()>().await;
    });
    subscriptions
        .state
        .lock()
        .unwrap()
        .tasks
        .insert("pending".into(), task);
    drop(subscriptions);
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !released.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("dropping the connection must drop pending stream resources");
}
