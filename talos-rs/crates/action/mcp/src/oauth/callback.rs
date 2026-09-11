use http_body_util::Full;
use hyper::{
    body::{Bytes, Incoming},
    service::service_fn,
    Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};
use talos_error::{ErrorCode, TalosError, TalosResult};

/// The attempt owns the listener and each accepted connection. Cancellation drops both;
/// no detached HTTP server or connection task survives the authorization attempt.
pub(super) async fn receive(
    listener: &tokio::net::TcpListener,
    redirect_uri: &str,
    state: &str,
) -> TalosResult<String> {
    loop {
        let (stream, _) = listener.accept().await.map_err(|error| {
            TalosError::internal_with_source("The local OAuth callback listener failed.", error)
        })?;
        let received = Arc::new(Mutex::new(None));
        let result = received.clone();
        let redirect = redirect_uri.to_owned();
        let state = state.to_owned();
        let service = service_fn(move |request: Request<Incoming>| {
            let result = result.clone();
            let redirect = redirect.clone();
            let state = state.clone();
            async move {
                let callback = bound_callback(&request, &redirect, &state);
                let valid = callback.is_some();
                if let Some(callback) = callback {
                    *result.lock().expect("OAuth callback lock poisoned") = Some(callback);
                }
                let mut response = Response::new(Full::new(Bytes::from_static(if valid {
                    b"Authorization response received. Return to Talos to check the result."
                } else {
                    b"Invalid authorization callback."
                })));
                *response.status_mut() = if valid {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                };
                response.headers_mut().insert(
                    hyper::header::CONTENT_TYPE,
                    hyper::header::HeaderValue::from_static("text/plain; charset=utf-8"),
                );
                response.headers_mut().insert(
                    hyper::header::CACHE_CONTROL,
                    hyper::header::HeaderValue::from_static("no-store"),
                );
                Ok::<_, Infallible>(response)
            }
        });
        let mut builder = hyper::server::conn::http1::Builder::new();
        builder
            .keep_alive(false)
            .max_buf_size(16 * 1024)
            .timer(TokioTimer::new())
            .header_read_timeout(Duration::from_secs(5));
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            builder.serve_connection(TokioIo::new(stream), service),
        )
        .await;
        if let Some(callback) = received
            .lock()
            .expect("OAuth callback lock poisoned")
            .take()
        {
            return callback;
        };
    }
}

// Route and attempt binding precede the SDK exchange. Reject duplicate security
// parameters rather than interpreting a different value from the SDK parser.
fn bound_callback(
    request: &Request<Incoming>,
    redirect: &str,
    expected_state: &str,
) -> Option<TalosResult<String>> {
    let host = redirect.strip_prefix("http://")?.split('/').next()?;
    if request.method() != hyper::Method::GET
        || request.uri().path() != "/mcp/oauth/callback"
        || request.uri().to_string().len() > 8192
        || request.uri().scheme().is_some()
        || request.headers().get(hyper::header::HOST)?.to_str().ok()? != host
    {
        return None;
    }
    let query = request.uri().query()?;
    let mut state = None;
    let mut code = None;
    let mut denial = None;
    let mut issuer = None;
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let field = match name.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut denial,
            "iss" => &mut issuer,
            _ => continue,
        };
        if value.is_empty() || field.replace(value).is_some() {
            return None;
        }
    }
    if state.as_deref() != Some(expected_state) {
        return None;
    }
    match (code, denial) {
        (Some(_), None) => Some(Ok(format!("{redirect}?{query}"))),
        (None, Some(_)) => Some(Err(TalosError::new(
            ErrorCode::Cancelled,
            "The MCP authorization request was declined.",
        ))),
        _ => None,
    }
}
