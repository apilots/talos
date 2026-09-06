use std::collections::HashMap;
use talos_error::{TalosError, TalosResult};

/// A prompt template that can be rendered with context variables.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PromptTemplate {
    /// The raw template string with placeholders.
    pub template: String,
    /// Optional description of the template's purpose.
    pub description: String,
    /// List of required variables for this template.
    pub required_vars: Vec<String>,
}

impl PromptTemplate {
    /// Create a new prompt template.
    pub fn new(template: impl Into<String>, description: impl Into<String>) -> Self {
        let template_str = template.into();
        let required_vars = Self::extract_variables(&template_str);

        Self {
            template: template_str,
            description: description.into(),
            required_vars,
        }
    }

    /// Create a new prompt template with explicit required variables.
    pub fn new_with_vars(
        template: impl Into<String>,
        description: impl Into<String>,
        required_vars: Vec<String>,
    ) -> Self {
        Self {
            template: template.into(),
            description: description.into(),
            required_vars,
        }
    }

    /// Render the template with provided context.
    pub fn render(&self, context: &HashMap<String, String>) -> TalosResult<String> {
        let mut result = self.template.clone();

        for var in &self.required_vars {
            if !context.contains_key(var) {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    format!(
                        "Missing required variable '{}' for template: {}",
                        var, self.description
                    ),
                ));
            }
        }

        for (key, value) in context {
            let placeholder = format!("{{{{{}}}}}", key);
            result = result.replace(&placeholder, value);
        }

        Ok(result)
    }

    /// Extract variable names from template string.
    pub(crate) fn extract_variables(template: &str) -> Vec<String> {
        let mut vars = Vec::new();
        let chars: Vec<char> = template.chars().collect();

        let mut i = 0;
        while i < chars.len() {
            if i + 3 < chars.len() && chars[i] == '{' && chars[i + 1] == '{' && chars[i + 2] != '{'
            {
                let start = i + 2;
                let mut end = start;

                while end < chars.len() {
                    if end + 1 < chars.len() && chars[end] == '}' && chars[end + 1] == '}' {
                        break;
                    }
                    end += 1;
                }

                if end < chars.len() {
                    let var_name: String = chars[start..end].iter().collect();
                    let var_name = var_name.trim().to_string();

                    if !var_name.is_empty() && !vars.contains(&var_name) {
                        vars.push(var_name);
                    }

                    i = end + 2;
                    continue;
                }
            }

            i += 1;
        }

        vars
    }

    /// Render with default values for missing variables.
    pub fn render_with_defaults(
        &self,
        context: &HashMap<String, String>,
        defaults: &HashMap<String, String>,
    ) -> TalosResult<String> {
        let mut full_context = defaults.clone();
        full_context.extend(context.clone());

        self.render(&full_context)
    }
}
