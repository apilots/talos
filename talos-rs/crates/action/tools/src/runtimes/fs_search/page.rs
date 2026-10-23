//! Stream a bounded page, but keep counting discarded matches until EOF.
use std::collections::VecDeque;

use serde_json::Value;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::talos::TalosToolSearchMatch;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

use super::{GrepFilesRequest, GrepOutputMode, MAX_LIMIT};

const MAX_FRAME_BYTES: usize = 256 * 1024;
const MAX_MATCH_BYTES: usize = 500;
const MAX_CONTEXT_LINES: usize = 20;

pub(super) struct SearchPage {
    pub matches: Vec<TalosToolSearchMatch>,
    pub total: usize,
}

fn line_text(data: &Value) -> String {
    crate::output_budget::truncate_text(
        data["lines"]["text"]
            .as_str()
            .unwrap_or_default()
            .trim_end_matches(['\r', '\n']),
        MAX_MATCH_BYTES,
    )
}

pub(super) async fn collect(
    reader: impl AsyncRead + Unpin,
    request: GrepFilesRequest,
    budget: usize,
    stopped: tokio_util::sync::CancellationToken,
) -> TalosResult<SearchPage> {
    let mut reader = BufReader::with_capacity(4096, reader);
    let mut page = SearchPage {
        matches: Vec::new(),
        total: 0,
    };
    let limit = request.head_limit.unwrap_or(request.limit).min(MAX_LIMIT);
    let before_limit = request
        .before_context
        .unwrap_or(request.context)
        .min(MAX_CONTEXT_LINES);
    let after_limit = request
        .after_context
        .unwrap_or(request.context)
        .min(MAX_CONTEXT_LINES);
    let mut pending_before = VecDeque::new();
    let mut current: Option<usize> = None;
    let mut used = 0_usize;
    let mut full = false;
    let mut last_path = String::new();
    let mut frame = Vec::new();
    loop {
        frame.clear();
        (&mut reader)
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut frame)
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Read search output; technical details were withheld.",
                    error,
                )
            })?;
        if frame.is_empty() {
            break;
        }
        if frame.len() > MAX_FRAME_BYTES {
            return Err(TalosError::new(ErrorCode::UsageLimitExceeded, "A search record exceeds its size limit. Narrow the search or use Read with byte_offset and max_bytes."));
        }
        let event: Value = match serde_json::from_slice(&frame) {
            Ok(event) => event,
            Err(_) if stopped.is_cancelled() && !frame.ends_with(b"\n") => break,
            Err(error) => {
                return Err(TalosError::internal_with_source(
                    "Search returned invalid JSON.",
                    error,
                ))
            }
        };
        let data = &event["data"];
        match event["type"].as_str() {
            Some("begin" | "end") => {
                current = None;
                pending_before.clear();
            }
            Some("context") if request.output_mode == GrepOutputMode::Content => {
                let text = line_text(data);
                if let Some(index) = current {
                    let item = &mut page.matches[index];
                    let extra =
                        serde_json::to_vec(&text).map_or(usize::MAX, |bytes| bytes.len() + 1);
                    if item.after.len() < after_limit && used.saturating_add(extra) <= budget {
                        used += extra;
                        item.after.push(text.clone());
                    }
                }
                if before_limit > 0 {
                    pending_before.push_back(text);
                    while pending_before.len() > before_limit {
                        pending_before.pop_front();
                    }
                }
            }
            Some("match") => {
                current = None;
                let path = data["path"]["text"].as_str().unwrap_or_default();
                if request.output_mode == GrepOutputMode::FilesWithMatches {
                    // ripgrep groups a file's records between begin/end; only
                    // retain the previous path, not an unbounded unique set.
                    if path == last_path {
                        continue;
                    }
                    last_path = path.to_owned();
                }
                let index = page.total;
                page.total = page.total.saturating_add(1);
                if request.output_mode == GrepOutputMode::Count
                    || index < request.offset
                    || full
                    || page.matches.len() >= limit
                {
                    continue;
                }
                let content = request.output_mode == GrepOutputMode::Content;
                let mut item = TalosToolSearchMatch {
                    path: path.to_owned(),
                    line: if content {
                        data["line_number"]
                            .as_u64()
                            .and_then(|value| value.try_into().ok())
                            .unwrap_or(0)
                    } else {
                        0
                    },
                    column: if content {
                        data["submatches"][0]["start"]
                            .as_u64()
                            .and_then(|value| value.checked_add(1)?.try_into().ok())
                    } else {
                        None
                    },
                    text: if content {
                        line_text(data)
                    } else {
                        String::new()
                    },
                    before: if content {
                        pending_before.drain(..).collect()
                    } else {
                        Vec::new()
                    },
                    after: Vec::new(),
                };
                // Drop optional context before declaring even one row too big.
                let mut size =
                    serde_json::to_vec(&item).map_or(usize::MAX, |bytes| bytes.len() + 1);
                if used.saturating_add(size) > budget {
                    item.before.clear();
                    size = serde_json::to_vec(&item).map_or(usize::MAX, |bytes| bytes.len() + 1);
                }
                if used.saturating_add(size) > budget {
                    if page.matches.is_empty() {
                        return Err(TalosError::new(
                            ErrorCode::UsageLimitExceeded,
                            "A search match cannot fit the output budget. Narrow the search scope.",
                        ));
                    }
                    full = true;
                    continue;
                }
                used += size;
                page.matches.push(item);
                current = Some(page.matches.len() - 1);
            }
            _ => {}
        }
    }
    Ok(page)
}
