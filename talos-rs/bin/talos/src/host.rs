use talos_channel::ChannelServer;
use talos_error::{TalosError, TalosResult};
use talos_gateway_http::HttpServer;

pub struct TalosHost {
    http_server: Option<HttpServer>,
    channel_server: Option<ChannelServer>,
}

impl TalosHost {
    pub fn new() -> Self {
        Self {
            http_server: None,
            channel_server: None,
        }
    }

    pub fn server(mut self, server: HttpServer) -> Self {
        self.http_server = Some(server);
        self
    }

    pub fn channel(mut self, channel: ChannelServer) -> Self {
        self.channel_server = Some(channel);
        self
    }

    pub async fn run(self) -> TalosResult<()> {
        match (self.http_server, self.channel_server) {
            (Some(http_server), Some(channel_server)) => {
                let mut tasks = tokio::task::JoinSet::new();
                tasks.spawn(async move { http_server.run().await });
                tasks.spawn(async move { channel_server.run().await });

                match tasks.join_next().await {
                    Some(Ok(result)) => result,
                    Some(Err(err)) => Err(TalosError::InternalError(format!(
                        "host task join error: {}",
                        err
                    ))),
                    None => Ok(()),
                }
            }
            (Some(http_server), None) => http_server.run().await,
            (None, Some(channel_server)) => channel_server.run().await,
            (None, None) => Err(TalosError::InvalidConfiguration(
                "TalosHost requires at least one server".to_string(),
            )),
        }
    }
}

impl Default for TalosHost {
    fn default() -> Self {
        Self::new()
    }
}
