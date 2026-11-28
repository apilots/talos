//! Embedded-only request, authentication, byte-stream and shutdown adaptation.
use super::*;

impl EmbeddedServerClient {
    pub(super) fn select_optional_computer(
        &mut self,
        node_id: &mut Option<String>,
    ) -> TalosResult<()> {
        if let Some(target) = node_id.as_deref() {
            self.select_computer(target)?;
            if self.remote.is_none() {
                *node_id = Some("embedded".into());
            }
        }
        Ok(())
    }

    pub(super) fn gateway(&self) -> &dyn talos_gateway::Gateway {
        self.runtime.gateway.as_ref()
    }

    pub(super) async fn refresh_embedded_login(
        &mut self,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        let response = self
            .runtime
            .gateway
            .cloud_login_status(&self.actor, request)
            .await?;
        if let CloudLoginStatusResponse::Authenticated { access_token, .. } = &response {
            self.actor = authenticated_actor_from_access_token(&self.runtime, access_token).await?;
            self.runtime.resume_account_host(&self.actor);
        }
        Ok(response)
    }

    pub(super) async fn start_embedded_turn(
        &mut self,
        request: StartTurnRequest,
    ) -> TalosResult<StartTurnResponse> {
        let (request, source) = prepare_embedded_start_turn(request)?;
        let idempotency_key = request.client_request_id.clone();
        self.runtime
            .gateway
            .start_turn(&self.actor, source, request, idempotency_key)
            .await
    }

    pub(super) async fn upload_embedded_file(
        &mut self,
        request: FileUploadRequest,
        data: Bytes,
    ) -> TalosResult<FileUploadResponse> {
        self.runtime
            .gateway
            .upload_file(
                &self.actor,
                request,
                Box::pin(futures_util::stream::iter([Ok(data)])),
            )
            .await
    }

    pub(super) async fn read_embedded_file_content(
        &mut self,
        request: FileReadRequest,
    ) -> TalosResult<talos_sdk::FileDownloadResponse> {
        let content = self
            .runtime
            .service
            .files
            .read_content(&self.actor, request)
            .await?;
        Ok(talos_sdk::FileDownloadResponse {
            file: content.file,
            data: content.bytes,
        })
    }

    pub(super) async fn shutdown_embedded_runtime(&mut self) -> TalosResult<()> {
        if let Some(remote) = self.remote.take() {
            talos_sdk::GatewayClient::shutdown(&mut *remote.lock().await).await?;
        }
        self.shutdown_result
            .get_or_init(|| async {
                let subscriptions = self
                    .subscriptions
                    .lock()
                    .expect("embedded subscription lock poisoned")
                    .take()
                    .unwrap_or_default();
                for task in subscriptions.values() {
                    task.abort();
                }
                for (_, task) in subscriptions {
                    let _ = task.await;
                }
                self.runtime.shutdown().await
            })
            .await
            .clone()
    }
}
