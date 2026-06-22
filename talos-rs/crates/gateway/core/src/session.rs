use talos_application_service::Actor;
use talos_error::TalosResult;
use talos_types::{
    SessionActiveTurnRequest, SessionActiveTurnResponse, SessionClearRequest, SessionClearResponse,
    SessionCloseRequest, SessionCloseResponse, SessionCompactRequest, SessionCompactResponse,
    SessionDeleteRequest, SessionDeleteResponse, SessionGetRequest, SessionGetResponse,
    SessionHistoryRequest, SessionHistoryResponse, SessionInfo, SessionListRequest,
    SessionListResponse, SessionPatchRequest, SessionUpdateResponse,
};

use crate::application_service_gateway::{require_non_empty, ApplicationServiceGateway};

impl ApplicationServiceGateway {
    pub(crate) async fn list_sessions(
        &self,
        actor: &Actor,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse> {
        let sessions = self
            .application_service
            .list_sessions(actor, request)
            .await?;
        Ok(SessionListResponse {
            sessions: sessions.iter().map(SessionInfo::from).collect(),
        })
    }

    pub(crate) async fn get_session(
        &self,
        actor: &Actor,
        request: SessionGetRequest,
    ) -> TalosResult<SessionGetResponse> {
        let session = self
            .application_service
            .get_session(actor, &request.session_id)
            .await?;
        Ok(SessionGetResponse {
            session: SessionInfo::from(&session),
        })
    }

    pub(crate) async fn get_session_history(
        &self,
        actor: &Actor,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse> {
        let history = self
            .application_service
            .get_session_history(actor, &request.session_id)
            .await?;
        Ok(SessionHistoryResponse { history })
    }

    pub(crate) async fn get_active_turn(
        &self,
        actor: &Actor,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse> {
        let active_turn = self
            .application_service
            .get_active_turn(actor, &request.session_id)
            .await?;
        Ok(SessionActiveTurnResponse { active_turn })
    }

    pub(crate) async fn patch_session(
        &self,
        actor: &Actor,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse> {
        require_non_empty(&request.session_id, "session_id")?;
        let session_id = request.session_id.clone();
        self.application_service
            .patch_session(actor, &session_id, request.into_patch())
            .await?;
        let session = self
            .application_service
            .get_session(actor, &session_id)
            .await?;
        Ok(SessionUpdateResponse {
            session: SessionInfo::from(&session),
        })
    }

    pub(crate) async fn close_session(
        &self,
        actor: &Actor,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse> {
        self.application_service
            .close_session(actor, &request.session_id)
            .await?;
        Ok(SessionCloseResponse { success: true })
    }

    pub(crate) async fn delete_session(
        &self,
        actor: &Actor,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse> {
        self.application_service
            .delete_session(actor, &request.session_id)
            .await
            .map(|result| SessionDeleteResponse {
                success: result.success,
            })
    }

    pub(crate) async fn compact_session(
        &self,
        actor: &Actor,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse> {
        let session = self
            .application_service
            .compact_session(actor, &request.session_id)
            .await?;
        Ok(SessionCompactResponse {
            session: SessionInfo::from(&session),
        })
    }

    pub(crate) async fn clear_session(
        &self,
        actor: &Actor,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse> {
        let session = self
            .application_service
            .clear_session(actor, &request.session_id)
            .await?;
        Ok(SessionClearResponse {
            session: SessionInfo::from(&session),
        })
    }
}
