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
