use semver::Version;
use talos_config::{parse_timezone, validate_user_workspace_segment};
use talos_error::TalosResult;

use crate::runner::{
    UpgradeContext, UpgradeFuture, UpgradeTask, UpgradeTaskDefinition, UpgradeTaskRegistration,
};

mod converter;

const DREAM_MEMORY_OKF_UPGRADE_ID: &str = "202608290002_dream_memory_okf";

pub(crate) fn upgrade_task() -> Box<dyn UpgradeTask> {
    Box::new(UpgradeTaskRegistration::new(
        UpgradeTaskDefinition {
            id: DREAM_MEMORY_OKF_UPGRADE_ID,
            revision: 1,
            target_version: Version::new(0, 1, 7),
            priority: 100,
        },
        run_dream_memory_okf,
    ))
}

fn run_dream_memory_okf<'a>(context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async move { convert_legacy_dream_memory(context).await })
}

async fn convert_legacy_dream_memory(context: &UpgradeContext<'_>) -> TalosResult<()> {
    let users_root = context
        .paths
        .legacy_workspace_dir_for_agent(&context.config.agent.name)
        .join("users");
    let Some(mut entries) = talos_fs::read_directory(&users_root)? else {
        return Ok(());
    };
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let workspace_id = context.config.default_workspace_id();
    let memory_dirname = &context.config.memory.memory_dirname;
    let timezone = parse_timezone(&context.config.agent.kernel.timezone)?;
    for entry in entries {
        if !entry.metadata.is_dir || entry.metadata.is_symlink {
            continue;
        }
        let Some(user_id) = entry.path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if user_id == "orphaned" || validate_user_workspace_segment(user_id).is_err() {
            continue;
        }
        let Some(conversion) = converter::convert_legacy_scope(
            workspace_id,
            user_id,
            &entry.path,
            memory_dirname,
            timezone,
        )
        .await?
        else {
            continue;
        };
        conversion.finish().await?;
    }
    Ok(())
}
