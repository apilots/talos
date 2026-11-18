use std::path::Path;

use talos_error::TalosResult;
use talos_types::{PluginId, PluginInstall};

use super::{install_id_for, remove_plugin_directory, rename_plugin_path, InstallActivation};
use crate::PluginStateStore;

/// The registry still names the prior installation, so a replacement backup
/// left by an interrupted install is the rollback source.
pub(crate) fn recover_replacement_backup(
    state: &PluginStateStore,
    previous: &PluginInstall,
    replacement_id: &PluginId,
) -> TalosResult<()> {
    let backup = state.transaction_backup_dir(&install_id_for(replacement_id));
    if backup.exists() {
        restore_install_dir(&previous.installed_path, Some(&backup))?;
    }
    Ok(())
}

pub(crate) fn rollback_install_activation(
    _state: &PluginStateStore,
    activation: &InstallActivation,
) -> TalosResult<()> {
    restore_install_dir(
        &activation.install.installed_path,
        activation.backup_dir.as_deref(),
    )
}

fn restore_install_dir(install_dir: &Path, backup_dir: Option<&Path>) -> TalosResult<()> {
    if install_dir.exists() {
        remove_plugin_directory(install_dir, "remove failed plugin install")?;
    }
    if let Some(backup_dir) = backup_dir.filter(|path| path.exists()) {
        rename_plugin_path(backup_dir, install_dir, "restore plugin install backup")?;
    }
    Ok(())
}
