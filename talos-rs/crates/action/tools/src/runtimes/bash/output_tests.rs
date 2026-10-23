use super::*;

async fn persistent_capture(workspace: &std::path::Path) -> OutputCapture {
    let config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut context =
        ToolContext::new_with_kernel_config("session", "user", Some(std::sync::Arc::new(config)))
            .with_turn_id("turn");
    context.bind_current_call_id("escaped");
    OutputCapture::new(
        &context,
        ToolResultPolicy {
            max_inline_bytes: 1024,
            preview_lines: 60,
            persist_large_output: true,
        },
    )
    .await
    .unwrap()
}

#[test]
fn progress_decoder_preserves_split_utf8_and_bounds_incomplete_suffixes() {
    let original = "中文😀abc".repeat(100);
    let mut pending = Vec::new();
    let mut combined = String::new();
    for chunk in original.as_bytes().chunks(2) {
        combined.push_str(&decode_progress(&mut pending, chunk));
        assert!(pending.len() < 4);
    }
    assert_eq!(combined, original);
    assert!(pending.is_empty());
}

#[tokio::test]
async fn preview_memory_stays_bounded_and_persist_false_does_not_create_a_log() {
    let context = ToolContext::new("bounded", "user");
    let mut output = OutputCapture::new(
        &context,
        ToolResultPolicy {
            max_inline_bytes: 1024,
            preview_lines: 3,
            persist_large_output: false,
        },
    )
    .await
    .unwrap();
    for _ in 0..1024 {
        output.append(&[b'x'; 4096], OutputStreamKind::Stdout).await;
        assert!(output.preview.len() <= 1024);
    }
    output.append(b"\nFINAL\n", OutputStreamKind::Stderr).await;
    output.finish().await;
    assert!(output.snapshot().0.ends_with("FINAL\n"));
    assert!(output.log.is_none());
    assert!(output.truncated);
    assert!(output.total_bytes > 4 * 1024 * 1024);
}

#[tokio::test]
async fn json_escaping_triggers_spill_before_raw_bytes_reach_the_limit() {
    let workspace = tempfile::tempdir().unwrap();
    let mut output = persistent_capture(workspace.path()).await;
    let raw = vec![0_u8; 400];
    output.append(&raw, OutputStreamKind::Stdout).await;
    output.finish().await;
    assert!(output.truncated);
    assert!(output.serialized_preview_bytes() <= 1024);
    let log = output.log.unwrap();
    assert_eq!(std::fs::read(log.path).unwrap(), raw);
    assert_eq!(
        log.capture_status,
        talos_types::BashOutputCaptureStatus::Complete
    );
}

#[tokio::test]
async fn a_pipe_failure_does_not_stop_saving_bytes_received_on_the_other_stream() {
    let workspace = tempfile::tempdir().unwrap();
    let mut output = persistent_capture(workspace.path()).await;
    let mut expected = vec![b'x'; 2048];
    output.append(&expected, OutputStreamKind::Stdout).await;
    output.interrupted = true;
    output.record_error(TalosError::internal("A Bash pipe failed."));
    output.append(b"TAIL", OutputStreamKind::Stderr).await;
    expected.extend_from_slice(b"TAIL");
    output.finish().await;
    let log = output.log.unwrap();
    assert_eq!(std::fs::read(&log.path).unwrap(), expected);
    assert_eq!(log.stored_bytes, expected.len() as u64);
    assert_eq!(
        log.capture_status,
        talos_types::BashOutputCaptureStatus::Interrupted
    );
    assert!(output.error.is_some());
}
