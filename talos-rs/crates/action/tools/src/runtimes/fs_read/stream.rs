//! Large-file reads never materialize the complete file or hash partial content
//! into the edit freshness state. Byte cursors refer to raw file positions.
use std::path::Path;

use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader, SeekFrom};

use super::{looks_like_text, ReadFileRequest, ReadFileResult, MAX_LIMIT};
use crate::{runtimes::map_sandbox_error, ToolContext};

const MAX_LINE_SCAN_BYTES: u64 = 64 * 1024 * 1024;

fn io_error(error: std::io::Error) -> TalosError {
    TalosError::internal_with_source(
        "Read the requested file range; technical details were withheld.",
        error,
    )
}

fn decode(bytes: &[u8], partial: bool) -> TalosResult<(&str, usize)> {
    if !looks_like_text(bytes) {
        return Err(TalosError::new(
            ErrorCode::UnsupportedMediaType,
            "The requested range is not supported text.",
        ));
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok((text, bytes.len())),
        Err(error) if partial && error.error_len().is_none() => {
            let end = error.valid_up_to();
            Ok((
                std::str::from_utf8(&bytes[..end]).map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::UnsupportedMediaType,
                        "The requested range is not valid UTF-8 text.",
                        error,
                    )
                })?,
                end,
            ))
        }
        Err(error) => Err(TalosError::from_source(
            ErrorCode::UnsupportedMediaType,
            "The requested range is not valid UTF-8 text.",
            error,
        )),
    }
}

pub(super) async fn run(
    request: &ReadFileRequest,
    path: &Path,
    context: &ToolContext,
) -> TalosResult<ReadFileResult> {
    let file_system = context.sandbox_file_system()?;
    let metadata = file_system
        .metadata(path)
        .await
        .map_err(map_sandbox_error)?;
    if !metadata.is_file {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Read requires a regular file.",
        ));
    }
    let mut file = file_system
        .open_read(path)
        .await
        .map_err(map_sandbox_error)?;
    // Worst-case JSON escaping is six bytes per raw text byte. Budget it here,
    // before advancing a cursor, so a later projection cannot skip unseen bytes.
    let path_bytes =
        serde_json::to_vec(&path.to_string_lossy()).map_or(usize::MAX, |bytes| bytes.len());
    let budget = (context
        .kernel_config()
        .map(|config| config.tool_result_max_bytes)
        .unwrap_or(10_000)
        .min(crate::output_budget::MAX_MODEL_TOOL_BYTES)
        .saturating_sub(path_bytes.saturating_add(1024))
        / 6)
    .min(request.max_bytes.unwrap_or(8192));
    if budget < 4 {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "A file range requires at least four bytes of output budget.",
        ));
    }
    if request.byte_offset.is_some() || request.max_bytes.is_some() {
        let start = request.byte_offset.unwrap_or(0);
        if start > metadata.len {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "The byte offset exceeds the file length.",
            ));
        }
        file.seek(SeekFrom::Start(start)).await.map_err(io_error)?;
        let wanted = (metadata.len - start).min(budget as u64) as usize;
        let mut bytes = vec![0; wanted];
        let mut length = 0;
        while length < wanted {
            let read = file.read(&mut bytes[length..]).await.map_err(io_error)?;
            if read == 0 {
                break;
            }
            length += read;
        }
        bytes.truncate(length);
        // An arbitrary byte offset may point into a multi-byte character.
        let skipped = bytes
            .iter()
            .take_while(|byte| **byte & 0xc0 == 0x80)
            .count();
        let at_end = length < wanted || start + (length as u64) >= metadata.len;
        let (text, valid) = decode(&bytes[skipped..], !at_end)?;
        let next = start + (skipped + valid) as u64;
        return Ok(ReadFileResult {
            path: path.display().to_string(),
            offset: 0,
            limit: 0,
            total_lines: None,
            next_byte_offset: (!at_end).then_some(next),
            content: text.to_owned(),
            truncated: !at_end,
            unchanged: false,
        });
    }

    let mut reader = BufReader::with_capacity(4096, file.take(metadata.len));
    let mut position = 0_u64;
    let mut line = 1_usize;
    let last_line = request.offset.saturating_add(request.limit.min(MAX_LIMIT));
    let mut selected = Vec::with_capacity(budget);
    let mut last_was_newline = false;
    let mut reached_eof = false;
    loop {
        let buffer = reader.fill_buf().await.map_err(io_error)?;
        if buffer.is_empty() {
            reached_eof = true;
            break;
        }
        if line >= last_line || selected.len() >= budget {
            break;
        }
        let mut consumed = 0;
        for byte in buffer {
            if line >= last_line || selected.len() >= budget {
                break;
            }
            if position >= MAX_LINE_SCAN_BYTES {
                return Err(TalosError::new(
                    ErrorCode::UsageLimitExceeded,
                    "Line scan limit reached. Use Read with byte_offset and max_bytes.",
                ));
            }
            if line >= request.offset {
                selected.push(*byte);
            }
            last_was_newline = *byte == b'\n';
            if last_was_newline {
                line += 1;
            }
            position += 1;
            consumed += 1;
        }
        reader.consume(consumed);
    }
    let eof = reached_eof || position >= metadata.len;
    let total = if metadata.len == 0 {
        0
    } else {
        line - usize::from(last_was_newline)
    };
    if eof && request.offset > total && metadata.len > 0 {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "The requested offset exceeds the file length.",
        ));
    }
    let (text, valid) = decode(&selected, !eof)?;
    let next = position - (selected.len() - valid) as u64;
    Ok(ReadFileResult {
        path: path.display().to_string(),
        offset: request.offset.saturating_sub(1),
        limit: request.limit.min(MAX_LIMIT),
        total_lines: eof.then_some(total),
        next_byte_offset: (!eof).then_some(next),
        content: text.trim_end_matches('\n').to_owned(),
        truncated: !eof,
        unchanged: false,
    })
}
