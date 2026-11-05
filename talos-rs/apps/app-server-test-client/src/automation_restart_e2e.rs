//! Hard process restart distinguishes durable definitions from the existing memory queue.
use crate::automation_e2e_support::*;
use anyhow::{ensure, Result};
use serde_json::json;
use std::time::Duration;
use talos_sdk::{
    AppServerClient, AppServerClientConfig, GatewayClient, LoopAction, LoopStatus,
    MessageQueueAddRequest, MessageQueueListRequest, QueuedMessageSource, SkillScope,
    SkillsListRequest,
};

async fn connect(binary: &str) -> Result<AppServerClient> {
    Ok(AppServerClient::connect(AppServerClientConfig {
        app_server_bin: binary.into(),
        client_name: "automation-restart-acceptance".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        actor_credentials: None,
        use_cloud_session_cache: false,
    })
    .await?)
}

pub(super) async fn run() -> Result<()> {
    let root = directory()?;
    let wrapper = root.join("restart-app-server.sh");
    // Only this test-owned wrapper publishes its own PID. No process discovery or user host is used.
    std::fs::write(
        &wrapper,
        concat!(
            "#!/bin/sh\n",
            "printf '%s\\n' \"$$\" > \"${TALOS_AUTOMATION_FIXTURE_DIR}/restart-app-server.pid\"\n",
            "exec \"${TALOS_APP_SERVER_BIN}\"\n",
        ),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))?;
    }
    let binary = wrapper.to_string_lossy();
    let mut first = connect(&binary).await?;
    let session = uuid::Uuid::new_v4().to_string();
    let registration = marker("stop");
    let skills = first
        .list_skills(SkillsListRequest {
            node_id: Some("embedded".into()),
            include_disabled: false,
        })
        .await?;
    let skill = skills
        .data
        .iter()
        .find(|skill| {
            skill.scope == SkillScope::System
                && skill.name == "loop"
                && skill.enabled
                && skill.read_only
        })
        .expect("real System Loop Skill unavailable");
    start(&mut first, &session, selected(&registration, skill)).await?;
    gate(&registration, "registered").await?;
    let task = snapshot(&mut first, &session)
        .await?
        .loops
        .first()
        .expect("real model Tool did not create Loop")
        .clone();
    ensure!(
        task.next_due_at == Some(task.created_at + 10),
        "registration did not retain the initial grace deadline: {task:?}"
    );
    first
        .add_queued_message(MessageQueueAddRequest {
            session_id: session.clone(),
            input: text("QUEUED_USER_MUST_NOT_BE_REPLAYED_AFTER_RESTART"),
        })
        .await?;
    let due = wait_snapshot(&mut first, &session, |state| {
        state
            .loops
            .iter()
            .any(|loop_task| loop_task.id == task.id && loop_task.next_due_at.is_none())
    })
    .await?;
    let queued = first
        .list_message_queue(MessageQueueListRequest {
            session_id: session.clone(),
        })
        .await?;
    ensure!(queued.messages.len() == 2 && queued.messages.iter().any(|message|
        matches!(&message.source, QueuedMessageSource::Loop { loop_id, .. } if loop_id == &task.id)),
        "restart fixture did not have both real User and Loop queue entries");
    std::fs::write(
        root.join("restart-before.json"),
        serde_json::to_vec_pretty(&json!({
            "registration":task,
            "definition":due.loops.iter().find(|current| current.id == task.id),
            "queue":queued
        }))?,
    )?;
    let pid: u32 = std::fs::read_to_string(root.join("restart-app-server.pid"))?
        .trim()
        .parse()?;
    ensure!(pid > 1, "invalid test-owned app-server PID");
    ensure!(
        std::process::Command::new("/bin/kill")
            .args(["-KILL", &pid.to_string()])
            .status()?
            .success(),
        "hard restart failed"
    );
    drop(first);
    let mut second = connect(&binary).await?;
    let after = snapshot(&mut second, &session).await?;
    let retained = after
        .loops
        .iter()
        .find(|loop_task| loop_task.id == task.id)
        .expect("restart discarded the durable Loop definition");
    ensure!(
        retained.status == LoopStatus::Active && retained.next_due_at.is_none(),
        "restart invented a blanket stop or a next time: {retained:?}"
    );
    ensure!(
        second
            .list_message_queue(MessageQueueListRequest {
                session_id: session.clone()
            })
            .await?
            .messages
            .is_empty(),
        "process-local User/Loop queue was replayed after restart"
    );
    // Let the original real scan tick; null time and lost memory alone cannot fabricate an occurrence.
    tokio::time::sleep(Duration::from_secs(2)).await;
    ensure!(
        snapshot(&mut second, &session)
            .await?
            .loops
            .iter()
            .any(|loop_task| loop_task.id == task.id
                && loop_task.status == LoopStatus::Active
                && loop_task.next_due_at.is_none()),
        "lost queued occurrence changed the durable definition after restart"
    );
    std::fs::write(
        root.join("restart-after.json"),
        serde_json::to_vec_pretty(&after)?,
    )?;
    control(&mut second, &session, LoopAction::Stop { loop_id: task.id }).await?;
    release(&registration, "registered")?;
    GatewayClient::shutdown(&mut second).await?;
    eprintln!("CASE automation_restart PASS durableDefinition=retained queue=memoryOnly nullNext=noInventedOccurrence");
    Ok(())
}
