use serde::{Deserialize, Serialize};
use talos_config::AgentKernelConfig;
use talos_prompt::{PromptDiagnostics, PromptSectionKind, WorkspaceBootstrapFileKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum KernelWarning {
    PromptBootstrapFileMissing(PromptBootstrapFileMissingWarning),
    PromptBootstrapFileTruncated(PromptBootstrapFileTruncatedWarning),
    PromptBootstrapFileDropped(PromptBootstrapFileDroppedWarning),
    PromptDynamicSectionDropped(PromptDynamicSectionDroppedWarning),
    PromptDynamicSectionTruncated(PromptDynamicSectionTruncatedWarning),
    PromptDurableMemoryMissing(PromptDurableMemoryMissingWarning),
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
    pub dynamic_budget_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptDynamicSectionTruncatedWarning {
    pub sections: Vec<PromptSectionKind>,
    pub dynamic_budget_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptDurableMemoryMissingWarning {
    pub expected_file_name: String,
    pub workspace_dir: String,
}

pub(crate) fn derive_prompt_warnings(
    diagnostics: &PromptDiagnostics,
    kernel_config: &AgentKernelConfig,
) -> Vec<KernelWarning> {
    let mut warnings = Vec::new();

    for file in &diagnostics.bootstrap_files {
        if file.missing {
            warnings.push(KernelWarning::PromptBootstrapFileMissing(
                PromptBootstrapFileMissingWarning {
                    file_kind: file.kind,
                    path: file.path.display().to_string(),
                },
            ));
            continue;
        }

        if file.raw_chars == 0 && file.injected_chars == 0 {
            warnings.push(KernelWarning::PromptBootstrapFileDropped(
                PromptBootstrapFileDroppedWarning {
                    file_kind: file.kind,
                    path: file.path.display().to_string(),
                    raw_chars: file.raw_chars,
                    reason: PromptBootstrapDropReason::EmptyContent,
                },
            ));
            continue;
        }

        if file.truncated && file.injected_chars == 0 {
            warnings.push(KernelWarning::PromptBootstrapFileDropped(
                PromptBootstrapFileDroppedWarning {
                    file_kind: file.kind,
                    path: file.path.display().to_string(),
                    raw_chars: file.raw_chars,
                    reason: PromptBootstrapDropReason::BudgetExceeded,
                },
            ));
            continue;
        }

        if file.truncated {
            warnings.push(KernelWarning::PromptBootstrapFileTruncated(
                PromptBootstrapFileTruncatedWarning {
                    file_kind: file.kind,
                    path: file.path.display().to_string(),
                    raw_chars: file.raw_chars,
                    injected_chars: file.injected_chars,
                },
            ));
        }
    }

    if !diagnostics.dropped_sections.is_empty() {
        warnings.push(KernelWarning::PromptDynamicSectionDropped(
            PromptDynamicSectionDroppedWarning {
                sections: diagnostics.dropped_sections.clone(),
                dynamic_budget_tokens: kernel_config.prompt.max_dynamic_tokens,
            },
        ));
    }

    if !diagnostics.truncated_sections.is_empty() {
        warnings.push(KernelWarning::PromptDynamicSectionTruncated(
            PromptDynamicSectionTruncatedWarning {
                sections: diagnostics.truncated_sections.clone(),
                dynamic_budget_tokens: kernel_config.prompt.max_dynamic_tokens,
            },
        ));
    }

    if kernel_config.prompt.include_root_memory
        && !diagnostics
            .included_sections
            .contains(&PromptSectionKind::DurableMemory)
    {
        warnings.push(KernelWarning::PromptDurableMemoryMissing(
            PromptDurableMemoryMissingWarning {
                expected_file_name: "MEMORY.md".to_string(),
                workspace_dir: kernel_config.memory.workspace_dir.clone(),
            },
        ));
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use talos_prompt::{BootstrapFileDiagnostic, PromptMode};

    fn base_diagnostics() -> PromptDiagnostics {
        PromptDiagnostics {
            mode: PromptMode::Full,
            included_sections: vec![PromptSectionKind::Identity],
            dropped_sections: Vec::new(),
            truncated_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 10,
            static_estimated_tokens: 4,
            dynamic_estimated_tokens: 6,
        }
    }

    #[test]
    fn test_derive_prompt_warnings_covers_all_first_version_kinds() {
        let mut diagnostics = base_diagnostics();
        diagnostics.bootstrap_files = vec![
            BootstrapFileDiagnostic {
                kind: WorkspaceBootstrapFileKind::Agents,
                path: PathBuf::from("/tmp/AGENTS.md"),
                raw_chars: 100,
                injected_chars: 0,
                truncated: false,
                missing: true,
            },
            BootstrapFileDiagnostic {
                kind: WorkspaceBootstrapFileKind::Soul,
                path: PathBuf::from("/tmp/SOUL.md"),
                raw_chars: 100,
                injected_chars: 20,
                truncated: true,
                missing: false,
            },
            BootstrapFileDiagnostic {
                kind: WorkspaceBootstrapFileKind::Tools,
                path: PathBuf::from("/tmp/TOOLS.md"),
                raw_chars: 100,
                injected_chars: 0,
                truncated: true,
                missing: false,
            },
            BootstrapFileDiagnostic {
                kind: WorkspaceBootstrapFileKind::Identity,
                path: PathBuf::from("/tmp/IDENTITY.md"),
                raw_chars: 0,
                injected_chars: 0,
                truncated: false,
                missing: false,
            },
        ];
        diagnostics.dropped_sections = vec![PromptSectionKind::RecentMessages];
        diagnostics.truncated_sections = vec![PromptSectionKind::DurableMemory];

        let config = AgentKernelConfig::default();
        let warnings = derive_prompt_warnings(&diagnostics, &config);

        assert!(warnings.iter().any(|warning| matches!(
            warning,
            KernelWarning::PromptBootstrapFileMissing(_)
        )));
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            KernelWarning::PromptBootstrapFileTruncated(_)
        )));
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            KernelWarning::PromptBootstrapFileDropped(_)
        )));
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            KernelWarning::PromptDynamicSectionDropped(_)
        )));
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            KernelWarning::PromptDynamicSectionTruncated(_)
        )));
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            KernelWarning::PromptDurableMemoryMissing(_)
        )));
    }

    #[test]
    fn test_derive_prompt_warnings_skips_durable_memory_warning_when_present() {
        let mut diagnostics = base_diagnostics();
        diagnostics.included_sections.push(PromptSectionKind::DurableMemory);

        let config = AgentKernelConfig::default();
        let warnings = derive_prompt_warnings(&diagnostics, &config);

        assert!(!warnings.iter().any(|warning| matches!(
            warning,
            KernelWarning::PromptDurableMemoryMissing(_)
        )));
    }
}
