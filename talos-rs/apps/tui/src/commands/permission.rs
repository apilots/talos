use super::*;
use crate::permission_picker::permission_mode_label;

impl TuiApp {
    pub(crate) fn confirm_permission_picker(&mut self) {
        let Some(mode) = self
            .permission_picker
            .as_mut()
            .and_then(PermissionPickerState::confirm)
        else {
            return;
        };
        if mode == self.current_permission_mode {
            self.permission_picker = None;
            return;
        }
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            self.permission_picker = None;
            return;
        };
        let _ = self.spawn_command_task(
            format!("updating permissions to {}", permission_mode_label(mode)),
            async move {
                CommandUpdate::PermissionModeSwitched(
                    client
                        .update_session_permission_mode(&session_id, mode)
                        .await,
                )
            },
        );
        self.permission_picker = None;
    }
}
