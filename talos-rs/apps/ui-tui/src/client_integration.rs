//! Talos Terminal User Interface Components
//!
//! This module provides a bridge between the TUI and the new TalosClient API,
//! allowing the TUI to leverage chat, session and model management features.

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};

use talos_client::{
    SessionCreateParams, TalosClient, TalosInputItem, TalosSession,
    TalosMessageRequest, TalosMessageResponse, TalosOutputItem,
};

/// TUI State Management - handles UI state and client integration
pub struct TuiAppState {
    /// Shared client instance
    client: Arc<TalosClient>,
    
    /// Current active session (if any)
    active_session: RwLock<Option<TalosSession>>,
    
    /// Available models cache
    available_models: RwLock<Vec<String>>,
    
    /// Available sessions list cache
    available_sessions: RwLock<Vec<TalosSession>>,
    
    /// Currently selected model
    current_model: RwLock<String>,
    
    /// Receiver for async updates
    update_receiver: RwLock<Option<mpsc::UnboundedReceiver<TuiUpdate>>>,
    
    /// Sender for async updates
    update_sender: Option<mpsc::UnboundedSender<TuiUpdate>>,
}

/// Types of updates that can be sent to the TUI
#[derive(Debug)]
pub enum TuiUpdate {
    /// Chat response received
    ChatResponse(String),
    /// Session created
    SessionCreated(TalosSession),
    /// Models loaded
    ModelsLoaded(Vec<String>),
    /// Sessions list updated
    SessionsList(Vec<TalosSession>),
    /// Error occurred
    Error(String),
}

impl TuiAppState {
    /// Create a new TUI app state with the provided client
    pub fn new(client: Arc<TalosClient>) -> Arc<Self> {
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        
        Arc::new(Self {
            client,
            active_session: RwLock::new(None),
            available_models: RwLock::new(Vec::new()),
            available_sessions: RwLock::new(Vec::new()),
            current_model: RwLock::new("qwen-plus".to_string()),
            update_receiver: RwLock::new(Some(update_rx)),
            update_sender: Some(update_tx),
        })
    }
    
    /// Perform initial load of models and sessions
    pub async fn initialize(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Load models asynchronously
        {
            let client = self.client.clone();
            let sender = self.update_sender.as_ref().unwrap().clone();
            tokio::spawn(async move {
                match client.list_models().await {
                    Ok(models) => {
                        let model_ids: Vec<String> =
                            models.into_iter().map(|m| m.id).collect();
                        sender.send(TuiUpdate::ModelsLoaded(model_ids)).unwrap();
                    }
                    Err(e) => {
                        error!("Failed to load models: {}", e);
                        sender.send(TuiUpdate::Error(format!("Failed to load models: {}", e))).unwrap();
                    }
                }
            });
        }
        
        // Load sessions asynchronously
        self.refresh_sessions().await;
        
        Ok(())
    }
    
    /// Chat with current settings
    pub async fn chat(&self, message: &str) -> Result<String, Box<dyn std::error::Error>> {
        let session_guard = self.active_session.read().await;
        let model_guard = self.current_model.read().await;

        let session_id = if let Some(session) = session_guard.as_ref() {
            session.context.session_id.clone()
        } else {
            drop(session_guard);
            let session = self
                .client
                .create_session(&SessionCreateParams {
                    context: None,
                    model: Some(model_guard.clone()),
                })
                .await?;
            let mut active_sess = self.active_session.write().await;
            *active_sess = Some(session.clone());
            session.context.session_id.clone()
        };

        let response = self
            .client
            .send_message(
                &session_id,
                &TalosMessageRequest {
                    input: vec![TalosInputItem::Text {
                        text: message.to_string(),
                    }],
                    stream: Some(false),
                    workspace_id: None,
                },
            )
            .await?;
        Ok(extract_output_text(&response))
    }
    
    /// Create a new session
    pub async fn create_session(
        &self,
        params: &SessionCreateParams,
    ) -> Result<TalosSession, Box<dyn std::error::Error>> {
        let session = self.client.create_session(params).await?;
        
        // Update active session
        {
            let mut active_sess = self.active_session.write().await;
            *active_sess = Some(session.clone());
        }
        
        // Refresh session list
        self.refresh_sessions().await;
        
        // Notify update
        if let Some(sender) = &self.update_sender {
            sender.send(TuiUpdate::SessionCreated(session.clone())).unwrap();
        }
        
        Ok(session)
    }
    
    /// Switch to an existing session
    pub async fn switch_session(&self, session_id: String) -> Result<(), Box<dyn std::error::Error>> {
        let session = self.client.get_session(&session_id).await?;

        let mut active_session = self.active_session.write().await;
        *active_session = Some(session);
        
        Ok(())
    }
    
    /// Refresh the list of available sessions
    pub async fn refresh_sessions(&self) {
        let client = self.client.clone();
        let sender = self.update_sender.as_ref().unwrap().clone();
        
        tokio::spawn(async move {
            match client.list_sessions().await {
                Ok(sessions) => {
                    sender.send(TuiUpdate::SessionsList(sessions)).unwrap();
                }
                Err(e) => {
                    error!("Failed to refresh sessions: {}", e);
                    sender.send(TuiUpdate::Error(format!("Failed to refresh sessions: {}", e))).unwrap();
                }
            }
        });
    }
    
    /// Change current model
    pub async fn set_model(&self, model: String) {
        let mut current_model = self.current_model.write().await;
        *current_model = model;
    }
    
    /// Get current model
    pub async fn get_current_model(&self) -> String {
        self.current_model.read().await.clone()
    }
    
    /// Get active session info
    pub async fn get_active_session(&self) -> Option<TalosSession> {
        self.active_session.read().await.clone()
    }
    
    /// Get available models
    pub async fn get_available_models(&self) -> Vec<String> {
        self.available_models.read().await.clone()
    }
    
    /// Get available sessions list
    pub async fn get_available_sessions(&self) -> Vec<TalosSession> {
        self.available_sessions.read().await.clone()
    }
    
    /// Process UI updates (to be called periodically by TUI)
    pub async fn process_updates(&self) -> Option<TuiUpdate> {
        let mut receiver_guard = self.update_receiver.write().await;
        
        if let Some(ref mut receiver) = *receiver_guard {
            match receiver.recv().now_or_never() {
                Some(update) => Some(update),
                None => None,
            }
        } else {
            None
        }
    }
    
    /// Handle an update from async operations
    pub async fn handle_update(&self, update: TuiUpdate) {
        match update {
            TuiUpdate::ModelsLoaded(models) => {
                let mut models_guard = self.available_models.write().await;
                *models_guard = models;
            },
            TuiUpdate::SessionsList(sessions) => {
                let mut sessions_guard = self.available_sessions.write().await;
                *sessions_guard = sessions;
            },
            TuiUpdate::Error(msg) => {
                error!("TUI Update error: {}", msg);
            },
            _ => {
                // Other updates handled by calling UI code
            }
        }
    }
}

fn extract_output_text(response: &TalosMessageResponse) -> String {
    let texts: Vec<String> = response
        .output
        .iter()
        .filter_map(|item| match item {
            TalosOutputItem::OutputText { text } => Some(text.clone()),
            _ => None,
        })
        .collect();

    if texts.is_empty() {
        return String::new();
    }

    texts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tui_app_state_creation() {
        // This would require a mocked client in real scenario
        // but we can at least test struct creation
        let _state = TuiAppState::new(Arc::new(crate::TalosClient::new_default().unwrap()));
    }
}
