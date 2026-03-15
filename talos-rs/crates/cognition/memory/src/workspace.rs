use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::future::Future;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

const DAY_FILE_PATTERN_LEN: usize = 10; // YYYY-MM-DD

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceMessage {
    pub role: WorkspaceMessageRole,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceToolCallRecord {
    pub call_id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceToolResultRecord {
    pub call_id: String,
    pub tool: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTurnRecord {
    pub timestamp: DateTime<Utc>,
    pub user_input: String,
    pub assistant_output: String,
    #[serde(default)]
    pub reasoning: Vec<String>,
    #[serde(default)]
    pub tool_calls: Vec<WorkspaceToolCallRecord>,
    #[serde(default)]
    pub tool_results: Vec<WorkspaceToolResultRecord>,
}

impl WorkspaceTurnRecord {
    pub fn new(user_input: impl Into<String>, assistant_output: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            user_input: user_input.into(),
            assistant_output: assistant_output.into(),
            reasoning: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceMemoryContext {
    pub summary: Option<String>,
    pub recent_messages: Vec<WorkspaceMessage>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceMemory {
    root_dir: PathBuf,
}

impl WorkspaceMemory {
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    pub fn new_default() -> Self {
        Self::new(Self::default_root_dir())
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn default_root_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".talos")
            .join("workspace")
            .join("memory")
    }

    pub async fn append_turn(
        &self,
        session_id: &str,
        record: &WorkspaceTurnRecord,
    ) -> TalosResult<()> {
        let session_dir = self.session_dir(session_id);
        fs::create_dir_all(&session_dir)
            .await
            .map_err(|e| TalosError::StorageError(format!("create session memory dir: {}", e)))?;

        let day_file = session_dir.join(format!("{}.md", record.timestamp.format("%Y-%m-%d")));
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&day_file)
            .await
            .map_err(|e| {
                TalosError::StorageError(format!("open daily session memory file: {}", e))
            })?;

        let payload = Self::render_turn_markdown(record);
        file.write_all(payload.as_bytes()).await.map_err(|e| {
            TalosError::StorageError(format!("append daily session memory file: {}", e))
        })?;

        Ok(())
    }

    pub async fn append_turn_daily_md(
        &self,
        session_id: &str,
        record: &WorkspaceTurnRecord,
    ) -> TalosResult<()> {
        self.append_turn(session_id, record).await
    }

    pub async fn load_recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<WorkspaceMessage>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut files = self.list_daily_files(session_id).await?;
        files.sort();

        let mut messages = Vec::new();
        for file in files {
            let content = fs::read_to_string(&file).await.map_err(|e| {
                TalosError::StorageError(format!("read daily session memory file: {}", e))
            })?;
            messages.extend(Self::parse_messages_from_markdown(&content));
        }

        if messages.len() <= limit {
            return Ok(messages);
        }

        Ok(messages.split_off(messages.len() - limit))
    }

    pub async fn load_recent_messages_for_session(
        &self,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<WorkspaceMessage>> {
        self.load_recent_messages(session_id, limit).await
    }

    pub async fn load_summary(&self, session_id: &str) -> TalosResult<Option<String>> {
        let summary_path = self.summary_file(session_id);
        match fs::read_to_string(&summary_path).await {
            Ok(content) => Ok(Some(content)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(TalosError::StorageError(format!(
                "read session summary: {}",
                err
            ))),
        }
    }

    pub async fn load_session_summary(&self, session_id: &str) -> TalosResult<Option<String>> {
        self.load_summary(session_id).await
    }

    pub async fn save_summary(&self, session_id: &str, summary: &str) -> TalosResult<()> {
        let session_dir = self.session_dir(session_id);
        fs::create_dir_all(&session_dir)
            .await
            .map_err(|e| TalosError::StorageError(format!("create session memory dir: {}", e)))?;

        let normalized = summary.trim();
        let body = if normalized.is_empty() {
            "(empty)".to_string()
        } else {
            normalized.to_string()
        };
        let rendered = format!(
            "# Session Summary\n\nUpdated: {}\n\n{}\n",
            Utc::now().to_rfc3339(),
            body
        );

        fs::write(self.summary_file(session_id), rendered)
            .await
            .map_err(|e| TalosError::StorageError(format!("write session summary: {}", e)))?;

        Ok(())
    }

    pub async fn save_session_summary(&self, session_id: &str, summary: &str) -> TalosResult<()> {
        self.save_summary(session_id, summary).await
    }

    pub async fn build_context(
        &self,
        session_id: &str,
        recent_limit: usize,
    ) -> TalosResult<WorkspaceMemoryContext> {
        let summary = self.load_summary(session_id).await?;
        let recent_messages = self.load_recent_messages(session_id, recent_limit).await?;
        Ok(WorkspaceMemoryContext {
            summary,
            recent_messages,
        })
    }

    pub async fn session_size_bytes(&self, session_id: &str) -> TalosResult<u64> {
        let mut total = 0_u64;
        for path in self.list_daily_files(session_id).await? {
            let metadata = fs::metadata(path).await.map_err(|e| {
                TalosError::StorageError(format!("read daily memory metadata: {}", e))
            })?;
            total = total.saturating_add(metadata.len());
        }

        let summary = self.summary_file(session_id);
        if let Ok(metadata) = fs::metadata(summary).await {
            total = total.saturating_add(metadata.len());
        }

        Ok(total)
    }

    pub async fn compact_if_needed<F, Fut>(
        &self,
        session_id: &str,
        threshold_bytes: u64,
        recent_limit: usize,
        summarize: F,
    ) -> TalosResult<Option<String>>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = TalosResult<String>>,
    {
        if threshold_bytes == 0 {
            return Ok(None);
        }

        let size = self.session_size_bytes(session_id).await?;
        if size < threshold_bytes {
            return Ok(None);
        }

        let history = self
            .load_recent_messages_for_session(session_id, recent_limit)
            .await?;
        if history.is_empty() {
            return Ok(None);
        }

        let transcript = Self::messages_to_transcript(&history);
        if transcript.trim().is_empty() {
            return Ok(None);
        }

        let summary = summarize(transcript).await?;
        self.save_session_summary(session_id, &summary).await?;
        Ok(Some(summary))
    }

    fn render_turn_markdown(record: &WorkspaceTurnRecord) -> String {
        let mut out = String::new();
        out.push_str(&format!("## Turn {}\n\n", record.timestamp.to_rfc3339()));

        out.push_str("### User\n");
        out.push_str(record.user_input.trim());
        out.push_str("\n\n");

        if !record.reasoning.is_empty() {
            out.push_str("### Reasoning\n");
            for line in &record.reasoning {
                if line.trim().is_empty() {
                    continue;
                }
                out.push_str("- ");
                out.push_str(line.trim());
                out.push('\n');
            }
            out.push('\n');
        }

        if !record.tool_calls.is_empty() {
            out.push_str("### Tool Calls\n");
            for call in &record.tool_calls {
                let line = json!({
                    "call_id": call.call_id,
                    "name": call.name,
                    "arguments": call.arguments,
                });
                out.push_str("```json\n");
                out.push_str(
                    &serde_json::to_string_pretty(&line).unwrap_or_else(|_| "{}".to_string()),
                );
                out.push_str("\n```\n");
            }
            out.push('\n');
        }

        if !record.tool_results.is_empty() {
            out.push_str("### Tool Results\n");
            for result in &record.tool_results {
                let line = json!({
                    "call_id": result.call_id,
                    "tool": result.tool,
                    "is_error": result.is_error,
                    "truncated": result.truncated,
                    "size_bytes": result.size_bytes,
                    "payload": result.payload,
                });
                out.push_str("```json\n");
                out.push_str(
                    &serde_json::to_string_pretty(&line).unwrap_or_else(|_| "{}".to_string()),
                );
                out.push_str("\n```\n");
            }
            out.push('\n');
        }

        out.push_str("### Assistant\n");
        out.push_str(record.assistant_output.trim());
        out.push_str("\n\n");

        out
    }

    fn messages_to_transcript(messages: &[WorkspaceMessage]) -> String {
        messages
            .iter()
            .map(|entry| match entry.role {
                WorkspaceMessageRole::User => format!("User: {}", entry.text.trim()),
                WorkspaceMessageRole::Assistant => {
                    format!("Assistant: {}", entry.text.trim())
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn parse_messages_from_markdown(content: &str) -> Vec<WorkspaceMessage> {
        #[derive(Clone, Copy)]
        enum Section {
            User,
            Assistant,
        }

        let mut section: Option<Section> = None;
        let mut buffer = String::new();
        let mut output = Vec::new();

        fn flush(
            section: Option<Section>,
            buffer: &mut String,
            output: &mut Vec<WorkspaceMessage>,
        ) {
            let text = buffer.trim();
            if text.is_empty() {
                buffer.clear();
                return;
            }

            let role = match section {
                Some(Section::User) => WorkspaceMessageRole::User,
                Some(Section::Assistant) => WorkspaceMessageRole::Assistant,
                None => {
                    buffer.clear();
                    return;
                }
            };

            output.push(WorkspaceMessage {
                role,
                text: text.to_string(),
            });
            buffer.clear();
        }

        for line in content.lines() {
            if line.starts_with("### ") {
                flush(section, &mut buffer, &mut output);
                section = match line.trim() {
                    "### User" => Some(Section::User),
                    "### Assistant" => Some(Section::Assistant),
                    _ => None,
                };
                continue;
            }

            if section.is_some() {
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(line);
            }
        }

        flush(section, &mut buffer, &mut output);
        output
    }

    async fn list_daily_files(&self, session_id: &str) -> TalosResult<Vec<PathBuf>> {
        let mut files = Vec::new();
        let session_dir = self.session_dir(session_id);

        let mut dir = match fs::read_dir(&session_dir).await {
            Ok(dir) => dir,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
            Err(err) => {
                return Err(TalosError::StorageError(format!(
                    "read session memory dir: {}",
                    err
                )));
            }
        };

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| TalosError::StorageError(format!("read session memory entry: {}", e)))?
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !Self::is_daily_file_name(name) {
                continue;
            }
            files.push(path);
        }

        Ok(files)
    }

    fn is_daily_file_name(file_name: &str) -> bool {
        if !file_name.ends_with(".md") {
            return false;
        }

        let date = &file_name[..file_name.len() - 3];
        if date.len() != DAY_FILE_PATTERN_LEN {
            return false;
        }

        let chars: Vec<char> = date.chars().collect();
        for (idx, ch) in chars.iter().enumerate() {
            let is_dash = idx == 4 || idx == 7;
            if is_dash {
                if *ch != '-' {
                    return false;
                }
            } else if !ch.is_ascii_digit() {
                return false;
            }
        }

        true
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root_dir
            .join("sessions")
            .join(Self::sanitize_segment(session_id))
    }

    fn summary_file(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("summary.md")
    }

    fn sanitize_segment(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        for ch in input.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                out.push(ch);
            } else {
                out.push('_');
            }
        }
        if out.trim_matches('_').is_empty() {
            "session".to_string()
        } else {
            out
        }
    }
}

impl Default for WorkspaceMemory {
    fn default() -> Self {
        Self::new_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_temp_memory() -> (tempfile::TempDir, WorkspaceMemory) {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let memory = WorkspaceMemory::new(dir.path().to_path_buf());
        (dir, memory)
    }

    #[tokio::test]
    async fn test_append_turn_and_load_recent_messages() {
        let (_guard, memory) = new_temp_memory();
        let session_id = "session-1";

        memory
            .append_turn(
                session_id,
                &WorkspaceTurnRecord {
                    timestamp: Utc::now(),
                    user_input: "hello".to_string(),
                    assistant_output: "world".to_string(),
                    reasoning: vec!["thinking".to_string()],
                    tool_calls: vec![WorkspaceToolCallRecord {
                        call_id: "call-1".to_string(),
                        name: "exec".to_string(),
                        arguments: json!({"command": "ls"}),
                    }],
                    tool_results: vec![WorkspaceToolResultRecord {
                        call_id: "call-1".to_string(),
                        tool: "exec".to_string(),
                        payload: json!({"result": "Cargo.toml"}),
                        is_error: false,
                        truncated: false,
                        size_bytes: 20,
                    }],
                },
            )
            .await
            .expect("append turn should succeed");

        let messages = memory
            .load_recent_messages(session_id, 10)
            .await
            .expect("load recent should succeed");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, WorkspaceMessageRole::User);
        assert_eq!(messages[0].text, "hello");
        assert_eq!(messages[1].role, WorkspaceMessageRole::Assistant);
        assert_eq!(messages[1].text, "world");
    }

    #[tokio::test]
    async fn test_summary_read_write() {
        let (_guard, memory) = new_temp_memory();
        let session_id = "session-2";

        memory
            .save_summary(session_id, "key facts")
            .await
            .expect("save summary should succeed");

        let summary = memory
            .load_summary(session_id)
            .await
            .expect("load summary should succeed")
            .expect("summary should exist");

        assert!(summary.contains("# Session Summary"));
        assert!(summary.contains("key facts"));
    }

    #[tokio::test]
    async fn test_daily_file_split() {
        let (_guard, memory) = new_temp_memory();
        let session_id = "session-3";

        let day1 = DateTime::parse_from_rfc3339("2026-03-18T08:00:00Z")
            .expect("rfc3339 parse should succeed")
            .with_timezone(&Utc);
        let day2 = DateTime::parse_from_rfc3339("2026-03-19T08:00:00Z")
            .expect("rfc3339 parse should succeed")
            .with_timezone(&Utc);

        memory
            .append_turn(
                session_id,
                &WorkspaceTurnRecord {
                    timestamp: day1,
                    user_input: "u1".to_string(),
                    assistant_output: "a1".to_string(),
                    reasoning: Vec::new(),
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                },
            )
            .await
            .expect("day1 append should succeed");

        memory
            .append_turn(
                session_id,
                &WorkspaceTurnRecord {
                    timestamp: day2,
                    user_input: "u2".to_string(),
                    assistant_output: "a2".to_string(),
                    reasoning: Vec::new(),
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                },
            )
            .await
            .expect("day2 append should succeed");

        let session_dir = memory.session_dir(session_id);
        let day1_file = session_dir.join("2026-03-18.md");
        let day2_file = session_dir.join("2026-03-19.md");
        assert!(day1_file.exists());
        assert!(day2_file.exists());

        let total = memory
            .session_size_bytes(session_id)
            .await
            .expect("session size should be available");
        assert!(total > 0);
    }

    #[tokio::test]
    async fn test_compact_if_needed_persists_summary() {
        let (_guard, memory) = new_temp_memory();
        let session_id = "session-compact";

        memory
            .append_turn_daily_md(
                session_id,
                &WorkspaceTurnRecord {
                    timestamp: Utc::now(),
                    user_input: "u1".to_string(),
                    assistant_output: "a1".to_string(),
                    reasoning: Vec::new(),
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                },
            )
            .await
            .expect("append turn should succeed");

        let summary = memory
            .compact_if_needed(session_id, 1, 16, |transcript| async move {
                Ok(format!("summary: {}", transcript))
            })
            .await
            .expect("compact should succeed")
            .expect("summary should be generated");
        assert!(summary.contains("summary:"));

        let saved = memory
            .load_session_summary(session_id)
            .await
            .expect("load summary should succeed")
            .expect("summary should exist");
        assert!(saved.contains("summary:"));
    }

    #[tokio::test]
    async fn test_compact_if_needed_skips_when_under_threshold() {
        let (_guard, memory) = new_temp_memory();
        let session_id = "session-skip-compact";

        let generated = memory
            .compact_if_needed(session_id, u64::MAX, 8, |_transcript| async move {
                Ok("should not run".to_string())
            })
            .await
            .expect("compact check should succeed");
        assert!(generated.is_none());
    }
}
