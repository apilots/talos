//! Lossless projections of canonical skills DTOs; routing stays outside business params.
use super::environment_pb::{invalid, path};
use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::*;
fn encode_skill_scope(value: SkillScope) -> i32 {
    match value {
        SkillScope::Project => pb::SkillScope::Project as i32,
        SkillScope::Plugin => pb::SkillScope::Plugin as i32,
        SkillScope::External => pb::SkillScope::External as i32,
        SkillScope::System => pb::SkillScope::System as i32,
    }
}
fn decode_skill_scope(value: i32) -> TalosResult<SkillScope> {
    Ok(match pb::SkillScope::try_from(value) {
        Ok(pb::SkillScope::Project) => SkillScope::Project,
        Ok(pb::SkillScope::Plugin) => SkillScope::Plugin,
        Ok(pb::SkillScope::External) => SkillScope::External,
        Ok(pb::SkillScope::System) => SkillScope::System,
        _ => return Err(invalid()),
    })
}
fn encode_skill_load_error_kind(value: SkillLoadErrorKind) -> i32 {
    match value {
        SkillLoadErrorKind::SkillRootReadFailed => {
            pb::SkillLoadErrorKind::SkillRootReadFailed as i32
        }
        SkillLoadErrorKind::SkillDirectoryReadFailed => {
            pb::SkillLoadErrorKind::SkillDirectoryReadFailed as i32
        }
        SkillLoadErrorKind::MissingSkillFile => pb::SkillLoadErrorKind::MissingSkillFile as i32,
        SkillLoadErrorKind::SkillFileMetadataReadFailed => {
            pb::SkillLoadErrorKind::SkillFileMetadataReadFailed as i32
        }
        SkillLoadErrorKind::SkillFileTooLarge => pb::SkillLoadErrorKind::SkillFileTooLarge as i32,
        SkillLoadErrorKind::SkillFileReadFailed => {
            pb::SkillLoadErrorKind::SkillFileReadFailed as i32
        }
        SkillLoadErrorKind::InvalidFrontmatter => pb::SkillLoadErrorKind::InvalidFrontmatter as i32,
        SkillLoadErrorKind::MissingRequiredField => {
            pb::SkillLoadErrorKind::MissingRequiredField as i32
        }
    }
}
fn decode_skill_load_error_kind(value: i32) -> TalosResult<SkillLoadErrorKind> {
    Ok(match pb::SkillLoadErrorKind::try_from(value) {
        Ok(pb::SkillLoadErrorKind::SkillRootReadFailed) => SkillLoadErrorKind::SkillRootReadFailed,
        Ok(pb::SkillLoadErrorKind::SkillDirectoryReadFailed) => {
            SkillLoadErrorKind::SkillDirectoryReadFailed
        }
        Ok(pb::SkillLoadErrorKind::MissingSkillFile) => SkillLoadErrorKind::MissingSkillFile,
        Ok(pb::SkillLoadErrorKind::SkillFileMetadataReadFailed) => {
            SkillLoadErrorKind::SkillFileMetadataReadFailed
        }
        Ok(pb::SkillLoadErrorKind::SkillFileTooLarge) => SkillLoadErrorKind::SkillFileTooLarge,
        Ok(pb::SkillLoadErrorKind::SkillFileReadFailed) => SkillLoadErrorKind::SkillFileReadFailed,
        Ok(pb::SkillLoadErrorKind::InvalidFrontmatter) => SkillLoadErrorKind::InvalidFrontmatter,
        Ok(pb::SkillLoadErrorKind::MissingRequiredField) => {
            SkillLoadErrorKind::MissingRequiredField
        }
        _ => return Err(invalid()),
    })
}
fn encode_skill_warning_code(value: SkillWarningCode) -> i32 {
    match value {
        SkillWarningCode::SkillNotFound => pb::SkillWarningCode::SkillNotFound as i32,
        SkillWarningCode::AmbiguousSkillName => pb::SkillWarningCode::AmbiguousSkillName as i32,
        SkillWarningCode::SkillInjectionFailed => pb::SkillWarningCode::SkillInjectionFailed as i32,
    }
}
fn decode_skill_warning_code(value: i32) -> TalosResult<SkillWarningCode> {
    Ok(match pb::SkillWarningCode::try_from(value) {
        Ok(pb::SkillWarningCode::SkillNotFound) => SkillWarningCode::SkillNotFound,
        Ok(pb::SkillWarningCode::AmbiguousSkillName) => SkillWarningCode::AmbiguousSkillName,
        Ok(pb::SkillWarningCode::SkillInjectionFailed) => SkillWarningCode::SkillInjectionFailed,
        _ => return Err(invalid()),
    })
}
fn encode_skills_list_params(value: SkillsListRequest) -> TalosResult<pb::SkillsListParams> {
    Ok(pb::SkillsListParams {
        include_disabled: value.include_disabled,
    })
}
fn decode_skills_list_params(value: pb::SkillsListParams) -> TalosResult<SkillsListRequest> {
    Ok(SkillsListRequest {
        node_id: None,
        include_disabled: value.include_disabled,
    })
}
fn encode_skills_refresh_params(
    _value: SkillsRefreshRequest,
) -> TalosResult<pb::SkillsRefreshParams> {
    Ok(pb::SkillsRefreshParams {})
}
fn decode_skills_refresh_params(
    _value: pb::SkillsRefreshParams,
) -> TalosResult<SkillsRefreshRequest> {
    Ok(SkillsRefreshRequest { node_id: None })
}
fn encode_skills_suggest_params(
    value: SkillsSuggestRequest,
) -> TalosResult<pb::SkillsSuggestParams> {
    Ok(pb::SkillsSuggestParams {
        query: value.query,
        limit: value
            .limit
            .map(u64::try_from)
            .transpose()
            .map_err(|_| invalid())?,
    })
}
fn decode_skills_suggest_params(
    value: pb::SkillsSuggestParams,
) -> TalosResult<SkillsSuggestRequest> {
    Ok(SkillsSuggestRequest {
        node_id: None,
        query: value.query,
        limit: value
            .limit
            .map(usize::try_from)
            .transpose()
            .map_err(|_| invalid())?,
    })
}
fn encode_skills_read_params(value: SkillsReadRequest) -> TalosResult<pb::SkillsReadParams> {
    Ok(pb::SkillsReadParams {
        selector: Some(encode_skill_selector(value.selector)?),
    })
}
fn decode_skills_read_params(value: pb::SkillsReadParams) -> TalosResult<SkillsReadRequest> {
    Ok(SkillsReadRequest {
        node_id: None,
        selector: decode_skill_selector(value.selector.ok_or_else(invalid)?)?,
    })
}
fn encode_skills_resolve_params(
    value: SkillsResolveRequest,
) -> TalosResult<pb::SkillsResolveParams> {
    Ok(pb::SkillsResolveParams {
        text: value.text,
        selected: value
            .selected
            .into_iter()
            .map(encode_skill_selector)
            .collect::<TalosResult<_>>()?,
    })
}
fn decode_skills_resolve_params(
    value: pb::SkillsResolveParams,
) -> TalosResult<SkillsResolveRequest> {
    Ok(SkillsResolveRequest {
        node_id: None,
        text: value.text,
        selected: value
            .selected
            .into_iter()
            .map(decode_skill_selector)
            .collect::<TalosResult<_>>()?,
    })
}
fn encode_skill_selector(value: SkillSelector) -> TalosResult<pb::SkillSelector> {
    Ok(pb::SkillSelector {
        name: value.name,
        path: value.path.map(path).transpose()?,
    })
}
fn decode_skill_selector(value: pb::SkillSelector) -> TalosResult<SkillSelector> {
    Ok(SkillSelector {
        name: value.name,
        path: value.path.map(Into::into),
    })
}
fn encode_skill_metadata(value: SkillMetadata) -> TalosResult<pb::SkillMetadata> {
    Ok(pb::SkillMetadata {
        display_name: value.display_name,
        icon: value.icon,
        author: value.author,
        version: value.version,
        argument_hint: value.argument_hint,
    })
}
fn decode_skill_metadata(value: pb::SkillMetadata) -> TalosResult<SkillMetadata> {
    Ok(SkillMetadata {
        display_name: value.display_name,
        icon: value.icon,
        author: value.author,
        version: value.version,
        argument_hint: value.argument_hint,
    })
}
fn encode_skill_summary(value: SkillSummary) -> TalosResult<pb::SkillSummary> {
    Ok(pb::SkillSummary {
        name: value.name,
        description: value.description,
        short_description: value.short_description,
        root: path(value.root)?,
        path: path(value.path)?,
        scope: encode_skill_scope(value.scope),
        source_plugin: value.source_plugin,
        source_install: value.source_install,
        enabled: value.enabled,
        read_only: value.read_only,
        metadata: value.metadata.map(encode_skill_metadata).transpose()?,
    })
}
fn decode_skill_summary(value: pb::SkillSummary) -> TalosResult<SkillSummary> {
    Ok(SkillSummary {
        name: value.name,
        description: value.description,
        short_description: value.short_description,
        root: value.root.into(),
        path: value.path.into(),
        scope: decode_skill_scope(value.scope)?,
        source_plugin: value.source_plugin,
        source_install: value.source_install,
        enabled: value.enabled,
        read_only: value.read_only,
        metadata: value.metadata.map(decode_skill_metadata).transpose()?,
    })
}
fn encode_skill_suggestion(value: SkillSuggestion) -> TalosResult<pb::SkillSuggestion> {
    Ok(pb::SkillSuggestion {
        name: value.name,
        description: value.description,
        short_description: value.short_description,
        root: path(value.root)?,
        path: path(value.path)?,
        scope: encode_skill_scope(value.scope),
        source_plugin: value.source_plugin,
        source_install: value.source_install,
        metadata: value.metadata.map(encode_skill_metadata).transpose()?,
    })
}
fn decode_skill_suggestion(value: pb::SkillSuggestion) -> TalosResult<SkillSuggestion> {
    Ok(SkillSuggestion {
        name: value.name,
        description: value.description,
        short_description: value.short_description,
        root: value.root.into(),
        path: value.path.into(),
        scope: decode_skill_scope(value.scope)?,
        source_plugin: value.source_plugin,
        source_install: value.source_install,
        metadata: value.metadata.map(decode_skill_metadata).transpose()?,
    })
}
fn encode_skill_load_error(value: SkillLoadError) -> TalosResult<pb::SkillLoadError> {
    Ok(pb::SkillLoadError {
        kind: encode_skill_load_error_kind(value.kind),
        root: path(value.root)?,
        path: value.path.map(path).transpose()?,
        message: value.message,
    })
}
fn decode_skill_load_error(value: pb::SkillLoadError) -> TalosResult<SkillLoadError> {
    Ok(SkillLoadError {
        kind: decode_skill_load_error_kind(value.kind)?,
        root: value.root.into(),
        path: value.path.map(Into::into),
        message: value.message,
    })
}
fn encode_skill_warning(value: SkillWarning) -> TalosResult<pb::SkillWarning> {
    Ok(pb::SkillWarning {
        code: encode_skill_warning_code(value.code),
        message: value.message,
    })
}
fn decode_skill_warning(value: pb::SkillWarning) -> TalosResult<SkillWarning> {
    Ok(SkillWarning {
        code: decode_skill_warning_code(value.code)?,
        message: value.message,
    })
}
fn encode_skills(value: SkillsListResponse) -> TalosResult<pb::Skills> {
    Ok(pb::Skills {
        data: value
            .data
            .into_iter()
            .map(encode_skill_summary)
            .collect::<TalosResult<_>>()?,
        errors: value
            .errors
            .into_iter()
            .map(encode_skill_load_error)
            .collect::<TalosResult<_>>()?,
    })
}
fn decode_skills(value: pb::Skills) -> TalosResult<SkillsListResponse> {
    Ok(SkillsListResponse {
        data: value
            .data
            .into_iter()
            .map(decode_skill_summary)
            .collect::<TalosResult<_>>()?,
        errors: value
            .errors
            .into_iter()
            .map(decode_skill_load_error)
            .collect::<TalosResult<_>>()?,
    })
}
fn encode_skill_document(value: SkillDocument) -> TalosResult<pb::SkillDocument> {
    Ok(pb::SkillDocument {
        summary: Some(encode_skill_summary(value.summary)?),
        contents: value.contents,
    })
}
fn decode_skill_document(value: pb::SkillDocument) -> TalosResult<SkillDocument> {
    Ok(SkillDocument {
        summary: decode_skill_summary(value.summary.ok_or_else(invalid)?)?,
        contents: value.contents,
    })
}
fn encode_skill_suggestions(value: SkillsSuggestResponse) -> TalosResult<pb::SkillSuggestions> {
    Ok(pb::SkillSuggestions {
        data: value
            .data
            .into_iter()
            .map(encode_skill_suggestion)
            .collect::<TalosResult<_>>()?,
    })
}
fn decode_skill_suggestions(value: pb::SkillSuggestions) -> TalosResult<SkillsSuggestResponse> {
    Ok(SkillsSuggestResponse {
        data: value
            .data
            .into_iter()
            .map(decode_skill_suggestion)
            .collect::<TalosResult<_>>()?,
    })
}
fn encode_resolved_skills(value: SkillsResolveResponse) -> TalosResult<pb::ResolvedSkills> {
    Ok(pb::ResolvedSkills {
        data: value
            .data
            .into_iter()
            .map(encode_skill_summary)
            .collect::<TalosResult<_>>()?,
        warnings: value
            .warnings
            .into_iter()
            .map(encode_skill_warning)
            .collect::<TalosResult<_>>()?,
    })
}
fn decode_resolved_skills(value: pb::ResolvedSkills) -> TalosResult<SkillsResolveResponse> {
    Ok(SkillsResolveResponse {
        data: value
            .data
            .into_iter()
            .map(decode_skill_summary)
            .collect::<TalosResult<_>>()?,
        warnings: value
            .warnings
            .into_iter()
            .map(decode_skill_warning)
            .collect::<TalosResult<_>>()?,
    })
}
pub fn encode_list_skills_params(value: SkillsListRequest) -> TalosResult<pb::SkillsListParams> {
    encode_skills_list_params(value)
}
pub fn decode_list_skills_params(
    value: Option<pb::SkillsListParams>,
) -> TalosResult<SkillsListRequest> {
    decode_skills_list_params(value.ok_or_else(invalid)?)
}
pub fn encode_list_skills_result(value: SkillsListResponse) -> TalosResult<pb::Skills> {
    encode_skills(value)
}
pub fn decode_list_skills_result(value: pb::Skills) -> TalosResult<SkillsListResponse> {
    decode_skills(value)
}
pub fn encode_refresh_skills_params(
    value: SkillsRefreshRequest,
) -> TalosResult<pb::SkillsRefreshParams> {
    encode_skills_refresh_params(value)
}
pub fn decode_refresh_skills_params(
    value: Option<pb::SkillsRefreshParams>,
) -> TalosResult<SkillsRefreshRequest> {
    decode_skills_refresh_params(value.ok_or_else(invalid)?)
}
pub fn encode_refresh_skills_result(value: SkillsListResponse) -> TalosResult<pb::Skills> {
    encode_skills(value)
}
pub fn decode_refresh_skills_result(value: pb::Skills) -> TalosResult<SkillsListResponse> {
    decode_skills(value)
}
pub fn encode_suggest_skills_params(
    value: SkillsSuggestRequest,
) -> TalosResult<pb::SkillsSuggestParams> {
    encode_skills_suggest_params(value)
}
pub fn decode_suggest_skills_params(
    value: Option<pb::SkillsSuggestParams>,
) -> TalosResult<SkillsSuggestRequest> {
    decode_skills_suggest_params(value.ok_or_else(invalid)?)
}
pub fn encode_suggest_skills_result(
    value: SkillsSuggestResponse,
) -> TalosResult<pb::SkillSuggestions> {
    encode_skill_suggestions(value)
}
pub fn decode_suggest_skills_result(
    value: pb::SkillSuggestions,
) -> TalosResult<SkillsSuggestResponse> {
    decode_skill_suggestions(value)
}
pub fn encode_read_skill_params(value: SkillsReadRequest) -> TalosResult<pb::SkillsReadParams> {
    encode_skills_read_params(value)
}
pub fn decode_read_skill_params(
    value: Option<pb::SkillsReadParams>,
) -> TalosResult<SkillsReadRequest> {
    decode_skills_read_params(value.ok_or_else(invalid)?)
}
pub fn encode_read_skill_result(value: SkillsReadResponse) -> TalosResult<pb::SkillDocument> {
    encode_skill_document(value.document)
}
pub fn decode_read_skill_result(value: pb::SkillDocument) -> TalosResult<SkillsReadResponse> {
    Ok(SkillsReadResponse {
        document: decode_skill_document(value)?,
    })
}
pub fn encode_resolve_skills_params(
    value: SkillsResolveRequest,
) -> TalosResult<pb::SkillsResolveParams> {
    encode_skills_resolve_params(value)
}
pub fn decode_resolve_skills_params(
    value: Option<pb::SkillsResolveParams>,
) -> TalosResult<SkillsResolveRequest> {
    decode_skills_resolve_params(value.ok_or_else(invalid)?)
}
pub fn encode_resolve_skills_result(
    value: SkillsResolveResponse,
) -> TalosResult<pb::ResolvedSkills> {
    encode_resolved_skills(value)
}
pub fn decode_resolve_skills_result(
    value: pb::ResolvedSkills,
) -> TalosResult<SkillsResolveResponse> {
    decode_resolved_skills(value)
}
