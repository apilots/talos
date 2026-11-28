use super::*;
use talos_app_protocol::requests::{ClientMethod, ClientRequest, ClientRequestParams};
use talos_app_server::AppServerApplication;
use talos_types::{
    BuiltinJobAction, BuiltinJobControlRequest, BuiltinJobInfo, BuiltinJobListRequest,
    BuiltinJobListResponse,
};

#[tokio::test]
async fn enrolled_desktop_owner_controls_jobs_through_public_and_embedded_targets() {
    let fixture = DesktopFixture::new().await;
    fixture.login("user-1");
    let host = fixture.host("Job owner");
    let computer = host
        .enroll_worker(None, None)
        .await
        .unwrap()
        .node_id
        .unwrap();
    let owner = Actor::regular("user-1").with_access_token("user-1");
    for selected in [&computer, "embedded"] {
        let response = host
            .dispatch(
                &owner,
                ClientRequest {
                    computer_id: None,
                    method: ClientMethod::BuiltinJobList,
                    params: ClientRequestParams::BuiltinJobList(BuiltinJobListRequest {
                        computer_id: selected.into(),
                    }),
                },
            )
            .await
            .unwrap();
        let rows: BuiltinJobListResponse = serde_json::from_value(response.body).unwrap();
        assert!(rows.data.iter().all(|job| job.computer_id == computer));
        let job = &rows.data[0];
        for action in [BuiltinJobAction::Pause, BuiltinJobAction::Resume] {
            let response = host
                .dispatch(
                    &owner,
                    ClientRequest {
                        computer_id: None,
                        method: ClientMethod::BuiltinJobControl,
                        params: ClientRequestParams::BuiltinJobControl(BuiltinJobControlRequest {
                            computer_id: selected.into(),
                            execution_instance_id: job.execution_instance_id.clone(),
                            job_id: job.id,
                            action,
                        }),
                    },
                )
                .await
                .unwrap();
            let result: BuiltinJobInfo = serde_json::from_value(response.body).unwrap();
            assert_eq!(result.computer_id, computer);
            assert_eq!(result.execution_instance_id, job.execution_instance_id);
            assert_eq!(result.paused, action == BuiltinJobAction::Pause);
        }
    }
    host.stop_registration().await.unwrap();
    fixture.runtime.shutdown_host().await.unwrap();
    fixture.runtime.shutdown().await.unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}
