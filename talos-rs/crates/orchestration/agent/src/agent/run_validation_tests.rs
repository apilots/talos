use super::*;

#[test]
fn run_validation_rejects_a_mismatched_agent_identity() {
    let agent = build_test_agent();
    let session = build_test_session("session-wrong-agent", "workspace-1");
    let mut request = agent_run_params(session, "turn-wrong-agent", Vec::new());
    request.context.agent_id = "other-agent".to_string();

    let error = agent
        .validate_run_request(&request)
        .expect_err("the execution context must target this agent");

    assert!(error.to_string().contains("other-agent"));
    assert!(error.to_string().contains("test_agent"));
}

#[tokio::test]
async fn run_rejects_resume_context() {
    let agent = build_test_agent();
    let session = build_test_session("session-run-with-resume-context", "workspace-1");
    let mut request = agent_run_params(session, "turn-run-with-resume-context", Vec::new());
    request.resume_context = Some(AgentDispatchResumeContext {
        tool_call: Some(AgentDispatchResumeToolCall {
            item_id: "tool-item-original".to_string(),
            call_id: "call-dynamic".to_string(),
            tool_name: "echo_dynamic".to_string(),
        }),
    });

    let error = match agent.run(request).await {
        Ok(_) => panic!("run should reject continuation context"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("use resume API"));
    assert!(error.to_string().contains("turn-run-with-resume-context"));
}
