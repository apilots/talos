use talos_error::{TalosError, TalosResult};
use talos_types::{ModelInfo, ModelRef};

/// Resolves the configured Inner Model or the Agent default against the shared model catalog.
///
/// The explicit selection always wins. Provider and model identifiers are normalized only by
/// trimming whitespace; unknown pairs fail without falling back to another model.
pub fn resolve_inner_model_ref(
    configured_model: Option<&ModelRef>,
    agent_default_model: &ModelRef,
    model_catalog: &[ModelInfo],
) -> TalosResult<ModelRef> {
    let selected = configured_model.unwrap_or(agent_default_model);
    let resolved = ModelRef {
        provider: selected.provider.trim().to_string(),
        id: selected.id.trim().to_string(),
    };
    if model_catalog
        .iter()
        .any(|entry| entry.provider == resolved.provider && entry.model.id == resolved.id)
    {
        return Ok(resolved);
    }
    Err(TalosError::localized(
        talos_error::ErrorLocalization::ModelNotConfigured {
            provider: resolved.provider,
            model: resolved.id,
        },
    ))
}
