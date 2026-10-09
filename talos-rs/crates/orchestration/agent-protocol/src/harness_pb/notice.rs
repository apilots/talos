use agent_pb as pb;
use talos_agent_api::{
    AgentSystemNotice, PromptWarning, PromptWarningFile, PromptWarningNotice, SystemNoticeContent,
};
use talos_error::TalosResult;
use talos_types::TalosTimelineNoticeLevel;

use super::invalid;
use crate::error_pb;

pub fn encode_system_notice(value: AgentSystemNotice) -> pb::SystemNotice {
    pb::SystemNotice {
        level: match value.level {
            TalosTimelineNoticeLevel::Info => pb::NoticeLevel::Info as i32,
            TalosTimelineNoticeLevel::Warning => pb::NoticeLevel::Warning as i32,
            TalosTimelineNoticeLevel::Error => pb::NoticeLevel::Error as i32,
        },
        content: Some(match value.content {
            SystemNoticeContent::Text(value) => pb::system_notice::Content::Text(value),
            SystemNoticeContent::Error(value) => {
                pb::system_notice::Content::Error(error_pb::encode_api_error(value))
            }
            SystemNoticeContent::PromptWarning(value) => {
                pb::system_notice::Content::PromptWarning(encode_prompt_warning(value))
            }
        }),
    }
}

pub fn decode_system_notice(value: pb::SystemNotice) -> TalosResult<AgentSystemNotice> {
    let level = match pb::NoticeLevel::try_from(value.level) {
        Ok(pb::NoticeLevel::Info) => TalosTimelineNoticeLevel::Info,
        Ok(pb::NoticeLevel::Warning) => TalosTimelineNoticeLevel::Warning,
        Ok(pb::NoticeLevel::Error) => TalosTimelineNoticeLevel::Error,
        _ => return Err(invalid()),
    };
    let content = match value.content.ok_or_else(invalid)? {
        pb::system_notice::Content::Text(value) => SystemNoticeContent::Text(value),
        pb::system_notice::Content::Error(value) => {
            SystemNoticeContent::Error(error_pb::decode_api_error(value)?)
        }
        pb::system_notice::Content::PromptWarning(value) => {
            SystemNoticeContent::PromptWarning(decode_prompt_warning(value)?)
        }
    };
    Ok(AgentSystemNotice { level, content })
}

fn encode_prompt_warning(value: PromptWarningNotice) -> pb::PromptWarningNotice {
    use pb::prompt_warning_notice::Warning;
    pb::PromptWarningNotice {
        message: value.message,
        warning: Some(match value.warning {
            PromptWarning::BootstrapFileMissing { file } => {
                Warning::BootstrapFileMissing(encode_prompt_file(file))
            }
            PromptWarning::BootstrapFileTruncated {
                file,
                injected_chars,
                raw_chars,
            } => Warning::BootstrapFileTruncated(pb::PromptTruncated {
                file: encode_prompt_file(file),
                injected_chars,
                raw_chars,
            }),
            PromptWarning::BootstrapFileDropped { file } => {
                Warning::BootstrapFileDropped(encode_prompt_file(file))
            }
            PromptWarning::DynamicSectionsDropped { count } => {
                Warning::DynamicSectionsDropped(count)
            }
        }),
    }
}

fn decode_prompt_warning(value: pb::PromptWarningNotice) -> TalosResult<PromptWarningNotice> {
    use pb::prompt_warning_notice::Warning;
    let warning = match value.warning.ok_or_else(invalid)? {
        Warning::BootstrapFileMissing(value) => PromptWarning::BootstrapFileMissing {
            file: decode_prompt_file(value)?,
        },
        Warning::BootstrapFileTruncated(value) => PromptWarning::BootstrapFileTruncated {
            file: decode_prompt_file(value.file)?,
            injected_chars: value.injected_chars,
            raw_chars: value.raw_chars,
        },
        Warning::BootstrapFileDropped(value) => PromptWarning::BootstrapFileDropped {
            file: decode_prompt_file(value)?,
        },
        Warning::DynamicSectionsDropped(count) => PromptWarning::DynamicSectionsDropped { count },
    };
    Ok(PromptWarningNotice {
        message: value.message,
        warning,
    })
}

fn encode_prompt_file(value: PromptWarningFile) -> i32 {
    match value {
        PromptWarningFile::Agents => pb::PromptFile::Agents as i32,
        PromptWarningFile::Soul => pb::PromptFile::Soul as i32,
        PromptWarningFile::Identity => pb::PromptFile::Identity as i32,
        PromptWarningFile::User => pb::PromptFile::User as i32,
        PromptWarningFile::Heartbeat => pb::PromptFile::Heartbeat as i32,
    }
}

fn decode_prompt_file(value: i32) -> TalosResult<PromptWarningFile> {
    match pb::PromptFile::try_from(value) {
        Ok(pb::PromptFile::Agents) => Ok(PromptWarningFile::Agents),
        Ok(pb::PromptFile::Soul) => Ok(PromptWarningFile::Soul),
        Ok(pb::PromptFile::Identity) => Ok(PromptWarningFile::Identity),
        Ok(pb::PromptFile::User) => Ok(PromptWarningFile::User),
        Ok(pb::PromptFile::Heartbeat) => Ok(PromptWarningFile::Heartbeat),
        _ => Err(invalid()),
    }
}
