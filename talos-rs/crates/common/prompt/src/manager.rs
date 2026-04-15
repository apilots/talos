use std::collections::HashMap;
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};

use crate::template::PromptTemplate;

/// Manager for multiple prompt templates.
pub struct PromptManager {
    templates: HashMap<String, Arc<PromptTemplate>>,
}

impl PromptManager {
    /// Create a new prompt manager.
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// Register a new template.
    pub fn register_template(&mut self, name: impl Into<String>, template: PromptTemplate) {
        self.templates.insert(name.into(), Arc::new(template));
    }

    /// Get a template by name.
    pub fn get_template(&self, name: &str) -> Option<&Arc<PromptTemplate>> {
        self.templates.get(name)
    }

    /// Render a named template with context.
    pub fn render_template(
        &self,
        name: &str,
        context: &HashMap<String, String>,
    ) -> TalosResult<String> {
        match self.get_template(name) {
            Some(template) => template.render(context),
            None => Err(TalosError::ValidationError(format!(
                "Template '{}' not found",
                name
            ))),
        }
    }

    /// Check if a template exists.
    pub fn has_template(&self, name: &str) -> bool {
        self.templates.contains_key(name)
    }

    /// List all registered template names.
    pub fn list_templates(&self) -> Vec<String> {
        self.templates.keys().cloned().collect()
    }
}
