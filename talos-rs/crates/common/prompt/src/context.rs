use std::collections::HashMap;

/// Context builder for assembling prompt contexts.
pub struct PromptContext {
    pub variables: HashMap<String, String>,
}

impl PromptContext {
    /// Create a new context.
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    /// Add a variable to the context.
    pub fn add<V: ToString>(mut self, key: impl Into<String>, value: V) -> Self {
        self.variables.insert(key.into(), value.to_string());
        self
    }

    /// Extend context with multiple variables.
    pub fn extend(mut self, other: HashMap<String, String>) -> Self {
        self.variables.extend(other);
        self
    }

    /// Get a variable value.
    pub fn get(&self, key: &str) -> Option<&String> {
        self.variables.get(key)
    }

    /// Check if a variable exists.
    pub fn contains_key(&self, key: &str) -> bool {
        self.variables.contains_key(key)
    }
}

impl Default for PromptContext {
    fn default() -> Self {
        Self::new()
    }
}
