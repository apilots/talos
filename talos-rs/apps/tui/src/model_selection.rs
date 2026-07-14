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
    Some(ModelSelection {
        model_ref: talos_sdk::ModelRef {
            provider: model.provider.clone(),
            id: model.model.id.clone(),
        },
        reasoning_effort: model
            .model
            .reasoning
            .then_some(reasoning_effort.unwrap_or(model.model.reasoning_effort)),
    })
}

#[cfg(test)]
#[path = "model_selection_tests.rs"]
mod tests;
