/// Stable diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OkfDiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// One OKF conformance or guidance finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfDiagnostic {
    pub severity: OkfDiagnosticSeverity,
    pub path: Option<String>,
    pub concept: Option<String>,
    pub message: String,
    pub fixable: bool,
}

/// Aggregate OKF validation result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OkfValidationReport {
    pub diagnostics: Vec<OkfDiagnostic>,
}

impl OkfValidationReport {
    /// Returns true when no conformance error exists.
    #[must_use]
    pub fn is_conformant(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|item| item.severity == OkfDiagnosticSeverity::Error)
    }

    pub(crate) fn push(
        &mut self,
        severity: OkfDiagnosticSeverity,
        path: Option<String>,
        concept: Option<String>,
        message: impl Into<String>,
    ) {
        self.push_fixable(severity, path, concept, message, false);
    }

    pub(crate) fn push_fixable(
        &mut self,
        severity: OkfDiagnosticSeverity,
        path: Option<String>,
        concept: Option<String>,
        message: impl Into<String>,
        fixable: bool,
    ) {
        self.diagnostics.push(OkfDiagnostic {
            severity,
            path,
            concept,
            message: message.into(),
            fixable,
        });
    }
}
