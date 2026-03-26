use crate::{
    BootstrapFileDiagnostic, PromptDiagnostics, PromptMode, PromptSection, PromptSectionKind,
};

pub(crate) fn build_prompt_diagnostics(
    mode: PromptMode,
    static_sections: &[PromptSection],
    dynamic_sections: &[PromptSection],
    bootstrap_files: Vec<BootstrapFileDiagnostic>,
    dropped_sections: Vec<PromptSectionKind>,
    truncated_sections: Vec<PromptSectionKind>,
) -> PromptDiagnostics {
    let static_estimated_tokens = static_sections
        .iter()
        .map(|section| section.estimated_tokens + 4)
        .sum();
    let dynamic_estimated_tokens = dynamic_sections
        .iter()
        .map(|section| section.estimated_tokens + 4)
        .sum();

    PromptDiagnostics {
        mode,
        included_sections: static_sections
            .iter()
            .chain(dynamic_sections.iter())
            .map(|section| section.kind)
            .collect(),
        dropped_sections,
        truncated_sections,
        bootstrap_files,
        total_estimated_tokens: static_estimated_tokens + dynamic_estimated_tokens,
        static_estimated_tokens,
        dynamic_estimated_tokens,
    }
}
