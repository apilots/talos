use serde::Deserialize;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::{
    protocols::chat::{self, ChatCompletionPromptTokensDetails, ChatCompletionUsage},
    Usage,
};

/// Bailian reports explicit cache creation alongside standard Chat usage.
#[derive(Debug, Deserialize)]
pub(super) struct BailianUsage {
    #[serde(flatten)]
    standard: ChatCompletionUsage,
    prompt_tokens_details: Option<BailianPromptTokensDetails>,
    cache_creation: Option<BailianCacheCreation>,
}

#[derive(Debug, Deserialize)]
struct BailianPromptTokensDetails {
    #[serde(flatten)]
    standard: ChatCompletionPromptTokensDetails,
    cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct BailianCacheCreation {
    cache_creation_input_tokens: Option<u32>,
    ephemeral_5m_input_tokens: Option<u32>,
}

impl BailianUsage {
    pub(super) fn into_usage(self) -> TalosResult<Usage> {
        let mut standard = self.standard;
        let prompt_cache_creation = self.prompt_tokens_details.and_then(|details| {
            standard.prompt_tokens_details = Some(details.standard);
            details.cache_creation_input_tokens
        });
        let legacy_cache_creation = match self.cache_creation {
            Some(creation) => creation.tokens()?,
            None => None,
        };
        let mut usage = chat::usage(standard)?;
        usage.cache_creation_input_tokens = usage
            .cache_creation_input_tokens
            .or(prompt_cache_creation)
            .or(legacy_cache_creation);
        Ok(usage)
    }
}

impl BailianCacheCreation {
    fn tokens(self) -> TalosResult<Option<u32>> {
        match (self.cache_creation_input_tokens, self.ephemeral_5m_input_tokens) {
            (Some(total), Some(ephemeral)) if total != ephemeral => Err(TalosError::new(
                ErrorCode::InvalidRequest,
                format!(
                    "Bailian cache creation token fields disagree: cache_creation_input_tokens={total} ephemeral_5m_input_tokens={ephemeral}"
                ),
            )),
            (total, ephemeral) => Ok(total.or(ephemeral)),
        }
    }
}
