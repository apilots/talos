use ratatui::text::Line;
use talos_error::TalosResult;
use talos_sdk::{
    LlmReasoningEffort, ModelInfo, SessionHistorySnapshot, SessionInfo, SkillSuggestion,
    TimelineRecoveryResponse,
};

// ── Suggestions ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct CommandSuggestion {
    pub(crate) name: String,
    pub(crate) insert_text: String,
    pub(crate) display_text: String,
    pub(crate) description: String,
}

impl CommandSuggestion {
    pub(crate) fn all() -> Vec<Self> {
        vec![
            CommandSuggestion {
                name: "help".to_string(),
                insert_text: "/help".to_string(),
                display_text: "/help".to_string(),
                description: "Show help and available commands".to_string(),
            },
            CommandSuggestion {
                name: "model".to_string(),
                insert_text: "/model".to_string(),
                display_text: "/model [id]".to_string(),
                description: "Switch model (no args: show picker)".to_string(),
            },
            CommandSuggestion {
                name: "models".to_string(),
                insert_text: "/models".to_string(),
                display_text: "/models".to_string(),
                description: "List all available models".to_string(),
            },
            CommandSuggestion {
                name: "sessions".to_string(),
                insert_text: "/sessions".to_string(),
                display_text: "/sessions [1-9]".to_string(),
                description: "Quick session switch (supports digit shortcuts)".to_string(),
            },
            CommandSuggestion {
                name: "status".to_string(),
                insert_text: "/status".to_string(),
                display_text: "/status".to_string(),
                description: "Show connection status".to_string(),
            },
            CommandSuggestion {
                name: "compact".to_string(),
                insert_text: "/compact".to_string(),
                display_text: "/compact".to_string(),
                description: "Compact current session history".to_string(),
            },
            CommandSuggestion {
                name: "clear".to_string(),
                insert_text: "/clear".to_string(),
                display_text: "/clear".to_string(),
                description: "Clear current session context".to_string(),
            },
            CommandSuggestion {
                name: "quit".to_string(),
                insert_text: "/quit".to_string(),
                display_text: "/quit".to_string(),
                description: "Exit application".to_string(),
            },
        ]
    }

    pub(crate) fn filter(query: &str) -> Vec<CommandSuggestion> {
        let normalized = query.trim_start_matches('/');
        Self::all()
            .into_iter()
            .filter(|cmd| {
                cmd.name.starts_with(normalized)
                    || cmd
                        .insert_text
                        .trim_start_matches('/')
                        .starts_with(normalized)
                    || cmd
                        .display_text
                        .trim_start_matches('/')
                        .starts_with(normalized)
            })
            .collect()
    }
}

// ── Chat types ───────────────────────────────────────────────────────────────

// ── Render types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct RenderedChatLine {
    pub(crate) display: Line<'static>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChatRenderCacheKey {
    pub(crate) content_width: u16,
    pub(crate) viewport_height: u16,
    pub(crate) block_count: usize,
    pub(crate) blocks_fingerprint: u64,
    pub(crate) in_flight: bool,
    pub(crate) spinner_index: Option<usize>,
}

// ── Input types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PastedTextBlock {
    pub(crate) marker: String,
    pub(crate) text: String,
}

impl PastedTextBlock {
    pub(crate) fn new(marker: String, text: String) -> Self {
        Self { marker, text }
    }
}

// ── Queue types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct QueuedMessage {
    pub(crate) text: String,
    pub(crate) skills: Vec<SkillSuggestion>,
    pub(crate) dispatch_attempts: u8,
}

impl QueuedMessage {
    pub(crate) fn with_skills(text: String, skills: Vec<SkillSuggestion>) -> Self {
        Self {
            text,
            skills,
            dispatch_attempts: 0,
        }
    }
}

pub(crate) struct SessionHistoryLoad {
    pub(crate) history: SessionHistorySnapshot,
    pub(crate) live_recovery: Option<TalosResult<TimelineRecoveryResponse>>,
}

pub(crate) struct StatusLoad {
    pub(crate) session: Option<SessionInfo>,
    pub(crate) auth: talos_sdk::AuthSessionReadResponse,
    pub(crate) workspace: talos_sdk::WorkspaceInfoResponse,
}

// ── Command update ───────────────────────────────────────────────────────────

pub(crate) enum CommandUpdate {
    ModelsLoaded(TalosResult<Vec<ModelInfo>>),
    StatusLoaded(Box<TalosResult<StatusLoad>>),
    SessionCompacted(TalosResult<SessionInfo>),
    SessionCleared(TalosResult<SessionInfo>),
    ModelSwitched {
        requested: String,
        reasoning_effort: Option<LlmReasoningEffort>,
        result: TalosResult<SessionInfo>,
    },
    HistoryLoaded(TalosResult<Vec<SessionInfo>>),
    ModelPickerSwitched {
        requested: String,
        reasoning_effort: Option<LlmReasoningEffort>,
        result: TalosResult<SessionInfo>,
    },
    SessionHistoryLoaded {
        session: SessionInfo,
        result: Box<TalosResult<SessionHistoryLoad>>,
    },
}
