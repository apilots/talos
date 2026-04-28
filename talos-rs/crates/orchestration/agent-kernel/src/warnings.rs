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
mod tests {
    use super::*;
    use std::path::PathBuf;
    use talos_prompt::{BootstrapFileDiagnostic, PromptMode};

    fn base_diagnostics() -> PromptDiagnostics {
        PromptDiagnostics {
            mode: PromptMode::Full,
            included_sections: vec![PromptSectionKind::IdentityCore],
            dropped_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 10,
            static_estimated_tokens: 4,
            dynamic_estimated_tokens: 6,
            window_usage: talos_prompt::PromptWindowUsage::default(),
            session_history_compaction: None,
        }
    }

    #[test]
    fn test_derive_prompt_warnings_covers_all_active_kinds() {
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
                kind: WorkspaceBootstrapFileKind::Identity,
                path: PathBuf::from("/tmp/IDENTITY.md"),
                raw_chars: 0,
                injected_chars: 0,
                truncated: false,
                missing: false,
            },
        ];
        diagnostics.dropped_sections = vec![PromptSectionKind::RecentMessages];

        let config = AgentKernelConfig::default();
        let warnings = derive_prompt_warnings(&diagnostics, &config);

        assert!(warnings
            .iter()
            .any(|warning| matches!(warning, KernelWarning::PromptBootstrapFileMissing(_))));
        assert!(warnings
            .iter()
            .any(|warning| matches!(warning, KernelWarning::PromptBootstrapFileTruncated(_))));
        assert!(warnings
            .iter()
            .any(|warning| matches!(warning, KernelWarning::PromptBootstrapFileDropped(_))));
        assert!(warnings
            .iter()
            .any(|warning| matches!(warning, KernelWarning::PromptDynamicSectionDropped(_))));
    }

    #[test]
    fn test_derive_prompt_warnings_respects_bootstrap_warning_mode_off() {
        let mut diagnostics = base_diagnostics();
        diagnostics.bootstrap_files = vec![BootstrapFileDiagnostic {
            kind: WorkspaceBootstrapFileKind::Agents,
            path: PathBuf::from("/tmp/AGENTS.md"),
            raw_chars: 100,
            injected_chars: 20,
            truncated: true,
            missing: false,
        }];

        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.warning_mode = BootstrapPromptWarningMode::Off;
        let warnings = derive_prompt_warnings(&diagnostics, &config);

        assert!(warnings.is_empty());
    }

    #[test]
    fn test_derive_prompt_warnings_respects_bootstrap_warning_mode_once() {
        let mut diagnostics = base_diagnostics();
        diagnostics.bootstrap_files = vec![
            BootstrapFileDiagnostic {
                kind: WorkspaceBootstrapFileKind::Agents,
                path: PathBuf::from("/tmp/AGENTS.md"),
                raw_chars: 100,
                injected_chars: 10,
                truncated: true,
                missing: false,
            },
            BootstrapFileDiagnostic {
                kind: WorkspaceBootstrapFileKind::Soul,
                path: PathBuf::from("/tmp/SOUL.md"),
                raw_chars: 120,
                injected_chars: 20,
                truncated: true,
                missing: false,
            },
        ];

        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.warning_mode = BootstrapPromptWarningMode::Once;
        let warnings = derive_prompt_warnings(&diagnostics, &config);

        assert_eq!(
            warnings
                .iter()
                .filter(|warning| matches!(warning, KernelWarning::PromptBootstrapFileTruncated(_)))
                .count(),
            1
        );
    }

    #[test]
    fn test_derive_prompt_warnings_respects_bootstrap_warning_mode_always() {
        let mut diagnostics = base_diagnostics();
        diagnostics.bootstrap_files = vec![
            BootstrapFileDiagnostic {
                kind: WorkspaceBootstrapFileKind::Agents,
                path: PathBuf::from("/tmp/AGENTS.md"),
                raw_chars: 100,
                injected_chars: 10,
                truncated: true,
                missing: false,
            },
            BootstrapFileDiagnostic {
                kind: WorkspaceBootstrapFileKind::Soul,
                path: PathBuf::from("/tmp/SOUL.md"),
                raw_chars: 120,
                injected_chars: 20,
                truncated: true,
                missing: false,
            },
        ];

        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.warning_mode = BootstrapPromptWarningMode::Always;
        let warnings = derive_prompt_warnings(&diagnostics, &config);

        assert_eq!(
            warnings
                .iter()
                .filter(|warning| matches!(warning, KernelWarning::PromptBootstrapFileTruncated(_)))
                .count(),
            2
        );
    }
}
