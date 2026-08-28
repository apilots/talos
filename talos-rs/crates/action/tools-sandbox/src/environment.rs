use std::collections::BTreeMap;

/// Removes inherited credential variables before launching an untrusted tool process.
///
/// Explicit per-tool configuration remains owned by the caller and must be applied only
/// after this filter has processed the parent process environment.
pub fn filter_inherited_tool_environment<I, K, V>(variables: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    variables
        .into_iter()
        .map(|(name, value)| (name.into(), value.into()))
        .filter(|(name, _)| !is_sensitive_environment_name(name))
        .collect()
}

fn is_sensitive_environment_name(name: &str) -> bool {
    let normalized = name.to_ascii_uppercase();
    let segments = normalized.split('_').collect::<Vec<_>>();
    segments
        .iter()
        .any(|segment| matches!(*segment, "TOKEN" | "SECRET"))
        || segments
            .windows(2)
            .any(|segments| segments == ["API", "KEY"])
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
