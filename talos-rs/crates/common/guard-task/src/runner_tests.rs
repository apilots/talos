use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, Notify};

struct FunctionTask<F>(F);
impl<F, R> GuardTask for FunctionTask<F>
where
    F: Fn(GuardTaskContext) -> R + Send + Sync + 'static,
    R: Future<Output = TalosResult<()>> + Send,
{
    async fn run(&self, context: GuardTaskContext) -> TalosResult<()> {
        (self.0)(context).await
    }
}

fn spec(name: &'static str) -> GuardTaskSpec {
    GuardTaskSpec {
        name,
        interval: Duration::from_secs(10),
        run_on_start: true,
    }
}

async fn flush() {
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn startup_periodic_and_independent_cadences() {
    let tasks = GuardTasks::default();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for (name, interval) in [("fast", 5), ("slow", 10)] {
        let tx = tx.clone();
        tasks
            .register(
                GuardTaskSpec {
                    interval: Duration::from_secs(interval),
                    ..spec(name)
                },
                FunctionTask(move |context: GuardTaskContext| {
                    let tx = tx.clone();
                    async move {
                        tx.send((name, context.reason())).unwrap();
                        Ok(())
                    }
                }),
            )
            .unwrap();
    }
    flush().await;
    assert!(matches!(
        rx.recv().await.unwrap().1,
        GuardTaskReason::Startup
    ));
    assert!(matches!(
        rx.recv().await.unwrap().1,
        GuardTaskReason::Startup
    ));
    tokio::time::advance(Duration::from_secs(5)).await;
    flush().await;
    assert_eq!(rx.try_recv().unwrap().0, "fast");
    assert!(rx.try_recv().is_err());
    tokio::time::advance(Duration::from_secs(5)).await;
    flush().await;
    let mut names = [rx.try_recv().unwrap().0, rx.try_recv().unwrap().0];
    names.sort();
    assert_eq!(names, ["fast", "slow"]);
    tasks.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn interval_update_wake_and_closed_registration() {
    let tasks = GuardTasks::default();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = tasks
        .register(
            GuardTaskSpec {
                run_on_start: false,
                ..spec("job")
            },
            FunctionTask(move |_| {
                let tx = tx.clone();
                async move {
                    tx.send(()).unwrap();
                    Ok(())
                }
            }),
        )
        .unwrap();
    flush().await;
    assert!(rx.try_recv().is_err());
    handle.wake();
    flush().await;
    rx.try_recv().unwrap();
    handle.set_interval(Duration::from_secs(2)).unwrap();
    flush().await;
    assert!(rx.try_recv().is_err());
    tokio::time::advance(Duration::from_secs(2)).await;
    flush().await;
    rx.try_recv().unwrap();
    tasks.shutdown().await.unwrap();
    handle.wake();
    assert!(tasks
        .register(spec("new"), FunctionTask(|_| async { Ok(()) }))
        .is_err());
    assert!(rx.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn no_overlap_or_catch_up_after_a_long_round() {
    let tasks = GuardTasks::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());
    let count = calls.clone();
    let signal = release.clone();
    tasks
        .register(
            spec("slow"),
            FunctionTask(move |context: GuardTaskContext| {
                let count = count.clone();
                let signal = signal.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    tokio::select! { _ = signal.notified() => {}, _ = context.cancelled() => {} }
                    Ok(())
                }
            }),
        )
        .unwrap();
    flush().await;
    tokio::time::advance(Duration::from_secs(100)).await;
    flush().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    release.notify_one();
    flush().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_secs(10)).await;
    flush().await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    tasks.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn panic_restarts_with_backoff_and_errors_keep_the_schedule() {
    let tasks = GuardTasks::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    tasks
        .register(
            spec("fault"),
            FunctionTask(move |_| {
                let counter = counter.clone();
                async move {
                    match counter.fetch_add(1, Ordering::SeqCst) {
                        0 => panic!("injected task panic"),
                        1 => Err(TalosError::internal("Injected business failure.")),
                        _ => Ok(()),
                    }
                }
            }),
        )
        .unwrap();
    flush().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    flush().await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    tokio::time::advance(Duration::from_secs(10)).await;
    flush().await;
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    tasks.shutdown().await.unwrap();
}

#[tokio::test]
async fn shared_shutdown_waits_for_safe_round_and_survives_caller_abort() {
    let tasks = Arc::new(GuardTasks::default());
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (first, second, third) = (started.clone(), cancelled.clone(), release.clone());
    tasks
        .register(
            spec("commit"),
            FunctionTask(move |context: GuardTaskContext| {
                let (first, second, third) = (first.clone(), second.clone(), third.clone());
                async move {
                    first.notify_one();
                    context.cancelled().await;
                    second.notify_one();
                    third.notified().await;
                    Ok(())
                }
            }),
        )
        .unwrap();
    started.notified().await;
    let owner = tasks.clone();
    let first_stop = tokio::spawn(async move { owner.shutdown().await });
    cancelled.notified().await;
    first_stop.abort();
    assert!(first_stop.await.unwrap_err().is_cancelled());
    let owner = tasks.clone();
    let second_stop = tokio::spawn(async move { owner.shutdown().await });
    flush().await;
    assert!(!second_stop.is_finished());
    release.notify_one();
    second_stop.await.unwrap().unwrap();
    tasks.shutdown().await.unwrap();
}

#[tokio::test]
async fn duplicate_names_and_invalid_intervals_fail_before_spawning() {
    let tasks = GuardTasks::default();
    let task = || FunctionTask(|_| async { Ok(()) });
    tasks.register(spec("same"), task()).unwrap();
    assert!(tasks.register(spec("same"), task()).is_err());
    assert!(tasks
        .register(
            GuardTaskSpec {
                interval: Duration::ZERO,
                ..spec("zero")
            },
            task()
        )
        .is_err());
    assert!(tasks
        .register(
            GuardTaskSpec {
                interval: Duration::MAX,
                ..spec("overflow")
            },
            task()
        )
        .is_err());
    tasks.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn shutdown_interrupts_panic_backoff() {
    let tasks = GuardTasks::default();
    tasks
        .register(
            spec("panic"),
            FunctionTask(|_| async {
                std::future::ready(()).await;
                panic!("injected")
            }),
        )
        .unwrap();
    flush().await;
    let before = tokio::time::Instant::now();
    tasks.shutdown().await.unwrap();
    assert_eq!(tokio::time::Instant::now(), before);
}

#[test]
fn registration_without_a_runtime_fails_explicitly() {
    let tasks = GuardTasks::default();
    assert!(tasks
        .register(spec("no-runtime"), FunctionTask(|_| async { Ok(()) }))
        .is_err());
}
