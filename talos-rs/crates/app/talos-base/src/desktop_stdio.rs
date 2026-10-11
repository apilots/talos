use crate::desktop_application::DesktopApplication;
use std::sync::Arc;
use talos_auth::{CloudAuthService, CloudSessionCache};
use talos_config::Config;
use talos_error::TalosResult;
use talos_service::AccountService;

pub(crate) async fn run_cloud_desktop(
    config: Config,
    device_name: String,
    max_frame_bytes: usize,
) -> TalosResult<()> {
    let auth = Arc::new(CloudAuthService::new(config.cloud.api_base_url.clone()));
    let account = AccountService::new(
        auth.clone(),
        auth.clone(),
        config.cloud.api_base_url.clone(),
    );
    account.set_cloud_session_cache(CloudSessionCache::for_talos_home(
        config.talos_paths().talos_home,
    ))?;
    let validator = Arc::new(talos_app_server::AuthServiceCredentialValidator::new(
        auth.clone(),
        account.clone(),
    ));
    let app = Arc::new(DesktopApplication::new(
        config.clone(),
        account,
        auth,
        device_name,
        None,
    ));
    // No local product database, automation or Coordinator is composed in cloud mode.
    talos_app_server::run_stdio_with_max_frame_bytes(
        app,
        validator,
        talos_app_server::AppServerConfig {
            auth_enabled: true,
            cloud_auth_url: Some(config.cloud.web_base_url.clone()),
            ..Default::default()
        },
        max_frame_bytes,
    )
    .await
    .map_err(|error| {
        talos_error::TalosError::internal_with_source(
            "Desktop cloud host failed to serve the connection; technical details were withheld.",
            error,
        )
    })
}
