use crate::{
    budget, BootstrapFileDiagnostic, PromptDiagnostics, PromptMode, PromptSection,
    PromptSectionKind, PromptWindowUsage,
};

pub(crate) fn build_prompt_diagnostics(
    mode: PromptMode,
    static_sections: &[PromptSection],
    dynamic_sections: &[PromptSection],
    bootstrap_files: Vec<BootstrapFileDiagnostic>,
    dropped_sections: Vec<PromptSectionKind>,
    window_usage: PromptWindowUsage,
) -> PromptDiagnostics {
    let static_estimated_tokens = budget::estimate_sections_tokens(static_sections);
    let dynamic_estimated_tokens = budget::estimate_sections_tokens(dynamic_sections);

    PromptDiagnostics {
        mode,
        included_sections: static_sections
            .iter()
            .chain(dynamic_sections.iter())
            .map(|section| section.kind)
            .collect(),
        dropped_sections,
        bootstrap_files,
        total_estimated_tokens: static_estimated_tokens + dynamic_estimated_tokens,
        static_estimated_tokens,
        dynamic_estimated_tokens,
        window_usage,
        session_history_compaction: None,
    }
}
