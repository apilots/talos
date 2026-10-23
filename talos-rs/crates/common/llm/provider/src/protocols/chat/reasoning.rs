use super::{
    ChatCompletionReasoningDetail, ChatCompletionResponseMessage, ChatReasoningContinuationFormat,
};
use crate::{ReasoningBlock, ReasoningContinuation, ReasoningOutput};
use talos_error::{ErrorCode, TalosError, TalosResult};

type ChatReasoningFields = (
    Option<String>,
    Option<String>,
    Option<Vec<ChatCompletionReasoningDetail>>,
);

pub(super) fn reasoning_fields(
    continuation: Option<ReasoningContinuation>,
    format: ChatReasoningContinuationFormat,
) -> TalosResult<ChatReasoningFields> {
    let Some(continuation) = continuation else {
        return Ok((None, None, None));
    };

    match (format, continuation) {
        (ChatReasoningContinuationFormat::Omit, _) => Ok((None, None, None)),
        (ChatReasoningContinuationFormat::None, _) => Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Chat protocol received reasoning continuation without a replay format".to_string(),
        )),
        (ChatReasoningContinuationFormat::Reasoning, continuation) => {
            let text = text_reasoning_continuation(continuation)?;
            Ok((Some(text), None, None))
        }
        (ChatReasoningContinuationFormat::ReasoningContent, continuation) => {
            let text = text_reasoning_continuation(continuation)?;
            Ok((None, Some(text), None))
        }
        (ChatReasoningContinuationFormat::ReasoningDetails, ReasoningContinuation::Text(text)) => {
            Ok((
                None,
                None,
                Some(vec![ChatCompletionReasoningDetail::Text {
                    text,
                    signature: None,
                    id: None,
                    format: None,
                    index: None,
                }]),
            ))
        }
        (
            ChatReasoningContinuationFormat::ReasoningDetails,
            ReasoningContinuation::Blocks(blocks),
        ) => Ok((
            None,
            None,
            Some(
                blocks
                    .into_iter()
                    .map(ChatCompletionReasoningDetail::from)
                    .collect(),
            ),
        )),
    }
}

fn text_reasoning_continuation(continuation: ReasoningContinuation) -> TalosResult<String> {
    match continuation {
        ReasoningContinuation::Text(text) => Ok(text),
        ReasoningContinuation::Blocks(blocks) => {
            let mut text = String::new();
            for block in blocks {
                match block {
                    ReasoningBlock::Text(block) => {
                        if block
                            .signature
                            .as_deref()
                            .is_some_and(|signature| !signature.is_empty())
                        {
                            return Err(TalosError::new(
                                ErrorCode::InvalidRequest,
                                "Chat text reasoning field cannot replay signed reasoning blocks",
                            ));
                        }
                        text.push_str(&block.text);
                    }
                    ReasoningBlock::Summary(_) | ReasoningBlock::Encrypted(_) => {
                        return Err(TalosError::new(
                            ErrorCode::InvalidRequest,
                            "Chat text reasoning field cannot replay structured reasoning blocks"
                                .to_string(),
                        ));
                    }
                }
            }
            Ok(text)
        }
    }
}

pub(super) fn response_reasoning(message: &ChatCompletionResponseMessage) -> ReasoningOutput {
    if let Some(details) = message
        .reasoning_details
        .as_ref()
        .filter(|details| !details.is_empty())
    {
        let blocks: Vec<_> = details
            .iter()
            .cloned()
            .map(ChatCompletionReasoningDetail::into_reasoning_block)
            .collect();
        let display_text = blocks
            .iter()
            .filter_map(|block| match block {
                ReasoningBlock::Text(block) => Some(block.text.clone()),
                ReasoningBlock::Summary(block) => Some(block.text.clone()),
                ReasoningBlock::Encrypted(_) => None,
            })
            .collect();
        return ReasoningOutput {
            display_text,
            continuation: Some(ReasoningContinuation::Blocks(blocks)),
        };
    }

    let text = message
        .reasoning_content
        .clone()
        .or_else(|| message.thinking.clone())
        .or_else(|| message.reasoning.clone())
        .filter(|text| !text.is_empty());
    match text {
        Some(text) => ReasoningOutput {
            display_text: vec![text.clone()],
            continuation: Some(ReasoningContinuation::Text(text)),
        },
        None => ReasoningOutput::default(),
    }
}
