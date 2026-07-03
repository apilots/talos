use ratatui::text::Line;
use talos_error::TalosResult;
use talos_sdk::{
    ModelInfo, SessionHistorySnapshot, SessionInfo, SkillSuggestion, TalosContentItem,
    TimelineRecoveryResponse, ToolArguments,
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
                name: "history".to_string(),
                insert_text: "/history".to_string(),
                display_text: "/history".to_string(),
                description: "Open session history panel".to_string(),
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

// ── App state ────────────────────────────────────────────────────────────────

/// Application states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Chat,
    History,
}

// ── Chat types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChatRole {
    User,
    Thinking,
    Assistant,
    System,
    Tool,
}

impl ChatRole {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Thinking => "Thinking",
            Self::Assistant => "Assistant",
            Self::System => "System",
            Self::Tool => "Tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolCallStatus {
    Queued,
    Running,
    PendingDynamicInput,
    Completed { exit_ok: bool },
}

#[derive(Debug, Clone)]
pub(crate) struct ChatBlock {
    pub(crate) role: ChatRole,
    pub(crate) content: String,
    pub(crate) tool_call_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    pub(crate) tool_input: Option<ToolArguments>,
    pub(crate) tool_content_items: Vec<TalosContentItem>,
    pub(crate) tool_status: Option<ToolCallStatus>,
    pub(crate) tool_progress: Vec<String>,
    pub(crate) tool_iteration: Option<u64>,
    pub(crate) tool_item_index: Option<u64>,
    pub(crate) expanded: bool,
    pub(crate) timing_ms: Option<u64>,
    pub(crate) round_timing_ms: Option<u64>,
}

impl ChatBlock {
    pub(crate) fn new(role: ChatRole, content: String) -> Self {
        Self {
            role,
            content,
            tool_call_id: None,
            tool_name: None,
            tool_input: None,
            tool_content_items: Vec::new(),
            tool_status: None,
            tool_progress: Vec::new(),
            tool_iteration: None,
            tool_item_index: None,
            expanded: false,
            timing_ms: None,
            round_timing_ms: None,
        }
    }

    pub(crate) fn new_tool_call(
        call_id: String,
        tool_name: String,
        tool_input: impl Into<ToolArguments>,
        iteration: u64,
        item_index: u64,
        timing_ms: Option<u64>,
    ) -> Self {
        let tool_input = tool_input.into();
        Self {
            role: ChatRole::Tool,
            content: String::new(),
            tool_call_id: Some(call_id),
            tool_name: Some(tool_name),
            tool_input: Some(tool_input),
            tool_content_items: Vec::new(),
            tool_status: Some(ToolCallStatus::Queued),
            tool_progress: Vec::new(),
            tool_iteration: Some(iteration),
            tool_item_index: Some(item_index),
            expanded: false,
            timing_ms,
            round_timing_ms: None,
        }
    }
}

// ── Render types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct CopyProjection {
    pub(crate) text: String,
    pub(crate) display_start_col: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderedChatLine {
    pub(crate) display: Line<'static>,
    pub(crate) copy_projection: Option<CopyProjection>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RoundFooterButton {
    pub(crate) round_start_block_index: usize,
    pub(crate) start_col: usize,
    pub(crate) end_col: usize,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ToolRenderData {
    pub(crate) summary: String,
    pub(crate) detail_lines: Vec<String>,
    pub(crate) preview_lines: Vec<String>,
    pub(crate) output_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolPanelEvent {
    pub(crate) call_id: String,
    pub(crate) summary: String,
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

// ── Selection types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionPoint {
    pub(crate) line_index: usize,
    pub(crate) column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChatSelection {
    pub(crate) anchor: SelectionPoint,
    pub(crate) focus: SelectionPoint,
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

// ── Command update ───────────────────────────────────────────────────────────

pub(crate) enum CommandUpdate {
    ModelsLoaded(TalosResult<Vec<ModelInfo>>),
    StatusLoaded(TalosResult<Option<SessionInfo>>),
    SessionCompacted(TalosResult<SessionInfo>),
    SessionCleared(TalosResult<SessionInfo>),
    ModelSwitched {
        requested: String,
        result: TalosResult<SessionInfo>,
    },
    HistoryLoaded(TalosResult<Vec<SessionInfo>>),
    ModelPickerSwitched {
        requested: String,
        result: TalosResult<SessionInfo>,
    },
    SessionHistoryLoaded {
        session: SessionInfo,
        result: Box<TalosResult<SessionHistoryLoad>>,
    },
}
