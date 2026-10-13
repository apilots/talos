use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::{
    AgentMediaSource, AgentUserContent, DynamicToolResult, QuestionAnswer, SandboxApprovalDecision,
    UserActionResponse,
};

use super::invalid;

pub fn encode_user_content(value: AgentUserContent) -> pb::UserContent {
    use pb::user_content::Content;
    pb::UserContent {
        content: Some(match value {
            AgentUserContent::Text { text } => Content::Text(text),
            AgentUserContent::SkillSelection { name, path } => {
                Content::SkillSelection(pb::SkillSelection { name, path })
            }
            AgentUserContent::File { file_id } => Content::FileId(file_id),
            AgentUserContent::Audio { source, mime_type } => Content::Audio(pb::Media {
                source: Some(encode_media_source(source)),
                mime_type,
            }),
            AgentUserContent::Video { source, mime_type } => Content::Video(pb::Media {
                source: Some(encode_media_source(source)),
                mime_type,
            }),
            AgentUserContent::UserActionResponse {
                action_id,
                response,
            } => Content::UserActionResponse(pb::ActionResponse {
                action_id,
                response: Some(encode_action_response(response)),
            }),
        }),
    }
}

pub fn decode_user_content(value: pb::UserContent) -> TalosResult<AgentUserContent> {
    use pb::user_content::Content;
    match value.content.ok_or_else(invalid)? {
        Content::Text(text) => Ok(AgentUserContent::Text { text }),
        Content::SkillSelection(value) => Ok(AgentUserContent::SkillSelection {
            name: value.name,
            path: value.path,
        }),
        Content::FileId(file_id) => Ok(AgentUserContent::File { file_id }),
        Content::Audio(value) => {
            let source = decode_media_source(value.source)?;
            Ok(AgentUserContent::Audio {
                source,
                mime_type: value.mime_type,
            })
        }
        Content::Video(value) => {
            let source = decode_media_source(value.source)?;
            Ok(AgentUserContent::Video {
                source,
                mime_type: value.mime_type,
            })
        }
        Content::UserActionResponse(value) => Ok(AgentUserContent::UserActionResponse {
            action_id: value.action_id,
            response: decode_action_response(value.response)?,
        }),
    }
}

fn encode_media_source(value: AgentMediaSource) -> pb::MediaSource {
    pb::MediaSource {
        source: Some(match value {
            AgentMediaSource::Url { url } => pb::media_source::Source::Url(url),
            AgentMediaSource::InlineBase64 { data } => pb::media_source::Source::InlineBase64(data),
        }),
    }
}

fn decode_media_source(value: Option<pb::MediaSource>) -> TalosResult<AgentMediaSource> {
    match value.ok_or_else(invalid)?.source.ok_or_else(invalid)? {
        pb::media_source::Source::Url(url) => Ok(AgentMediaSource::Url { url }),
        pb::media_source::Source::InlineBase64(data) => Ok(AgentMediaSource::InlineBase64 { data }),
    }
}

fn encode_action_response(value: UserActionResponse) -> pb::UserActionResponse {
    pb::UserActionResponse {
        response: Some(match value {
            UserActionResponse::DynamicToolResult(value) => {
                pb::user_action_response::Response::DynamicToolResult(pb::DynamicToolResult {
                    output: value.output,
                    success: value.success,
                })
            }
            UserActionResponse::Question(value) => {
                pb::user_action_response::Response::Question(pb::QuestionAnswer {
                    answer: Some(match value {
                        QuestionAnswer::SelectedOption { option_id } => {
                            pb::question_answer::Answer::OptionId(option_id)
                        }
                        QuestionAnswer::FreeForm { text } => {
                            pb::question_answer::Answer::Text(text)
                        }
                    }),
                })
            }
            UserActionResponse::SandboxApproval(value) => {
                pb::user_action_response::Response::SandboxApproval(match value {
                    SandboxApprovalDecision::AllowOnce => pb::ApprovalDecision::AllowOnce as i32,
                    SandboxApprovalDecision::Deny => pb::ApprovalDecision::Deny as i32,
                    SandboxApprovalDecision::Cancel => pb::ApprovalDecision::Cancel as i32,
                })
            }
        }),
    }
}

fn decode_action_response(
    value: Option<pb::UserActionResponse>,
) -> TalosResult<UserActionResponse> {
    use pb::user_action_response::Response;
    match value.ok_or_else(invalid)?.response.ok_or_else(invalid)? {
        Response::DynamicToolResult(value) => {
            Ok(UserActionResponse::DynamicToolResult(DynamicToolResult {
                output: value.output,
                success: value.success,
            }))
        }
        Response::Question(value) => {
            let answer = match value.answer.ok_or_else(invalid)? {
                pb::question_answer::Answer::OptionId(option_id) => {
                    QuestionAnswer::SelectedOption { option_id }
                }
                pb::question_answer::Answer::Text(text) => QuestionAnswer::FreeForm { text },
            };
            Ok(UserActionResponse::Question(answer))
        }
        Response::SandboxApproval(value) => {
            let decision = match pb::ApprovalDecision::try_from(value) {
                Ok(pb::ApprovalDecision::AllowOnce) => SandboxApprovalDecision::AllowOnce,
                Ok(pb::ApprovalDecision::Deny) => SandboxApprovalDecision::Deny,
                Ok(pb::ApprovalDecision::Cancel) => SandboxApprovalDecision::Cancel,
                _ => return Err(invalid()),
            };
            Ok(UserActionResponse::SandboxApproval(decision))
        }
    }
}
