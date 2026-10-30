//! HTTP deadlines shared by native and custom model connections.
use std::time::Duration;

use reqwest::{Client, ClientBuilder};
use talos_error::{TalosError, TalosResult};
pub use talos_types::ProviderHttpTimeouts;

/// HTTP clients selected by response mode, independently of the model API dialect.
pub struct ProviderHttpClients {
    response: Client,
    stream: Client,
}

impl ProviderHttpClients {
    /// The builder supplies adapter defaults; an explicit policy overrides deadlines.
    pub fn new(
        builder: impl Fn() -> ClientBuilder,
        timeouts: Option<&ProviderHttpTimeouts>,
    ) -> TalosResult<Self> {
        if let Some(timeouts) = timeouts {
            timeouts.validate()?;
        }
        let build = |stream: bool| {
            let mut client = builder();
            if let Some(timeouts) = timeouts {
                client = client
                    .connect_timeout(Duration::from_secs(timeouts.connect_secs))
                    .timeout(Duration::from_secs(if stream {
                        timeouts.stream_total_secs
                    } else {
                        timeouts.response_secs
                    }));
                if stream {
                    client = client.read_timeout(Duration::from_secs(timeouts.stream_idle_secs));
                }
            }
            client.build().map_err(|error| {
                TalosError::internal_with_source(
                    "Failed to initialize model HTTP client.",
                    error.without_url(),
                )
            })
        };
        let response = build(false)?;
        let stream = if timeouts.is_some() {
            build(true)?
        } else {
            response.clone()
        };
        Ok(Self { response, stream })
    }

    pub fn for_request(&self, stream: bool) -> &Client {
        if stream {
            &self.stream
        } else {
            &self.response
        }
    }
}
