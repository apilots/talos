//! Lossless projections of canonical memory DTOs; routing stays outside business params.
use super::environment_pb::invalid;
use crate::memory_pb::{decode_kind as decode_memory_kind, encode_kind as encode_memory_kind};
use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::*;
fn encode_memory_importance(value: MemoryImportance) -> i32 {
    match value {
        MemoryImportance::Low => pb::MemoryImportance::Low as i32,
        MemoryImportance::Normal => pb::MemoryImportance::Normal as i32,
        MemoryImportance::High => pb::MemoryImportance::High as i32,
    }
}
fn decode_memory_importance(value: i32) -> TalosResult<MemoryImportance> {
    Ok(match pb::MemoryImportance::try_from(value) {
        Ok(pb::MemoryImportance::Low) => MemoryImportance::Low,
        Ok(pb::MemoryImportance::Normal) => MemoryImportance::Normal,
        Ok(pb::MemoryImportance::High) => MemoryImportance::High,
        _ => return Err(invalid()),
    })
}
fn encode_memory_file_list_params(
    value: MemoryFileListRequest,
) -> TalosResult<pb::MemoryFileListParams> {
    Ok(pb::MemoryFileListParams {
        limit: value
            .limit
            .map(u64::try_from)
            .transpose()
            .map_err(|_| invalid())?,
        before_date: value.before_date,
        date_from: value.date_from,
        date_to: value.date_to,
    })
}
fn decode_memory_file_list_params(
    value: pb::MemoryFileListParams,
) -> TalosResult<MemoryFileListRequest> {
    Ok(MemoryFileListRequest {
        node_id: None,
        limit: value
            .limit
            .map(usize::try_from)
            .transpose()
            .map_err(|_| invalid())?,
        before_date: value.before_date,
        date_from: value.date_from,
        date_to: value.date_to,
    })
}
fn encode_memory_file_read_params(
    value: MemoryFileReadRequest,
) -> TalosResult<pb::MemoryFileReadParams> {
    Ok(pb::MemoryFileReadParams { date: value.date })
}
fn decode_memory_file_read_params(
    value: pb::MemoryFileReadParams,
) -> TalosResult<MemoryFileReadRequest> {
    Ok(MemoryFileReadRequest {
        node_id: None,
        date: value.date,
    })
}
fn encode_memory_file_write_params(
    value: MemoryFileWriteRequest,
) -> TalosResult<pb::MemoryFileWriteParams> {
    Ok(pb::MemoryFileWriteParams {
        date: value.date,
        content: value.content,
        expected_version: value.expected_version,
    })
}
fn decode_memory_file_write_params(
    value: pb::MemoryFileWriteParams,
) -> TalosResult<MemoryFileWriteRequest> {
    Ok(MemoryFileWriteRequest {
        node_id: None,
        date: value.date,
        content: value.content,
        expected_version: value.expected_version,
    })
}
fn encode_memory_search_params(value: MemorySearchRequest) -> TalosResult<pb::MemorySearchParams> {
    Ok(pb::MemorySearchParams {
        query: value.query,
        limit: value.limit,
        kinds: value.kinds.map(|values| pb::MemoryKindFilter {
            values: values.into_iter().map(encode_memory_kind).collect(),
        }),
        since_at: value.since_at,
        until_at: value.until_at,
    })
}
fn decode_memory_search_params(value: pb::MemorySearchParams) -> TalosResult<MemorySearchRequest> {
    Ok(MemorySearchRequest {
        node_id: None,
        query: value.query,
        limit: value.limit,
        kinds: value
            .kinds
            .map(|filter| {
                filter
                    .values
                    .into_iter()
                    .map(decode_memory_kind)
                    .collect::<TalosResult<_>>()
            })
            .transpose()?,
        since_at: value.since_at,
        until_at: value.until_at,
    })
}
fn encode_memory_get_params(value: MemoryGetRequest) -> TalosResult<pb::MemoryGetParams> {
    Ok(pb::MemoryGetParams {
        memory_id: value.memory_id,
    })
}
fn decode_memory_get_params(value: pb::MemoryGetParams) -> TalosResult<MemoryGetRequest> {
    Ok(MemoryGetRequest {
        node_id: None,
        memory_id: value.memory_id,
    })
}
fn encode_memory_file_summary(value: MemoryFileSummary) -> TalosResult<pb::MemoryFileSummary> {
    Ok(pb::MemoryFileSummary {
        date: value.date,
        section_count: u64::try_from(value.section_count).map_err(|_| invalid())?,
        updated_at: value.updated_at,
        version_token: value.version_token,
    })
}
fn decode_memory_file_summary(value: pb::MemoryFileSummary) -> TalosResult<MemoryFileSummary> {
    Ok(MemoryFileSummary {
        date: value.date,
        section_count: usize::try_from(value.section_count).map_err(|_| invalid())?,
        updated_at: value.updated_at,
        version_token: value.version_token,
    })
}
fn encode_memory_file(value: MemoryFileDetail) -> TalosResult<pb::MemoryFile> {
    Ok(pb::MemoryFile {
        date: value.date,
        section_count: u64::try_from(value.section_count).map_err(|_| invalid())?,
        content: value.content,
        updated_at: value.updated_at,
        version_token: value.version_token,
    })
}
fn decode_memory_file(value: pb::MemoryFile) -> TalosResult<MemoryFileDetail> {
    Ok(MemoryFileDetail {
        date: value.date,
        section_count: usize::try_from(value.section_count).map_err(|_| invalid())?,
        content: value.content,
        updated_at: value.updated_at,
        version_token: value.version_token,
    })
}
fn encode_memory_files(value: MemoryFileListResponse) -> TalosResult<pb::MemoryFiles> {
    Ok(pb::MemoryFiles {
        data: value
            .data
            .into_iter()
            .map(encode_memory_file_summary)
            .collect::<TalosResult<_>>()?,
    })
}
fn decode_memory_files(value: pb::MemoryFiles) -> TalosResult<MemoryFileListResponse> {
    Ok(MemoryFileListResponse {
        data: value
            .data
            .into_iter()
            .map(decode_memory_file_summary)
            .collect::<TalosResult<_>>()?,
    })
}
fn encode_memory_record_source(value: MemorySourceInfo) -> TalosResult<pb::MemoryRecordSource> {
    Ok(pb::MemoryRecordSource {
        date: value.date,
        session_id: value.session_id,
        from_seq: value.from_seq,
        until_seq: value.until_seq,
    })
}
fn decode_memory_record_source(value: pb::MemoryRecordSource) -> TalosResult<MemorySourceInfo> {
    Ok(MemorySourceInfo {
        date: value.date,
        session_id: value.session_id,
        from_seq: value.from_seq,
        until_seq: value.until_seq,
    })
}
fn encode_memory_record(value: MemoryInfo) -> TalosResult<pb::MemoryRecord> {
    Ok(pb::MemoryRecord {
        memory_id: value.memory_id,
        title: value.title,
        body: value.body,
        kind: encode_memory_kind(value.kind),
        importance: encode_memory_importance(value.importance),
        occurred_at: value.occurred_at,
        source: Some(encode_memory_record_source(value.source)?),
    })
}
fn decode_memory_record(value: pb::MemoryRecord) -> TalosResult<MemoryInfo> {
    Ok(MemoryInfo {
        memory_id: value.memory_id,
        title: value.title,
        body: value.body,
        kind: decode_memory_kind(value.kind)?,
        importance: decode_memory_importance(value.importance)?,
        occurred_at: value.occurred_at,
        source: decode_memory_record_source(value.source.ok_or_else(invalid)?)?,
    })
}
fn encode_memory_search_hit(value: MemorySearchHit) -> TalosResult<pb::MemorySearchHit> {
    Ok(pb::MemorySearchHit {
        memory_id: value.memory_id,
        title: value.title,
        preview: value.preview,
        kind: encode_memory_kind(value.kind),
        importance: encode_memory_importance(value.importance),
        occurred_at: value.occurred_at,
        rank: value.rank,
        source: Some(encode_memory_record_source(value.source)?),
    })
}
fn decode_memory_search_hit(value: pb::MemorySearchHit) -> TalosResult<MemorySearchHit> {
    Ok(MemorySearchHit {
        memory_id: value.memory_id,
        title: value.title,
        preview: value.preview,
        kind: decode_memory_kind(value.kind)?,
        importance: decode_memory_importance(value.importance)?,
        occurred_at: value.occurred_at,
        rank: value.rank,
        source: decode_memory_record_source(value.source.ok_or_else(invalid)?)?,
    })
}
fn encode_memory_search_hits(value: MemorySearchResponse) -> TalosResult<pb::MemorySearchHits> {
    Ok(pb::MemorySearchHits {
        data: value
            .data
            .into_iter()
            .map(encode_memory_search_hit)
            .collect::<TalosResult<_>>()?,
    })
}
fn decode_memory_search_hits(value: pb::MemorySearchHits) -> TalosResult<MemorySearchResponse> {
    Ok(MemorySearchResponse {
        data: value
            .data
            .into_iter()
            .map(decode_memory_search_hit)
            .collect::<TalosResult<_>>()?,
    })
}
pub fn encode_list_memory_files_params(
    value: MemoryFileListRequest,
) -> TalosResult<pb::MemoryFileListParams> {
    encode_memory_file_list_params(value)
}
pub fn decode_list_memory_files_params(
    value: Option<pb::MemoryFileListParams>,
) -> TalosResult<MemoryFileListRequest> {
    decode_memory_file_list_params(value.ok_or_else(invalid)?)
}
pub fn encode_list_memory_files_result(
    value: MemoryFileListResponse,
) -> TalosResult<pb::MemoryFiles> {
    encode_memory_files(value)
}
pub fn decode_list_memory_files_result(
    value: pb::MemoryFiles,
) -> TalosResult<MemoryFileListResponse> {
    decode_memory_files(value)
}
pub fn encode_read_memory_file_params(
    value: MemoryFileReadRequest,
) -> TalosResult<pb::MemoryFileReadParams> {
    encode_memory_file_read_params(value)
}
pub fn decode_read_memory_file_params(
    value: Option<pb::MemoryFileReadParams>,
) -> TalosResult<MemoryFileReadRequest> {
    decode_memory_file_read_params(value.ok_or_else(invalid)?)
}
pub fn encode_read_memory_file_result(
    value: MemoryFileReadResponse,
) -> TalosResult<pb::MemoryFile> {
    encode_memory_file(value.file)
}
pub fn decode_read_memory_file_result(
    value: pb::MemoryFile,
) -> TalosResult<MemoryFileReadResponse> {
    Ok(MemoryFileReadResponse {
        file: decode_memory_file(value)?,
    })
}
pub fn encode_write_memory_file_params(
    value: MemoryFileWriteRequest,
) -> TalosResult<pb::MemoryFileWriteParams> {
    encode_memory_file_write_params(value)
}
pub fn decode_write_memory_file_params(
    value: Option<pb::MemoryFileWriteParams>,
) -> TalosResult<MemoryFileWriteRequest> {
    decode_memory_file_write_params(value.ok_or_else(invalid)?)
}
pub fn encode_write_memory_file_result(
    value: MemoryFileWriteResponse,
) -> TalosResult<pb::MemoryFile> {
    encode_memory_file(value.file)
}
pub fn decode_write_memory_file_result(
    value: pb::MemoryFile,
) -> TalosResult<MemoryFileWriteResponse> {
    Ok(MemoryFileWriteResponse {
        file: decode_memory_file(value)?,
    })
}
pub fn encode_search_memory_params(
    value: MemorySearchRequest,
) -> TalosResult<pb::MemorySearchParams> {
    encode_memory_search_params(value)
}
pub fn decode_search_memory_params(
    value: Option<pb::MemorySearchParams>,
) -> TalosResult<MemorySearchRequest> {
    decode_memory_search_params(value.ok_or_else(invalid)?)
}
pub fn encode_search_memory_result(
    value: MemorySearchResponse,
) -> TalosResult<pb::MemorySearchHits> {
    encode_memory_search_hits(value)
}
pub fn decode_search_memory_result(
    value: pb::MemorySearchHits,
) -> TalosResult<MemorySearchResponse> {
    decode_memory_search_hits(value)
}
pub fn encode_get_memory_params(value: MemoryGetRequest) -> TalosResult<pb::MemoryGetParams> {
    encode_memory_get_params(value)
}
pub fn decode_get_memory_params(
    value: Option<pb::MemoryGetParams>,
) -> TalosResult<MemoryGetRequest> {
    decode_memory_get_params(value.ok_or_else(invalid)?)
}
pub fn encode_get_memory_result(value: MemoryGetResponse) -> TalosResult<pb::MemoryRecord> {
    encode_memory_record(value.memory)
}
pub fn decode_get_memory_result(value: pb::MemoryRecord) -> TalosResult<MemoryGetResponse> {
    Ok(MemoryGetResponse {
        memory: decode_memory_record(value)?,
    })
}
