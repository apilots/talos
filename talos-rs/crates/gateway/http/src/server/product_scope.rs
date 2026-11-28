//! Apply authenticated product selection to the existing typed REST contracts.
use super::{errors::ApiError, product_routing::SelectedComputer};
use axum::{
    async_trait,
    extract::{FromRequest, FromRequestParts, Query, Request},
    http::request::Parts,
    response::{IntoResponse, Response},
    Json,
};
use serde::de::DeserializeOwned;
use talos_app_protocol::requests::ClientRequestParams;
use talos_error::TalosResult;

pub(super) trait ScopedRequest: Sized {
    fn bind(self, scope: &SelectedComputer) -> TalosResult<Self>;
}

macro_rules! scoped {
    ($($variant:ident($request:ty)),* $(,)?) => {$(
        impl ScopedRequest for $request {
            fn bind(self, scope: &SelectedComputer) -> TalosResult<Self> {
                let mut request = ClientRequestParams::$variant(self);
                request.bind_computer_target_from_alias(&scope.computer_id, scope.requested_alias.as_deref())?;
                match request {
                    ClientRequestParams::$variant(value) => Ok(value),
                    _ => Err(talos_error::TalosError::internal("Product scope changed the request kind.")),
                }
            }
        }
    )*};
}

scoped! {
    SessionList(talos_types::SessionListRequest), ProjectList(talos_types::ProjectListRequest),
    ProjectCreate(talos_types::ProjectCreateRequest), TurnStart(talos_types::StartTurnRequest),
    ModelList(talos_types::ModelListRequest), ConfigReload(talos_types::ConfigReloadRequest),
    WorkspaceInfo(talos_types::WorkspaceInfoRequest), WorkspaceDocList(talos_types::WorkspaceDocListRequest),
    FsDirectoryRead(talos_types::FsDirectoryReadRequest), FsDirectoryCreate(talos_types::FsDirectoryCreateRequest),
    MemoryFileWrite(talos_types::MemoryFileWriteRequest),
    SkillsList(talos_types::SkillsListRequest), SkillsRefresh(talos_types::SkillsRefreshRequest),
    SkillsSuggest(talos_types::SkillsSuggestRequest), SkillsRead(talos_types::SkillsReadRequest), SkillsResolve(talos_types::SkillsResolveRequest),
    PluginSourceList(talos_types::PluginSourceListRequest), PluginSourceAdd(talos_types::PluginSourceAddRequest),
    PluginSourceRefresh(talos_types::PluginSourceRefreshRequest), PluginSourceRemove(talos_types::PluginSourceRemoveRequest),
    PluginSearch(talos_types::PluginSearchRequest), PluginList(talos_types::PluginListRequest),
    PluginRead(talos_types::PluginReadRequest), PluginInstall(talos_types::PluginInstallRequest), PluginUpdate(talos_types::PluginUpdateRequest),
    PluginUninstall(talos_types::PluginUninstallRequest), PluginEnable(talos_types::PluginEnableRequest), PluginDisable(talos_types::PluginDisableRequest),
    Browser(talos_types::BrowserRequest), DesktopOpen(talos_types::DesktopOpenRequest),
    McpServerList(talos_types::McpServerListRequest), McpServerImport(talos_types::McpServerImportRequest),
    McpAuthenticationStart(talos_types::McpAuthenticationStartRequest), McpAuthenticationRead(talos_types::McpAuthenticationReadRequest),
    McpCredentialsUpdate(talos_types::McpCredentialsUpdateRequest),
    BuiltinJobList(talos_types::BuiltinJobListRequest), BuiltinJobControl(talos_types::BuiltinJobControlRequest),
}

pub(super) struct ProductQuery<T>(pub T);
#[async_trait]
impl<S, T> FromRequestParts<S> for ProductQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned + ScopedRequest + Send,
{
    type Rejection = Response;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Response> {
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(IntoResponse::into_response)?;
        let value = match parts.extensions.get::<SelectedComputer>() {
            Some(scope) => value
                .bind(scope)
                .map_err(|error| ApiError::from_talos_error(error).into_response())?,
            None => value,
        };
        Ok(Self(value))
    }
}

pub(super) struct ProductJson<T>(pub T);
#[async_trait]
impl<S, T> FromRequest<S> for ProductJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + ScopedRequest + Send,
{
    type Rejection = Response;
    async fn from_request(request: Request, state: &S) -> Result<Self, Response> {
        let scope = request.extensions().get::<SelectedComputer>().cloned();
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(IntoResponse::into_response)?;
        let value = match scope {
            Some(scope) => value
                .bind(&scope)
                .map_err(|error| ApiError::from_talos_error(error).into_response())?,
            None => value,
        };
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        extract::Extension,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use tower::ServiceExt;

    fn router() -> Router {
        Router::new()
            .route("/query", get(|ProductQuery(value): ProductQuery<talos_types::WorkspaceInfoRequest>| async move { Json(value) }))
            .route("/body", post(|ProductJson(value): ProductJson<talos_types::ConfigReloadRequest>| async move { Json(value) }))
            .layer(Extension(SelectedComputer { computer_id: "embedded".into(), requested_alias: Some("personal-host".into()) }))
    }

    #[tokio::test]
    async fn typed_rest_queries_bind_defaults_and_authenticated_personal_alias() {
        for path in ["/query", "/query?nodeId=personal-host"] {
            let response = router()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
            let value: talos_types::WorkspaceInfoRequest = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value.node_id.as_deref(), Some("embedded"));
        }
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/query?nodeId=foreign-host")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn typed_rest_bodies_share_the_generic_request_scope_contract() {
        for (node, expected) in [
            ("personal-host", StatusCode::OK),
            ("foreign-host", StatusCode::FORBIDDEN),
        ] {
            let response = router()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/body")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::json!({"nodeId":node}).to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }
    }
}
