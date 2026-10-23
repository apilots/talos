use std::collections::VecDeque;
use std::path::PathBuf;

use talos_config::BashOutputLogConfig;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::BashOutputLog;

use super::output_log::LogWriter;
use super::OutputStreamKind;
use crate::spec::ToolResultPolicy;
use crate::ToolContext;

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

struct LogPlan {
    workspace: PathBuf,
    session: String,
    turn: String,
    call: String,
    config: BashOutputLogConfig,
}

/// One collector, serialized by BashSession's existing mutex. At most two
/// readers each hold one 4 KiB chunk while awaiting the collector/writer.
pub(super) struct OutputCapture {
    policy: ToolResultPolicy,
    plan: Option<LogPlan>,
    preview: VecDeque<(u8, OutputStreamKind)>,
    writer: Option<LogWriter>,
    pub log: Option<BashOutputLog>,
    pub error: Option<TalosError>,
    pub total_bytes: u64,
    newlines: u64,
    open_line: bool,
    pub truncated: bool,
    pub interrupted: bool,
    pub progress_bytes: usize,
    pub progress_omitted: bool,
}

/// Keep an incomplete UTF-8 suffix between pipe reads. Invalid bytes are shown
/// with a replacement character without retaining an unbounded pending buffer.
pub(super) fn decode_progress(pending: &mut Vec<u8>, bytes: &[u8]) -> String {
    pending.extend_from_slice(bytes);
    let mut text = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(valid) => {
                text.push_str(valid);
                pending.clear();
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                let invalid = error.error_len();
                text.push_str(std::str::from_utf8(&pending[..valid]).expect("valid UTF-8 prefix"));
                pending.drain(..valid);
                if let Some(length) = invalid {
                    text.push('\u{fffd}');
                    pending.drain(..length);
                } else {
                    break;
                }
            }
        }
    }
    text
}

impl OutputCapture {
    pub async fn new(context: &ToolContext, policy: ToolResultPolicy) -> TalosResult<Self> {
        if policy.max_inline_bytes == 0 || policy.preview_lines == 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Bash preview limits must be positive.",
            ));
        }
        let plan = if policy.persist_large_output {
            let config = context
                .kernel_config()
                .filter(|config| !config.workspace_dir.is_empty())
                .ok_or_else(|| {
                    TalosError::internal(
                        "Bash output persistence requires the resolved user Workspace.",
                    )
                })?;
            Some(LogPlan {
                workspace: PathBuf::from(&config.workspace_dir),
                session: context.session_id().to_owned(),
                turn: context
                    .turn_id()
                    .ok_or_else(|| {
                        TalosError::internal("Bash output persistence requires a Turn identity.")
                    })?
                    .to_owned(),
                call: context
                    .current_call_id()
                    .ok_or_else(|| {
                        TalosError::internal(
                            "Bash output persistence requires a tool call identity.",
                        )
                    })?
                    .to_owned(),
                config: config.tools.bash.output_log.clone(),
            })
        } else {
            None
        };
        Ok(Self {
            policy,
            plan,
            preview: VecDeque::new(),
            writer: None,
            log: None,
            error: None,
            total_bytes: 0,
            newlines: 0,
            open_line: false,
            truncated: false,
            interrupted: false,
            progress_bytes: 0,
            progress_omitted: false,
        })
    }

    pub fn policy_limit(&self) -> usize {
        self.policy.max_inline_bytes
    }

    pub fn record_error(&mut self, error: TalosError) {
        if self.error.is_none() {
            self.error = Some(error);
        } else {
            talos_logging::report_error(&error);
        }
    }

    fn serialized_preview_bytes(&self) -> usize {
        let (_, stdout, stderr) = self.snapshot();
        serde_json::to_vec(&(stdout, stderr)).map_or(usize::MAX, |bytes| bytes.len())
    }

    pub async fn append(&mut self, bytes: &[u8], stream: OutputStreamKind) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        self.newlines += bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
        if let Some(last) = bytes.last() {
            self.open_line = *last != b'\n';
        }
        self.preview
            .extend(bytes.iter().map(|byte| (*byte, stream)));
        let lines = self
            .preview
            .iter()
            .filter(|(byte, _)| *byte == b'\n')
            .count()
            + usize::from(self.open_line);
        let exceeds = self.preview.len() > self.policy.max_inline_bytes
            || lines > self.policy.preview_lines
            || self.serialized_preview_bytes() > self.policy.max_inline_bytes;
        if exceeds && !self.truncated {
            if let Some(plan) = self.plan.take() {
                match LogWriter::open(
                    plan.workspace,
                    plan.session,
                    plan.turn,
                    plan.call,
                    plan.config,
                )
                .await
                {
                    Ok(mut writer) => {
                        let initial = self
                            .preview
                            .iter()
                            .map(|(byte, _)| *byte)
                            .collect::<Vec<_>>();
                        if let Err(error) = writer.append(&initial).await {
                            self.record_error(error);
                        }
                        self.writer = Some(writer);
                    }
                    Err(error) => self.record_error(error),
                }
            }
        } else if let Some(writer) = &mut self.writer {
            if writer.accepts_output() {
                if let Err(error) = writer.append(bytes).await {
                    self.record_error(error);
                }
            }
        }
        self.truncated |= exceeds;
        while self.preview.len() > self.policy.max_inline_bytes {
            self.preview.pop_front();
        }
        let mut line_count = self
            .preview
            .iter()
            .filter(|(byte, _)| *byte == b'\n')
            .count()
            + usize::from(self.open_line);
        while line_count > self.policy.preview_lines {
            if self
                .preview
                .pop_front()
                .is_some_and(|(byte, _)| byte == b'\n')
            {
                line_count -= 1;
            }
        }
        // Escaping and invalid UTF-8 replacement can expand a short raw chunk.
        // Spill before trimming it, then bound the actual JSON preview as well.
        // A byte contributes at most six serialized bytes, so batched removal
        // avoids a quadratic one-byte-at-a-time serialization loop.
        loop {
            let size = self.serialized_preview_bytes();
            if size <= self.policy.max_inline_bytes || self.preview.is_empty() {
                break;
            }
            let remove = (size - self.policy.max_inline_bytes).div_ceil(6);
            self.preview.drain(..remove.min(self.preview.len()));
        }
    }

    pub fn snapshot(&self) -> (String, String, String) {
        fn decode(bytes: Vec<u8>) -> String {
            let start = bytes
                .iter()
                .take_while(|byte| **byte & 0xc0 == 0x80)
                .count();
            String::from_utf8_lossy(&bytes[start..]).into_owned()
        }
        let combined = decode(self.preview.iter().map(|(byte, _)| *byte).collect());
        let stdout = decode(
            self.preview
                .iter()
                .filter(|(_, stream)| matches!(stream, OutputStreamKind::Stdout))
                .map(|(byte, _)| *byte)
                .collect(),
        );
        let stderr = decode(
            self.preview
                .iter()
                .filter(|(_, stream)| matches!(stream, OutputStreamKind::Stderr))
                .map(|(byte, _)| *byte)
                .collect(),
        );
        (combined, stdout, stderr)
    }

    pub async fn finish(&mut self) {
        if let Some(writer) = self.writer.take() {
            let completion = writer
                .finish(
                    self.total_bytes,
                    self.newlines + u64::from(self.open_line),
                    self.interrupted,
                )
                .await;
            self.log = Some(completion.log);
            if let Some(error) = completion.error {
                self.record_error(error);
            }
        }
    }
}
