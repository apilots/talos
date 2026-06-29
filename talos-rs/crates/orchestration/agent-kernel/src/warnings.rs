use serde::{Deserialize, Serialize};
use talos_config::{AgentKernelConfig, BootstrapPromptWarningMode};
use talos_prompt::{PromptDiagnostics, PromptSectionKind, WorkspaceBootstrapFileKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum KernelWarning {
    PromptBootstrapFileMissing(PromptBootstrapFileMissingWarning),
    PromptBootstrapFileTruncated(PromptBootstrapFileTruncatedWarning),
    PromptBootstrapFileDropped(PromptBootstrapFileDroppedWarning),
    PromptDynamicSectionDropped(PromptDynamicSectionDroppedWarning),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptBootstrapDropReason {
    BudgetExceeded,
    EmptyContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptBootstrapFileMissingWarning {
    pub file_kind: WorkspaceBootstrapFileKind,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptBootstrapFileTruncatedWarning {
    pub file_kind: WorkspaceBootstrapFileKind,
    pub path: String,
    pub raw_chars: usize,
    pub injected_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptBootstrapFileDroppedWarning {
    pub file_kind: WorkspaceBootstrapFileKind,
    pub path: String,
    pub raw_chars: usize,
    pub reason: PromptBootstrapDropReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptDynamicSectionDroppedWarning {
    pub sections: Vec<PromptSectionKind>,
}

pub(crate) fn derive_prompt_warnings(
    diagnostics: &PromptDiagnostics,
    kernel_config: &AgentKernelConfig,
) -> Vec<KernelWarning> {
    let mut warnings = Vec::new();
    let mut emitted_missing = false;
    let mut emitted_truncated = false;
    let mut emitted_dropped = false;

    if kernel_config.prompt.bootstrap.warning_mode != BootstrapPromptWarningMode::Off {
        for file in &diagnostics.bootstrap_files {
            if file.missing {
                if kernel_config.prompt.bootstrap.warning_mode == BootstrapPromptWarningMode::Always
                    || !emitted_missing
                {
                    warnings.push(KernelWarning::PromptBootstrapFileMissing(
                        PromptBootstrapFileMissingWarning {
                            file_kind: file.kind,
                            path: file.path.display().to_string(),
                        },
                    ));
                    emitted_missing = true;
                }
                continue;
            }

            if file.raw_chars == 0 && file.injected_chars == 0 {
                if kernel_config.prompt.bootstrap.warning_mode == BootstrapPromptWarningMode::Always
                    || !emitted_dropped
                {
                    warnings.push(KernelWarning::PromptBootstrapFileDropped(
                        PromptBootstrapFileDroppedWarning {
                            file_kind: file.kind,
                            path: file.path.display().to_string(),
                            raw_chars: file.raw_chars,
                            reason: PromptBootstrapDropReason::EmptyContent,
                        },
                    ));
                    emitted_dropped = true;
                }
                continue;
            }

            if file.truncated && file.injected_chars == 0 {
                if kernel_config.prompt.bootstrap.warning_mode == BootstrapPromptWarningMode::Always
                    || !emitted_dropped
                {
                    warnings.push(KernelWarning::PromptBootstrapFileDropped(
                        PromptBootstrapFileDroppedWarning {
                            file_kind: file.kind,
                            path: file.path.display().to_string(),
                            raw_chars: file.raw_chars,
                            reason: PromptBootstrapDropReason::BudgetExceeded,
                        },
                    ));
                    emitted_dropped = true;
                }
                continue;
            }

            if file.truncated
                && (kernel_config.prompt.bootstrap.warning_mode
                    == BootstrapPromptWarningMode::Always
                    || !emitted_truncated)
            {
                warnings.push(KernelWarning::PromptBootstrapFileTruncated(
                    PromptBootstrapFileTruncatedWarning {
                        file_kind: file.kind,
                        path: file.path.display().to_string(),
                        raw_chars: file.raw_chars,
                        injected_chars: file.injected_chars,
                    },
                ));
                emitted_truncated = true;
            }
        }
    }

    if !diagnostics.dropped_sections.is_empty() {
        warnings.push(KernelWarning::PromptDynamicSectionDropped(
            PromptDynamicSectionDroppedWarning {
                sections: diagnostics.dropped_sections.clone(),
            },
        ));
    }

    warnings
}

#[cfg(test)]
#[path = "warnings_tests.rs"]
mod tests;
