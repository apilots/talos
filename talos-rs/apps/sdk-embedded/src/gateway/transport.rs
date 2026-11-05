//! Embedded-only request, authentication, byte-stream and shutdown adaptation.
use super::*;

impl EmbeddedServerClient {
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
            self.runtime.service.sync_status(&self.actor).await?;
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
        for (_, task) in self.subscriptions.drain() {
            task.abort();
            let _ = task.await;
        }
        self.runtime.shutdown().await?;
        Ok(())
    }
}
