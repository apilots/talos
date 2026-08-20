use ratatui::text::Line;
use talos_error::TalosResult;
use talos_sdk::{SessionHistorySnapshot, SessionInfo, TimelineRecoveryResponse};

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
                display_text: "/model [provider:id]".to_string(),
                description: "Switch model (no args: show picker)".to_string(),
            },
            CommandSuggestion {
                name: "permissions".to_string(),
                insert_text: "/permissions".to_string(),
                display_text: "/permissions".to_string(),
                description: "Choose what Talos is allowed to do".to_string(),
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PastedTextBlock {
    pub(crate) marker: String,
    pub(crate) text: String,
}

impl PastedTextBlock {
    pub(crate) fn new(marker: String, text: String) -> Self {
        Self { marker, text }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PastedImageAttachment {
    pub(crate) marker: String,
    pub(crate) filename: String,
    pub(crate) state: PastedImageState,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum PastedImageState {
    Pending { png_bytes: bytes::Bytes },
    Uploaded { file_id: String },
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

pub(crate) struct SessionCloseUpdate {
    pub(crate) result: TalosResult<String>,
    pub(crate) client_shutdown_completed: bool,
}

pub(crate) struct SessionUsageRefresh {
    pub(crate) session_id: String,
    pub(crate) result: TalosResult<SessionInfo>,
}

// ── Command update ───────────────────────────────────────────────────────────

pub(crate) enum CommandUpdate {
    ModelsLoaded(TalosResult<talos_sdk::ModelListResponse>),
    StatusLoaded(Box<TalosResult<StatusLoad>>),
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
    PermissionModeSwitched(TalosResult<SessionInfo>),
    UserActionSubmitted {
        action_id: String,
        result: TalosResult<talos_sdk::SubmitUserActionResponse>,
    },
    SessionHistoryLoaded {
        session: SessionInfo,
        result: Box<TalosResult<SessionHistoryLoad>>,
    },
}
