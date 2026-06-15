use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProvider};
use tokio::fs;

use crate::path_policy;
use crate::store::{
    MemoryGetResult, MemoryIndexFile, MemoryIndexSection, MemorySearchQuery, MemorySearchResult,
    MemoryStore, RetrievedMemoryItem,
};

const INDEX_PREVIEW_LIMIT: usize = 100;
const HOP1_FILE_SECTION_LIMIT: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemorySearchResponse {
    #[serde(default)]
    pub results: Vec<MemorySearchResult>,
}

impl MemorySearchResponse {
    pub fn into_retrieved_items(self) -> Vec<RetrievedMemoryItem> {
        self.results
            .into_iter()
            .map(|item| RetrievedMemoryItem {
                path: item.path,
                snippet: item.snippet,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct SectionRef {
    path: String,
    from: usize,
    to: usize,
}

pub fn strip_json_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    }
}

fn relative_source_path(workspace_dir: &Path, file: &Path) -> String {
    file.strip_prefix(workspace_dir)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn index_file_path_for(md_path: &Path) -> PathBuf {
    md_path.with_extension("index.json")
}

fn truncate_preview(input: &str, max_chars: usize) -> String {
    let trimmed = input.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_chars).collect::<String>()
}

fn parse_heading_title(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with("## ") {
        return None;
    }
    let without_prefix = trimmed.trim_start_matches("## ").trim();
    if let Some(rest) = without_prefix.strip_prefix('[') {
        let closing = rest.find(']')?;
        return Some(rest[closing + 1..].trim().to_string());
    }
    Some(without_prefix.to_string())
}

fn extract_session_id(section_lines: &[&str]) -> String {
    section_lines
        .iter()
        .find_map(|line| {
            line.trim()
                .strip_prefix("session: ")
                .map(|value| value.trim().to_string())
        })
        .unwrap_or_default()
}

fn extract_summary_preview(section_lines: &[&str]) -> String {
    let mut parts = Vec::new();
    for line in section_lines {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("## ")
            || trimmed.starts_with("session: ")
            || trimmed.starts_with("**")
        {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            parts.push(item.trim().to_string());
        } else {
            parts.push(trimmed.to_string());
        }
        if parts.len() >= 2 {
            break;
        }
    }
    truncate_preview(&parts.join(" "), INDEX_PREVIEW_LIMIT)
}

fn source_content_hash(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn build_index_file_from_content(
    workspace_dir: &Path,
    md_path: &Path,
    content: &str,
) -> MemoryIndexFile {
    let lines: Vec<&str> = content.lines().collect();
    let mut sections = Vec::new();
    let mut section_start_indices = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if line.trim().starts_with("## ") {
            section_start_indices.push(idx);
        }
    }

    for (position, start_idx) in section_start_indices.iter().enumerate() {
        let end_idx = section_start_indices
            .get(position + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or_else(|| lines.len().saturating_sub(1));
        let section_lines = &lines[*start_idx..=end_idx];
        let Some(title) = parse_heading_title(section_lines[0]) else {
            continue;
        };
        sections.push(MemoryIndexSection {
            title,
            session_id: extract_session_id(section_lines),
            summary_preview: extract_summary_preview(section_lines),
            start_line: start_idx + 1,
            end_line: end_idx + 1,
        });
    }

    let path = relative_source_path(workspace_dir, md_path);
    let date = md_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    MemoryIndexFile {
        path,
        date,
        source_content_hash: source_content_hash(content),
        sections,
    }
}

pub(crate) async fn rebuild_file_index(
    store: &MemoryStore,
    date: &str,
) -> TalosResult<MemoryIndexFile> {
    let md_path = store.workspace_memory_dir().join(format!("{date}.md"));
    let content = fs::read_to_string(&md_path)
        .await
        .map_err(|e| TalosError::StorageError(format!("read workspace memory file: {}", e)))?;
    let index_file = build_index_file_from_content(store.workspace_dir(), &md_path, &content);
    let json = serde_json::to_string_pretty(&index_file)
        .map_err(|e| TalosError::StorageError(format!("serialize memory index: {}", e)))?;
    fs::write(index_file_path_for(&md_path), json)
        .await
        .map_err(|e| TalosError::StorageError(format!("write memory index file: {}", e)))?;
    Ok(index_file)
}

pub(crate) async fn rebuild_file_index_for_timestamp(
    store: &MemoryStore,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> TalosResult<MemoryIndexFile> {
    let date = timestamp
        .with_timezone(&store.timezone())
        .format("%Y-%m-%d")
        .to_string();
    rebuild_file_index(store, &date).await
}

pub(crate) async fn rebuild_all_indexes(store: &MemoryStore) -> TalosResult<()> {
    for file in path_policy::list_workspace_memory_files(store).await? {
        let Some(date) = file.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        rebuild_file_index(store, date).await?;
    }
    Ok(())
}

async fn load_index_files(store: &MemoryStore) -> TalosResult<Vec<MemoryIndexFile>> {
    let daily_files = path_policy::list_workspace_memory_files(store).await?;
    if daily_files.is_empty() {
        return Ok(Vec::new());
    }

    let has_missing_index = daily_files
        .iter()
        .any(|path| !index_file_path_for(path).exists());
    if has_missing_index {
        rebuild_all_indexes(store).await?;
    }

    let mut indexes = Vec::new();
    for file in daily_files {
        let index_path = index_file_path_for(&file);
        let content = fs::read_to_string(&index_path)
            .await
            .map_err(|e| TalosError::StorageError(format!("read memory index file: {}", e)))?;
        let parsed = serde_json::from_str::<MemoryIndexFile>(&content)
            .map_err(|e| TalosError::StorageError(format!("parse memory index file: {}", e)))?;
        let source = fs::read_to_string(&file)
            .await
            .map_err(|e| TalosError::StorageError(format!("read memory source file: {}", e)))?;
        if parsed.source_content_hash == source_content_hash(&source) {
            indexes.push(parsed);
            continue;
        }

        let Some(date) = file.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        indexes.push(rebuild_file_index(store, date).await?);
    }
    indexes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(indexes)
}

fn format_file_summaries(indexes: &[MemoryIndexFile]) -> String {
    indexes
        .iter()
        .map(|file| {
            let mut output = format!("{} ({} sections)", file.path, file.sections.len());
            for section in file.sections.iter().take(HOP1_FILE_SECTION_LIMIT) {
                output.push_str(&format!("\n  - {}", section.title));
                if !section.summary_preview.is_empty() {
                    output.push_str(&format!(" | {}", section.summary_preview));
                }
            }
            output
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_candidate_sections(files: &[MemoryIndexFile]) -> String {
    files
        .iter()
        .map(|file| {
            let mut output = format!("{}:", file.path);
            for section in &file.sections {
                output.push_str(&format!(
                    "\n  L{}-{}: \"{}\"\n          session: {} | \"{}\"",
                    section.start_line,
                    section.end_line,
                    section.title,
                    section.session_id,
                    section.summary_preview
                ));
            }
            output
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn select_candidate_files(
    llm: Arc<dyn LlmProvider>,
    query: &str,
    indexes: &[MemoryIndexFile],
    max_results: usize,
) -> TalosResult<Vec<String>> {
    let request = ChatRequest::builder()
        .model(llm.default_model())
        .messages(vec![
            ChatMessage::system(
                "You select relevant memory files for a query. Respond ONLY with a valid JSON array of file paths.",
            ),
            ChatMessage::user(format!(
                "Query: \"{}\"\n\nMemory files:\n{}\n\nReturn at most {} relevant file paths as JSON array in the form [\"memory/YYYY-MM-DD.md\"].",
                query,
                format_file_summaries(indexes),
                max_results.max(1)
            )),
        ])
        .build();
    let response = llm.chat(request).await?;
    serde_json::from_str::<Vec<String>>(strip_json_fences(&response.content))
        .map_err(|e| TalosError::ToolError(format!("parse hop1 memory search response: {}", e)))
}

async fn select_candidate_sections(
    llm: Arc<dyn LlmProvider>,
    query: &str,
    files: &[MemoryIndexFile],
    max_results: usize,
) -> TalosResult<Vec<SectionRef>> {
    let request = ChatRequest::builder()
        .model(llm.default_model())
        .messages(vec![
            ChatMessage::system(
                "You select relevant memory sections for a query. Respond ONLY with a valid JSON array.",
            ),
            ChatMessage::user(format!(
                "Query: \"{}\"\n\nCandidate sections:\n{}\n\nReturn at most {} items as JSON array in the form [{{\"path\":\"memory/...\",\"from\":1,\"to\":2}}].",
                query,
                format_candidate_sections(files),
                max_results.max(1)
            )),
        ])
        .build();
    let response = llm.chat(request).await?;
    serde_json::from_str::<Vec<SectionRef>>(strip_json_fences(&response.content))
        .map_err(|e| TalosError::ToolError(format!("parse hop2 memory search response: {}", e)))
}

fn dedupe_section_refs(section_refs: Vec<SectionRef>) -> Vec<SectionRef> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for item in section_refs {
        let key = format!("{}:{}:{}", item.path, item.from, item.to);
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

fn retain_candidate_section_refs(
    section_refs: Vec<SectionRef>,
    candidate_files: &[MemoryIndexFile],
) -> Vec<SectionRef> {
    let allowed = candidate_files
        .iter()
        .flat_map(|file| {
            file.sections.iter().map(|section| SectionRef {
                path: file.path.clone(),
                from: section.start_line,
                to: section.end_line,
            })
        })
        .collect::<HashSet<_>>();

    section_refs
        .into_iter()
        .filter(|section_ref| allowed.contains(section_ref))
        .collect()
}

pub(crate) async fn semantic_search(
    store: &MemoryStore,
    query: &MemorySearchQuery,
) -> TalosResult<Vec<MemorySearchResult>> {
    let query_text = query.query.trim();
    if query_text.is_empty() {
        return Ok(Vec::new());
    }
    let Some(llm) = store.llm_provider() else {
        return Err(TalosError::ToolError(
            "memory search requires an llm-backed memory store".to_string(),
        ));
    };

    let indexes = load_index_files(store).await?;
    if indexes.is_empty() {
        return Ok(Vec::new());
    }

    let candidate_paths =
        select_candidate_files(llm.clone(), query_text, &indexes, query.max_results.max(3)).await?;
    if candidate_paths.is_empty() {
        return Ok(Vec::new());
    }

    let candidate_files = indexes
        .into_iter()
        .filter(|file| candidate_paths.iter().any(|path| path == &file.path))
        .collect::<Vec<_>>();
    if candidate_files.is_empty() {
        return Ok(Vec::new());
    }

    let section_refs = retain_candidate_section_refs(
        dedupe_section_refs(
            select_candidate_sections(llm, query_text, &candidate_files, query.max_results).await?,
        ),
        &candidate_files,
    );
    if section_refs.is_empty() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for section in section_refs.into_iter().take(query.max_results) {
        if section.to < section.from {
            continue;
        }
        let fetched = get_memory(
            store,
            &section.path,
            Some(section.from),
            Some(section.to - section.from + 1),
        )
        .await?;
        let snippet = fetched.text.trim().to_string();
        if snippet.is_empty() {
            continue;
        }
        results.push(MemorySearchResult {
            path: fetched.path,
            snippet,
            start_line: section.from,
            end_line: section.to,
        });
    }
    Ok(results)
}

pub(crate) async fn get_memory(
    store: &MemoryStore,
    path: &str,
    from: Option<usize>,
    lines: Option<usize>,
) -> TalosResult<MemoryGetResult> {
    let normalized = path_policy::normalize_memory_path(path)?;
    let abs_path = store.workspace_dir().join(&normalized);
    let canonical_workspace = fs::canonicalize(store.workspace_dir())
        .await
        .map_err(|e| TalosError::StorageError(format!("canonicalize workspace dir: {}", e)))?;
    let canonical_path = fs::canonicalize(&abs_path)
        .await
        .map_err(|e| TalosError::StorageError(format!("canonicalize memory file: {}", e)))?;
    if !canonical_path.starts_with(&canonical_workspace) {
        return Err(TalosError::ValidationError(format!(
            "memory path is outside workspace: {}",
            path.trim()
        )));
    }
    let content = fs::read_to_string(&abs_path)
        .await
        .map_err(|e| TalosError::StorageError(format!("read memory file: {}", e)))?;
    let from = from.unwrap_or(1).max(1);
    let line_count = lines.unwrap_or(20).max(1);
    let selected = content
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let line_no = idx + 1;
            if line_no < from || line_no >= from + line_count {
                None
            } else {
                Some(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(MemoryGetResult {
        path: normalized.to_string_lossy().replace('\\', "/"),
        text: selected,
        from,
        lines: line_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use talos_error::TalosResult;
    use talos_llm_provider::{
        ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, ReasoningSupport, Usage,
    };
    use tempfile::TempDir;

    use crate::{Memory, WorkspaceMemoryRecord};

    #[derive(Clone)]
    struct MockLlmProvider {
        responses: Arc<tokio::sync::Mutex<Vec<String>>>,
    }

    impl MockLlmProvider {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Arc::new(tokio::sync::Mutex::new(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockLlmProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            let mut responses = self.responses.lock().await;
            let content = responses.remove(0);
            Ok(ChatResponse {
                content,
                reasoning: Vec::new(),
                model: "mock".to_string(),
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: Some(Vec::<FunctionCall>::new()),
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            unreachable!("stream is not used in tests")
        }

        fn default_model(&self) -> String {
            "mock-model".to_string()
        }

        fn reasoning_support(&self, _model: &str) -> ReasoningSupport {
            ReasoningSupport::Unsupported
        }
    }

    fn new_temp_store_with_llm(responses: Vec<String>) -> (TempDir, MemoryStore) {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let llm = Arc::new(MockLlmProvider::new(responses));
        let store = MemoryStore::new_with_llm(temp_dir.path().to_path_buf(), llm);
        (temp_dir, store)
    }

    #[tokio::test]
    async fn test_rebuild_file_index_extracts_sections() {
        let (_guard, store) = new_temp_store_with_llm(Vec::new());
        let timestamp = Utc::now();
        store
            .append_workspace_memory(&WorkspaceMemoryRecord {
                timestamp,
                session_id: "session-1".to_string(),
                title: "Flush required".to_string(),
                content: "**Facts:**\n- flush on close\n\n**Preferences:**\n- ephemeral sessions\n"
                    .to_string(),
            })
            .await
            .expect("workspace memory append should succeed");

        let date = timestamp
            .with_timezone(&store.timezone())
            .format("%Y-%m-%d")
            .to_string();
        let index = rebuild_file_index(&store, &date)
            .await
            .expect("rebuild should succeed");
        assert_eq!(index.sections.len(), 1);
        assert_eq!(index.sections[0].title, "Flush required");
        assert_eq!(index.sections[0].session_id, "session-1");
        assert!(index.sections[0].summary_preview.contains("flush on close"));
    }

    #[tokio::test]
    async fn test_rebuild_all_indexes_bootstraps_existing_md_files() {
        let (_guard, store) = new_temp_store_with_llm(Vec::new());
        let memory_dir = store.workspace_memory_dir();
        tokio::fs::create_dir_all(&memory_dir)
            .await
            .expect("memory dir should exist");
        tokio::fs::write(
            memory_dir.join("2026-04-04.md"),
            "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- keep no residue\n",
        )
        .await
        .expect("seed memory file should write");

        rebuild_all_indexes(&store)
            .await
            .expect("rebuild all should succeed");

        let loaded = load_index_files(&store)
            .await
            .expect("load indexes should succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].path, "memory/2026-04-04.md");
    }

    #[tokio::test]
    async fn test_semantic_search_uses_two_hops() {
        let (_guard, store) = new_temp_store_with_llm(vec![
            "[\"memory/2026-04-04.md\"]".to_string(),
            "[{\"path\":\"memory/2026-04-04.md\",\"from\":1,\"to\":5}]".to_string(),
        ]);
        let memory_dir = store.workspace_memory_dir();
        tokio::fs::create_dir_all(&memory_dir)
            .await
            .expect("memory dir should exist");
        tokio::fs::write(
            memory_dir.join("2026-04-04.md"),
            "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- flush on close\n",
        )
        .await
        .expect("seed memory file should write");
        rebuild_all_indexes(&store)
            .await
            .expect("rebuild all should succeed");

        let results = store
            .search_memory(&MemorySearchQuery {
                query: "flush".to_string(),
                max_results: 3,
            })
            .await
            .expect("search should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "memory/2026-04-04.md");
        assert!(results[0].snippet.contains("flush on close"));
    }

    #[tokio::test]
    async fn test_semantic_search_rejects_hop2_refs_outside_candidate_sections() {
        let (_guard, store) = new_temp_store_with_llm(vec![
            "[\"memory/2026-04-04.md\"]".to_string(),
            "[{\"path\":\"memory/2026-04-05.md\",\"from\":1,\"to\":6},{\"path\":\"memory/2026-04-04.md\",\"from\":1,\"to\":99}]".to_string(),
        ]);
        let memory_dir = store.workspace_memory_dir();
        tokio::fs::create_dir_all(&memory_dir)
            .await
            .expect("memory dir should exist");
        tokio::fs::write(
            memory_dir.join("2026-04-04.md"),
            "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- flush on close\n",
        )
        .await
        .expect("seed memory file should write");
        tokio::fs::write(
            memory_dir.join("2026-04-05.md"),
            "## [11:01] Unrelated\nsession: session-2\n\n**Facts:**\n- unrelated content\n",
        )
        .await
        .expect("second seed memory file should write");
        rebuild_all_indexes(&store)
            .await
            .expect("rebuild all should succeed");

        let results = store
            .search_memory(&MemorySearchQuery {
                query: "flush".to_string(),
                max_results: 3,
            })
            .await
            .expect("search should succeed");
        assert!(results.is_empty(), "invalid hop2 refs should be discarded");
    }

    #[tokio::test]
    async fn test_semantic_search_ignores_hop2_refs_with_inverted_range() {
        let (_guard, store) = new_temp_store_with_llm(vec![
            "[\"memory/2026-04-04.md\"]".to_string(),
            "[{\"path\":\"memory/2026-04-04.md\",\"from\":5,\"to\":1}]".to_string(),
        ]);
        let memory_dir = store.workspace_memory_dir();
        tokio::fs::create_dir_all(&memory_dir)
            .await
            .expect("memory dir should exist");
        tokio::fs::write(
            memory_dir.join("2026-04-04.md"),
            "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- flush on close\n",
        )
        .await
        .expect("seed memory file should write");
        rebuild_all_indexes(&store)
            .await
            .expect("rebuild all should succeed");

        let results = store
            .search_memory(&MemorySearchQuery {
                query: "flush".to_string(),
                max_results: 3,
            })
            .await
            .expect("search should succeed");
        assert!(results.is_empty(), "inverted hop2 refs should be ignored");
    }

    #[tokio::test]
    async fn test_semantic_search_errors_on_invalid_hop2_json() {
        let (_guard, store) = new_temp_store_with_llm(vec![
            "[\"memory/2026-04-04.md\"]".to_string(),
            "{\"path\":\"memory/2026-04-04.md\"}".to_string(),
        ]);
        let memory_dir = store.workspace_memory_dir();
        tokio::fs::create_dir_all(&memory_dir)
            .await
            .expect("memory dir should exist");
        tokio::fs::write(
            memory_dir.join("2026-04-04.md"),
            "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- flush on close\n",
        )
        .await
        .expect("seed memory file should write");
        rebuild_all_indexes(&store)
            .await
            .expect("rebuild all should succeed");

        let error = store
            .search_memory(&MemorySearchQuery {
                query: "flush".to_string(),
                max_results: 3,
            })
            .await
            .expect_err("invalid hop2 json should fail");
        assert!(matches!(error, TalosError::ToolError(_)));
    }

    #[tokio::test]
    async fn test_semantic_search_requires_llm_backed_store() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let store = MemoryStore::new(temp_dir.path().to_path_buf());
        let result = store
            .search_memory(&MemorySearchQuery {
                query: "anything".to_string(),
                max_results: 1,
            })
            .await;
        assert!(matches!(result, Err(TalosError::ToolError(_))));
    }

    #[test]
    fn test_strip_json_fences_supports_markdown_wrappers() {
        assert_eq!(strip_json_fences("```json\n[1,2]\n```"), "[1,2]");
        assert_eq!(strip_json_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
    }
}
