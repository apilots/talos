use super::*;
use std::sync::Mutex;
use talos_guard_task::{GuardTaskSpec, GuardTasks};
use tracing::field::{Field, Visit};

#[derive(Default)]
struct Events {
    batches: Mutex<Vec<(String, bool)>>,
    yielded: std::sync::atomic::AtomicBool,
}
struct Observed(Arc<Events>);
#[derive(Default)]
struct Fields {
    message: String,
    user: String,
    complete: bool,
}
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => self.message = format!("{value:?}"),
            "user_id" => self.user = format!("{value:?}"),
            _ => {}
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "user_id" {
            self.user = value.into();
        }
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "complete" {
            self.complete = value;
        }
    }
}
impl tracing::Subscriber for Observed {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields.message.contains("continuing the same round") {
            self.0
                .yielded
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if fields.message.contains("maintenance batch completed") {
            // Display fields use record_debug; normalize the simple user value.
            self.0
                .batches
                .lock()
                .unwrap()
                .push((fields.user, fields.complete));
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
struct Round {
    config: Arc<RwLock<Config>>,
    finished: Arc<tokio::sync::Notify>,
}
impl GuardTask for Round {
    async fn run(&self, context: GuardTaskContext) -> TalosResult<()> {
        let result = run_round(
            self.config.clone(),
            context,
            Duration::from_millis(1),
            Duration::from_millis(1),
        )
        .await;
        self.finished.notify_one();
        result
    }
}

#[tokio::test(start_paused = true)]
async fn one_round_retains_progress_across_global_yields_and_rotates_workspaces() {
    let root = tempfile::tempdir().unwrap();
    let config = super::tests::config(root.path());
    for user in ["alice", "bob"] {
        for index in 0..8 {
            super::tests::fixture(
                &config,
                user,
                &format!("retained-{index}"),
                "complete",
                false,
            );
        }
    }
    let expired = super::tests::fixture(&config, "bob", "expired", "complete", true);
    let events = Arc::new(Events::default());
    let _subscriber = tracing::subscriber::set_default(Observed(events.clone()));
    let finished = Arc::new(tokio::sync::Notify::new());
    let tasks = GuardTasks::default();
    tasks
        .register(
            GuardTaskSpec {
                name: "round",
                interval: Duration::from_secs(3600),
                run_on_start: true,
            },
            Round {
                config: Arc::new(RwLock::new(config)),
                finished: finished.clone(),
            },
        )
        .unwrap();
    finished.notified().await;
    tasks.shutdown().await.unwrap();
    assert!(!expired.exists());
    assert!(events.yielded.load(std::sync::atomic::Ordering::SeqCst));
    let batches = events.batches.lock().unwrap();
    assert!(batches.len() > 2);
    let (first, _) = &batches[0];
    let complete = batches
        .iter()
        .position(|(user, complete)| user == first && *complete)
        .unwrap();
    assert!(batches[..complete].iter().any(|(user, _)| user != first));
}
