use std::time::Duration;

use tokio::time::Instant;

use super::*;

#[test]
fn lease_deadline_is_based_on_request_send_time() {
    let sent_at = Instant::now();
    let deadline = local_deadline(sent_at, 190, 100).unwrap();
    assert_eq!(deadline, sent_at + Duration::from_secs(90));
    assert!(local_deadline(sent_at, 100, 100).is_err());
}
