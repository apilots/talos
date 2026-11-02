use std::time::Duration;

use serde::de::DeserializeOwned;
use talos_error::{ErrorCode, TalosError, TalosResult};

use crate::runtimes::web_common::DEFAULT_WEB_USER_AGENT;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

pub(super) fn search_client(timeout: Duration) -> TalosResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(DEFAULT_WEB_USER_AGENT)
        .timeout(timeout)
        .build()
        .map_err(|err| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Build web search client failed.",
                err.without_url(),
            )
        })
}

/// All search providers decode through this boundary. Reject HTTP errors without
/// reading their bodies; accumulate at most 1 MiB of a successful response before
/// decoding JSON. Oversized JSON fails rather than returning partial results.
pub(super) async fn search_json<T: DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> TalosResult<T> {
    let mut response = request
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|err| {
            TalosError::from_source(
                ErrorCode::ToolExecutionFailed,
                "Web search request failed.",
                err.without_url(),
            )
        })?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| {
        TalosError::from_source(
            ErrorCode::ToolExecutionFailed,
            "Read web search response failed.",
            err.without_url(),
        )
    })? {
        if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
            return Err(TalosError::new(
                ErrorCode::ToolExecutionFailed,
                "Web search response exceeds the 1 MiB limit.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|err| {
        TalosError::from_source(
            ErrorCode::ToolExecutionFailed,
            "Parse web search response failed.",
            err,
        )
    })
}
