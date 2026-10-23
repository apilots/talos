use talos_sdk::{ModelInfo, ModelSelection, ReasoningEffort};

pub(crate) fn model_key(model: &ModelInfo) -> String {
    format!("{}:{}", model.provider, model.model.id)
}

pub(crate) fn selection_for_key(
    models: &[ModelInfo],
    key: &str,
    reasoning_effort: Option<ReasoningEffort>,
) -> Option<ModelSelection> {
    let model = models.iter().find(|model| model_key(model) == key)?;
    let mut selection = ModelSelection::from_model(
        talos_sdk::ModelRef {
            provider: model.provider.clone(),
            id: model.model.id.clone(),
        },
        &model.model,
    );
    if model.selection_capabilities.reasoning != talos_sdk::ReasoningControl::None
        && reasoning_effort.is_some()
    {
        selection.reasoning_effort = reasoning_effort;
    }
    Some(selection)
}

#[cfg(test)]
#[path = "model_selection_tests.rs"]
mod tests;
