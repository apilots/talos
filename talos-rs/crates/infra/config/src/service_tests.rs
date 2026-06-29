use super::*;
use crate::store::InMemoryLlmConfigStore;

fn make_service() -> LlmConfigManager {
    LlmConfigManager::new(Arc::new(InMemoryLlmConfigStore::new(LlmConfig::default())))
}

#[test]
fn test_get_delegates_to_store() {
    let service = make_service();
    let llm = service.get().unwrap();
    assert_eq!(llm.provider, LlmConfig::default().provider);
}

#[test]
fn test_update_saves_and_returns_config() {
    let service = make_service();
    let mut llm = service.get().unwrap();
    llm.model = "updated-model".to_string();
    let returned = service.update(llm.clone()).unwrap();
    assert_eq!(returned.model, "updated-model");
    assert_eq!(service.get().unwrap().model, "updated-model");
}
