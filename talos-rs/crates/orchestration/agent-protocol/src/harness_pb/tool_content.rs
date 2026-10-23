use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::{
    JsonContentValue, TalosContentItem, TalosToolDiffHunk, TalosToolDiffLine, TalosToolFileDiff,
    TalosToolSearchMatch, TalosToolTaskStatus,
};

use super::collaboration::{decode_collaboration, encode_collaboration};
use super::invalid;

pub fn encode_tool_content(value: TalosContentItem) -> TalosResult<pb::ToolContent> {
    use pb::tool_content::Content;
    Ok(pb::ToolContent {
        content: Some(match value {
            TalosContentItem::Text { text } => Content::Text(text),
            TalosContentItem::Json { value } => Content::Json(pb::JsonContent {
                json_utf8: serde_json::to_string(value.as_inner()).map_err(|_| invalid())?,
            }),
            TalosContentItem::SubagentResult { result } => {
                Content::SubagentResult(encode_collaboration(result))
            }
            TalosContentItem::FileRead {
                path,
                offset,
                limit,
                total_lines,
                next_byte_offset,
                content,
                truncated,
            } => Content::FileRead(pb::FileRead {
                path,
                offset: offset as u64,
                limit: limit as u64,
                total_lines: total_lines.map(|value| value as u64),
                next_byte_offset,
                content,
                truncated,
            }),
            TalosContentItem::Diff { files } => Content::Diff(pb::Diff {
                files: files.into_iter().map(encode_file_diff).collect(),
            }),
            TalosContentItem::SearchResults {
                query,
                matches,
                offset,
                limit,
                total_matches,
                more_available,
                partial,
            } => Content::SearchResults(pb::SearchResults {
                query,
                matches: matches.into_iter().map(encode_search_match).collect(),
                offset: offset as u64,
                limit: limit as u64,
                total_matches: total_matches.map(|value| value as u64),
                more_available,
                partial,
            }),
            TalosContentItem::BashOutput {
                session_id,
                status,
                exit_code,
                stdout,
                stderr,
                truncated,
                elapsed_ms,
                output_log,
            } => Content::BashOutput(pb::BashOutput {
                session_id,
                status: encode_task_status(status),
                exit_code,
                stdout,
                stderr,
                truncated,
                elapsed_ms,
                output_log: output_log.map(encode_output_log),
            }),
            TalosContentItem::WebReference {
                url,
                title,
                citation_id,
                snippet,
                source_domain,
                published_at,
                provider_rank,
            } => Content::WebReference(pb::WebReference {
                url,
                title,
                citation_id,
                snippet,
                source_domain,
                published_at,
                provider_rank: provider_rank.map(|value| value as u64),
            }),
            TalosContentItem::Image {
                file_id,
                mime_type,
                label,
            } => Content::Image(pb::Image {
                file_id,
                mime_type,
                label,
            }),
            TalosContentItem::ArtifactRef {
                artifact_id,
                mime_type,
                label,
            } => Content::ArtifactRef(pb::ArtifactRef {
                artifact_id,
                mime_type,
                label,
            }),
            TalosContentItem::McpResourceRef {
                server,
                uri,
                mime_type,
            } => Content::McpResourceRef(pb::McpResourceRef {
                server,
                uri,
                mime_type,
            }),
            TalosContentItem::Progress {
                message,
                elapsed_ms,
            } => Content::Progress(pb::ToolContentProgress {
                message,
                elapsed_ms,
            }),
        }),
    })
}

pub fn decode_tool_content(value: pb::ToolContent) -> TalosResult<TalosContentItem> {
    use pb::tool_content::Content;
    match value.content.ok_or_else(invalid)? {
        Content::Text(text) => Ok(TalosContentItem::Text { text }),
        Content::Json(value) => Ok(TalosContentItem::Json {
            value: JsonContentValue::new(
                serde_json::from_str(&value.json_utf8).map_err(|_| invalid())?,
            ),
        }),
        Content::SubagentResult(value) => Ok(TalosContentItem::SubagentResult {
            result: decode_collaboration(value)?,
        }),
        Content::FileRead(value) => Ok(TalosContentItem::FileRead {
            path: value.path,
            offset: usize::try_from(value.offset).map_err(|_| invalid())?,
            limit: usize::try_from(value.limit).map_err(|_| invalid())?,
            total_lines: value
                .total_lines
                .map(usize::try_from)
                .transpose()
                .map_err(|_| invalid())?,
            next_byte_offset: value.next_byte_offset,
            content: value.content,
            truncated: value.truncated,
        }),
        Content::Diff(value) => Ok(TalosContentItem::Diff {
            files: value
                .files
                .into_iter()
                .map(decode_file_diff)
                .collect::<TalosResult<_>>()?,
        }),
        Content::SearchResults(value) => Ok(TalosContentItem::SearchResults {
            query: value.query,
            matches: value.matches.into_iter().map(decode_search_match).collect(),
            offset: usize::try_from(value.offset).map_err(|_| invalid())?,
            limit: usize::try_from(value.limit).map_err(|_| invalid())?,
            total_matches: value
                .total_matches
                .map(usize::try_from)
                .transpose()
                .map_err(|_| invalid())?,
            more_available: value.more_available,
            partial: value.partial,
        }),
        Content::BashOutput(value) => Ok(TalosContentItem::BashOutput {
            session_id: value.session_id,
            status: decode_task_status(value.status)?,
            exit_code: value.exit_code,
            stdout: value.stdout,
            stderr: value.stderr,
            truncated: value.truncated,
            elapsed_ms: value.elapsed_ms,
            output_log: value.output_log.map(decode_output_log).transpose()?,
        }),
        Content::WebReference(value) => Ok(TalosContentItem::WebReference {
            url: value.url,
            title: value.title,
            citation_id: value.citation_id,
            snippet: value.snippet,
            source_domain: value.source_domain,
            published_at: value.published_at,
            provider_rank: value
                .provider_rank
                .map(usize::try_from)
                .transpose()
                .map_err(|_| invalid())?,
        }),
        Content::Image(value) => Ok(TalosContentItem::Image {
            file_id: value.file_id,
            mime_type: value.mime_type,
            label: value.label,
        }),
        Content::ArtifactRef(value) => Ok(TalosContentItem::ArtifactRef {
            artifact_id: value.artifact_id,
            mime_type: value.mime_type,
            label: value.label,
        }),
        Content::McpResourceRef(value) => Ok(TalosContentItem::McpResourceRef {
            server: value.server,
            uri: value.uri,
            mime_type: value.mime_type,
        }),
        Content::Progress(value) => Ok(TalosContentItem::Progress {
            message: value.message,
            elapsed_ms: value.elapsed_ms,
        }),
    }
}

fn encode_file_diff(value: TalosToolFileDiff) -> pb::FileDiff {
    pb::FileDiff {
        path: value.path,
        old_path: value.old_path,
        hunks: value.hunks.into_iter().map(encode_hunk).collect(),
        truncated: value.truncated,
    }
}

fn decode_file_diff(value: pb::FileDiff) -> TalosResult<TalosToolFileDiff> {
    Ok(TalosToolFileDiff {
        path: value.path,
        old_path: value.old_path,
        hunks: value
            .hunks
            .into_iter()
            .map(decode_hunk)
            .collect::<TalosResult<_>>()?,
        truncated: value.truncated,
    })
}

fn encode_hunk(value: TalosToolDiffHunk) -> pb::DiffHunk {
    pb::DiffHunk {
        old_start: value.old_start,
        old_lines: value.old_lines,
        new_start: value.new_start,
        new_lines: value.new_lines,
        lines: value
            .lines
            .into_iter()
            .map(|line| pb::DiffLine {
                line: Some(match line {
                    TalosToolDiffLine::Context { text } => pb::diff_line::Line::Context(text),
                    TalosToolDiffLine::Added { text } => pb::diff_line::Line::Added(text),
                    TalosToolDiffLine::Removed { text } => pb::diff_line::Line::Removed(text),
                }),
            })
            .collect(),
    }
}

fn decode_hunk(value: pb::DiffHunk) -> TalosResult<TalosToolDiffHunk> {
    Ok(TalosToolDiffHunk {
        old_start: value.old_start,
        old_lines: value.old_lines,
        new_start: value.new_start,
        new_lines: value.new_lines,
        lines: value
            .lines
            .into_iter()
            .map(|line| match line.line.ok_or_else(invalid)? {
                pb::diff_line::Line::Context(text) => Ok(TalosToolDiffLine::Context { text }),
                pb::diff_line::Line::Added(text) => Ok(TalosToolDiffLine::Added { text }),
                pb::diff_line::Line::Removed(text) => Ok(TalosToolDiffLine::Removed { text }),
            })
            .collect::<TalosResult<_>>()?,
    })
}

fn encode_search_match(value: TalosToolSearchMatch) -> pb::SearchMatch {
    pb::SearchMatch {
        path: value.path,
        line: value.line,
        column: value.column,
        text: value.text,
        before: value.before,
        after: value.after,
    }
}

fn decode_search_match(value: pb::SearchMatch) -> TalosToolSearchMatch {
    TalosToolSearchMatch {
        path: value.path,
        line: value.line,
        column: value.column,
        text: value.text,
        before: value.before,
        after: value.after,
    }
}

fn encode_task_status(value: TalosToolTaskStatus) -> i32 {
    match value {
        TalosToolTaskStatus::Queued => pb::TaskStatus::Queued as i32,
        TalosToolTaskStatus::Running => pb::TaskStatus::Running as i32,
        TalosToolTaskStatus::Completed => pb::TaskStatus::Completed as i32,
        TalosToolTaskStatus::Failed => pb::TaskStatus::Failed as i32,
        TalosToolTaskStatus::TimedOut => pb::TaskStatus::TimedOut as i32,
        TalosToolTaskStatus::Cancelled => pb::TaskStatus::Cancelled as i32,
    }
}

fn decode_task_status(value: i32) -> TalosResult<TalosToolTaskStatus> {
    match pb::TaskStatus::try_from(value) {
        Ok(pb::TaskStatus::Queued) => Ok(TalosToolTaskStatus::Queued),
        Ok(pb::TaskStatus::Running) => Ok(TalosToolTaskStatus::Running),
        Ok(pb::TaskStatus::Completed) => Ok(TalosToolTaskStatus::Completed),
        Ok(pb::TaskStatus::Failed) => Ok(TalosToolTaskStatus::Failed),
        Ok(pb::TaskStatus::TimedOut) => Ok(TalosToolTaskStatus::TimedOut),
        Ok(pb::TaskStatus::Cancelled) => Ok(TalosToolTaskStatus::Cancelled),
        _ => Err(invalid()),
    }
}

fn encode_output_log(log: talos_types::talos::BashOutputLog) -> pb::BashOutputLog {
    use talos_types::talos::BashOutputCaptureStatus as S;
    pb::BashOutputLog {
        path: log.path,
        relative_path: log.relative_path,
        total_bytes: log.total_bytes,
        stored_bytes: log.stored_bytes,
        total_lines: log.total_lines,
        expires_at: log.expires_at,
        capture_status: match log.capture_status {
            S::Capturing => 1,
            S::Complete => 2,
            S::QuotaLimited => 3,
            S::Interrupted => 4,
            S::WriteFailed => 5,
        },
    }
}

fn decode_output_log(log: pb::BashOutputLog) -> TalosResult<talos_types::talos::BashOutputLog> {
    use talos_types::talos::BashOutputCaptureStatus as S;
    let capture_status = match log.capture_status {
        1 => S::Capturing,
        2 => S::Complete,
        3 => S::QuotaLimited,
        4 => S::Interrupted,
        5 => S::WriteFailed,
        _ => return Err(invalid()),
    };
    Ok(talos_types::talos::BashOutputLog {
        path: log.path,
        relative_path: log.relative_path,
        total_bytes: log.total_bytes,
        stored_bytes: log.stored_bytes,
        total_lines: log.total_lines,
        capture_status,
        expires_at: log.expires_at,
    })
}
