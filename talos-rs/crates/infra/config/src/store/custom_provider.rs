//! Custom connections share the existing env-backed credential lifecycle.
use super::*;
use crate::custom_provider_key_env;

pub(super) fn persist(
    values: &mut BTreeMap<String, String>,
    next: &mut LlmConfig,
    previous: &LlmConfig,
) -> TalosResult<()> {
    for id in previous.custom_providers.keys() {
        if !next.custom_providers.contains_key(id) {
            values.remove(&custom_provider_key_env(id));
        }
    }
    for (id, entry) in &mut next.custom_providers {
        let existing = previous.custom_providers.get(id).map(|entry| &entry.config);
        let env_key = custom_provider_key_env(id);
        let keep_placeholder = entry
            .config
            .api_key
            .as_deref()
            .is_some_and(|key| !key.is_empty())
            || (entry.config.api_key.is_none()
                && (existing.is_some_and(|entry| entry.api_key.is_some())
                    || values.contains_key(&env_key)));
        persist_llm_provider_secret(
            values,
            &env_key,
            keep_placeholder,
            &mut entry.config,
            existing,
        )?;
    }
    Ok(())
}

pub(super) fn sanitize(llm: &mut LlmConfig) {
    for entry in llm.custom_providers.values_mut() {
        sanitize_provider_for_read(&mut entry.config);
    }
}

pub(super) fn resolve(llm: &mut LlmConfig, env: &RuntimeEnv) {
    for (id, entry) in &mut llm.custom_providers {
        resolve_provider_api_key(&mut entry.config, &custom_provider_key_env(id), env);
    }
}

pub(super) fn statuses(llm: &LlmConfig, env: &RuntimeEnv) -> BTreeMap<String, ProviderKeyStatus> {
    llm.custom_providers
        .iter()
        .map(|(id, entry)| {
            (
                id.clone(),
                ProviderKeyStatus {
                    api_key_configured: provider_api_key_configured(
                        &entry.config,
                        &custom_provider_key_env(id),
                        env,
                    ),
                },
            )
        })
        .collect()
}

pub(super) fn display(llm: &mut LlmConfig, statuses: &BTreeMap<String, ProviderKeyStatus>) {
    for (id, entry) in &mut llm.custom_providers {
        apply_provider_api_key_display(&mut entry.config, statuses.get(id));
    }
}

pub(super) fn preserve(current: &LlmConfig, update: &mut LlmConfig) {
    for (id, entry) in &mut update.custom_providers {
        if let Some(previous) = current.custom_providers.get(id) {
            preserve_provider_runtime_secret(&previous.config, &mut entry.config);
        } else if entry.config.api_key.as_deref() == Some("") {
            entry.config.api_key = None;
        }
    }
}
