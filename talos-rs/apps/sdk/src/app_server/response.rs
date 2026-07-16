use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use tokio::sync::{oneshot, Mutex};

pub(super) async fn fail_pending_responses(
    pending_responses: &Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    message: String,
) {
    let pending = {
        let mut guard = pending_responses.lock().await;
        std::mem::take(&mut *guard)
    };
    for (_, sender) in pending {
        let _ = sender.send(Err(TalosError::ClientError(message.clone())));
    }
}
