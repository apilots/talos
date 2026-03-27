use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use tokio::fs;

use crate::path_policy;
use crate::store::{
    MemoryGetResult, MemorySearchQuery, MemorySearchResult, MemoryStore, RetrievedMemoryItem,
};

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
                citation: item.citation,
                score: item.score,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct RetrievalSource {
    path: String,
    content: String,
}

#[derive(Debug, Clone)]
struct RetrievalChunk {
    path: String,
    text: String,
    start_line: usize,
    end_line: usize,
}

pub(crate) fn build_citation(path: &str, start_line: usize, end_line: usize) -> String {
    if start_line == end_line {
        format!("{path}#L{start_line}")
    } else {
        format!("{path}#L{start_line}-L{end_line}")
    }
}

fn normalized_query(query: &str) -> Option<String> {
    let normalized = query.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn score_match(haystack: &str, query: &str) -> f32 {
    let normalized_haystack = haystack.to_ascii_lowercase();
    let normalized_query = query.to_ascii_lowercase();
    if normalized_query.is_empty() {
        return 0.0;
    }
    let occurrences = normalized_haystack.matches(&normalized_query).count();
    if occurrences == 0 {
        return 0.0;
    }
    let covered_chars =
        (occurrences.saturating_mul(normalized_query.len())).min(normalized_haystack.len());
    covered_chars as f32 / normalized_haystack.len().max(1) as f32
}

fn push_chunk(
    chunks: &mut Vec<RetrievalChunk>,
    path: &str,
    lines: &[&str],
    start_line: usize,
    end_line: usize,
) {
    let text = lines
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if text.is_empty() {
        return;
    }
    chunks.push(RetrievalChunk {
        path: path.to_string(),
        text,
        start_line,
        end_line,
    });
}

fn chunk_source(source: &RetrievalSource) -> Vec<RetrievalChunk> {
    let lines: Vec<&str> = source.content.lines().collect();
    let mut chunks = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_start = 1usize;

    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();

        let should_split_before = (!current_lines.is_empty() && trimmed.starts_with('#'))
            || (!current_lines.is_empty() && trimmed.is_empty());
        if should_split_before {
            push_chunk(
                &mut chunks,
                &source.path,
                &current_lines,
                current_start,
                line_no.saturating_sub(1),
            );
            current_lines.clear();
        }

        if trimmed.is_empty() {
            current_start = line_no.saturating_add(1);
            continue;
        }

        if current_lines.is_empty() {
            current_start = line_no;
        }
        current_lines.push(line);
    }

    if !current_lines.is_empty() {
        push_chunk(
            &mut chunks,
            &source.path,
            &current_lines,
            current_start,
            lines.len().max(current_start),
        );
    }

    chunks
}

fn rank_chunk(chunk: &RetrievalChunk, query: &str) -> f32 {
    let text_score = score_match(&chunk.text, query);
    if text_score <= 0.0 {
        return 0.0;
    }

    let heading_score = chunk
        .text
        .lines()
        .next()
        .map(|line| {
            if line.trim_start().starts_with('#') {
                score_match(line, query)
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    ((text_score * 0.8) + (heading_score * 0.2)).clamp(0.0, 1.0)
}

fn render_chunk(chunk: &RetrievalChunk, score: f32) -> MemorySearchResult {
    MemorySearchResult {
        path: chunk.path.clone(),
        snippet: chunk.text.clone(),
        citation: Some(build_citation(
            &chunk.path,
            chunk.start_line,
            chunk.end_line,
        )),
        score,
        start_line: chunk.start_line,
        end_line: chunk.end_line,
    }
}

fn dedupe_results(results: Vec<MemorySearchResult>) -> Vec<MemorySearchResult> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for item in results {
        let key = format!("{}:{}", item.path, item.snippet.to_ascii_lowercase());
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

fn relative_source_path(workspace_dir: &Path, file: &PathBuf) -> String {
    file.strip_prefix(workspace_dir)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

async fn load_sources(store: &MemoryStore) -> TalosResult<Vec<RetrievalSource>> {
    let mut sources = Vec::new();

    for file in path_policy::list_workspace_memory_files(store).await? {
        let content = fs::read_to_string(&file)
            .await
            .map_err(|e| TalosError::StorageError(format!("read workspace memory file: {}", e)))?;
        if content.trim().is_empty() {
            continue;
        }
        sources.push(RetrievalSource {
            path: relative_source_path(store.workspace_dir(), &file),
            content,
        });
    }

    Ok(sources)
}

pub(crate) async fn search_memory(
    store: &MemoryStore,
    query: &MemorySearchQuery,
) -> TalosResult<Vec<MemorySearchResult>> {
    let Some(query_text) = normalized_query(&query.query) else {
        return Ok(Vec::new());
    };

    let mut results = load_sources(store)
        .await?
        .into_iter()
        .flat_map(|source| chunk_source(&source))
        .filter_map(|chunk| {
            let score = rank_chunk(&chunk, &query_text);
            if score <= 0.0 {
                None
            } else {
                Some(render_chunk(&chunk, score))
            }
        })
        .collect::<Vec<_>>();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.path.cmp(&b.path))
            .then(a.start_line.cmp(&b.start_line))
    });
    results = dedupe_results(results);

    if let Some(min_score) = query.min_score {
        results.retain(|item| item.score >= min_score);
    }
    if results.len() > query.max_results {
        results.truncate(query.max_results);
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
    use chrono::Utc;
    use tempfile::TempDir;

    use crate::store::{MemorySearchQuery, MemoryStore};
    use crate::{Memory, WorkspaceMemoryRecord};

    use super::{chunk_source, normalized_query, rank_chunk, RetrievalChunk, RetrievalSource};

    fn new_temp_store() -> (TempDir, MemoryStore) {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let store = MemoryStore::new(temp_dir.path().to_path_buf());
        (temp_dir, store)
    }

    #[test]
    fn test_chunk_source_splits_on_headings_and_blank_lines() {
        let source = RetrievalSource {
            path: "MEMORY.md".to_string(),
            content: "# Preferences\nTea only\n\n# Decisions\nUse blue green\n".to_string(),
        };

        let chunks = chunk_source(&source);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("Tea only"));
        assert!(chunks[1].text.contains("Use blue green"));
    }

    #[test]
    fn test_rank_chunk_prefers_heading_match() {
        let query = normalized_query("preferences").expect("query should normalize");
        let chunk = RetrievalChunk {
            path: "MEMORY.md".to_string(),
            text: "# Preferences\nTea only".to_string(),
            start_line: 1,
            end_line: 2,
        };

        let score = rank_chunk(&chunk, &query);
        assert!(score > 0.0);
        assert!(score <= 1.0);
    }

    #[tokio::test]
    async fn test_search_memory_returns_block_snippet() {
        let (_guard, store) = new_temp_store();
        store
            .append_workspace_memory(&WorkspaceMemoryRecord {
                timestamp: Utc::now(),
                title: Some("Daily".to_string()),
                content: "# Preferences\nTea only\nNo coffee\n\n# Decisions\nUse blue green"
                    .to_string(),
            })
            .await
            .expect("workspace memory append should succeed");

        let results = store
            .search_memory(&MemorySearchQuery {
                query: "tea".to_string(),
                max_results: 5,
                min_score: Some(0.0),
            })
            .await
            .expect("search should succeed");

        assert!(!results.is_empty());
        assert!(results[0].path.starts_with("memory/"));
        assert!(results[0].snippet.contains("Tea only"));
        assert!(results[0].snippet.contains("No coffee"));
        assert_eq!(results[0].start_line, 2);
        assert_eq!(results[0].end_line, 4);
    }

    #[tokio::test]
    async fn test_search_memory_dedupes_repeated_chunks() {
        let (_guard, store) = new_temp_store();
        store
            .append_workspace_memory(&WorkspaceMemoryRecord {
                timestamp: Utc::now(),
                title: Some("Daily".to_string()),
                content: "deployment decision: blue green".to_string(),
            })
            .await
            .expect("workspace memory append should succeed");
        store
            .append_workspace_memory(&WorkspaceMemoryRecord {
                timestamp: Utc::now(),
                title: Some("Daily".to_string()),
                content: "deployment decision: blue green".to_string(),
            })
            .await
            .expect("workspace memory append should succeed");

        let results = store
            .search_memory(&MemorySearchQuery {
                query: "blue green".to_string(),
                max_results: 10,
                min_score: Some(0.0),
            })
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
    }
}
