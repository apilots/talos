use super::*;

#[test]
fn coordinator_global_capacity_is_reclaimed_only_after_last_owner() {
    let admission = Arc::new(Mutex::new(Admission::default()));
    let mut permits = (0..MAX_CONNECTIONS)
        .map(|_| ConnectionPermit::acquire(&admission).unwrap())
        .collect::<Vec<_>>();
    assert!(ConnectionPermit::acquire(&admission)
        .err()
        .unwrap()
        .is_code(ErrorCode::ServiceUnavailable));
    let mut permit = permits.pop().unwrap();
    permit.admit_account("user-1").unwrap();
    let monitor = Arc::new(permit);
    let stop = CancellationToken::new();
    let output = AdmittedStream {
        inner: futures_util::stream::pending::<()>(),
        _permit: monitor.clone(),
        stop: stop.clone(),
    };
    drop(output);
    assert!(stop.is_cancelled());
    assert!(ConnectionPermit::acquire(&admission).is_err());
    drop(monitor);
    let replacement = ConnectionPermit::acquire(&admission).unwrap();
    drop((permits, replacement));
    let counts = admission.lock().unwrap();
    assert_eq!(counts.total, 0);
    assert!(counts.accounts.is_empty());
}
