use super::*;
use crate::test_support::mock_model_info;

#[test]
fn selects_duplicate_model_id_by_provider_key() {
    let mut openai = mock_model_info("shared-model", 128_000);
    openai.provider = "openai".to_string();
    let mut ollama = mock_model_info("shared-model", 128_000);
    ollama.provider = "ollama".to_string();

    let selection = selection_for_key(&[openai, ollama], "ollama:shared-model", None)
        .expect("provider-qualified model should resolve");

    assert_eq!(selection.model_ref.provider, "ollama");
    assert_eq!(selection.model_ref.id, "shared-model");
}
