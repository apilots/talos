//! Connection-owned local Timeline subscriptions, independent of Worker RPC waits.
use super::{
    forward_subscription, send_application_error, send_result, AppServerApplication,
    OutgoingMessage,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use talos_app_protocol::{
    errors::AppServerErrorBody,
    jsonrpc::{JsonRpcResponse, RequestId},
};
use talos_service::Actor;
use talos_types::{EventUnsubscribeRequest, EventUnsubscribeResponse, SubscribeTimelineRequest};

#[derive(Default)]
struct State {
    closed: bool,
    tasks: HashMap<String, tokio::task::JoinHandle<()>>,
}

pub(super) struct Subscriptions {
    state: Arc<Mutex<State>>,
    capacity: usize,
}

impl Subscriptions {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            capacity,
        }
    }

    pub(super) async fn subscribe(
        &self,
        app: &dyn AppServerApplication,
        actor: &Actor,
        params: SubscribeTimelineRequest,
        outgoing: tokio::sync::mpsc::Sender<OutgoingMessage>,
        id: RequestId,
    ) -> Result<(), ()> {
        match app.subscribe_timeline_events(actor, params).await {
            Ok(subscription) => {
                let response = subscription.response();
                let subscription_id = response.subscription_id.clone();
                let body = match serde_json::to_value(response) {
                    Ok(value) => super::api_success_value(value)?,
                    Err(error) => {
                        return send_result(
                            outgoing,
                            id,
                            Err(AppServerErrorBody::invalid_request(error.to_string())),
                        )
                        .await;
                    }
                };
                let reply = outgoing.reserve().await.map_err(|_| ())?;
                let mut state = self.state.lock().expect("subscription tasks lock");
                // Publish the acknowledgement and task under the same lock so an
                // immediate concurrent unsubscribe cannot miss the new task.
                reply.send(OutgoingMessage::Response(JsonRpcResponse::new(id, body)));
                if !state.closed {
                    let task = forward_subscription(
                        subscription_id.clone(),
                        subscription,
                        outgoing.clone(),
                        self.capacity,
                    );
                    let task = owned_subscription(&self.state, subscription_id.clone(), task);
                    state.tasks.insert(subscription_id, task);
                }
                Ok(())
            }
            Err(error) => send_application_error(outgoing, id, error).await,
        }
    }

    pub(super) async fn unsubscribe(
        &self,
        params: EventUnsubscribeRequest,
        outgoing: tokio::sync::mpsc::Sender<OutgoingMessage>,
        id: RequestId,
    ) -> Result<(), ()> {
        let task = self
            .state
            .lock()
            .expect("subscription tasks lock")
            .tasks
            .remove(&params.subscription_id);
        let unsubscribed = task.is_some();
        if let Some(task) = task {
            stop(task).await;
        }
        send_result(
            outgoing,
            id,
            serde_json::to_value(EventUnsubscribeResponse {
                subscription_id: params.subscription_id,
                unsubscribed,
            })
            .map_err(|error| AppServerErrorBody::invalid_request(error.to_string())),
        )
        .await
    }

    pub(super) async fn shutdown(&self) {
        let tasks = {
            let mut state = self.state.lock().expect("subscription tasks lock");
            state.closed = true;
            std::mem::take(&mut state.tasks)
        };
        for (_, task) in tasks {
            stop(task).await;
        }
    }
}

async fn stop(task: tokio::task::JoinHandle<()>) {
    task.abort();
    if let Err(error) = task.await {
        if !error.is_cancelled() {
            talos_logging::report_error(&talos_error::TalosError::internal_with_source(
                "The event subscription task failed.",
                error,
            ));
        }
    }
}

// The task only holds a weak owner, so dropping a connection aborts all streams.
impl Drop for State {
    fn drop(&mut self) {
        for task in self.tasks.values() {
            task.abort();
        }
    }
}

struct SubscriptionCompletion {
    owner: Weak<Mutex<State>>,
    id: String,
}
impl Drop for SubscriptionCompletion {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            owner
                .lock()
                .expect("subscription tasks lock")
                .tasks
                .remove(&self.id);
        }
    }
}

fn owned_subscription(
    owner: &Arc<Mutex<State>>,
    id: String,
    stream: impl std::future::Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    let completion = SubscriptionCompletion {
        owner: Arc::downgrade(owner),
        id,
    };
    tokio::spawn(async move {
        let _completion = completion;
        stream.await;
    })
}

#[cfg(test)]
#[path = "subscriptions_tests.rs"]
mod tests;
