use talos_types::{
    Plugin, PluginListRequest, PluginListStatusFilter, PluginSearchRequest,
    PluginSearchStatusFilter, PluginStatus,
};

pub(super) fn plugin_matches_search(plugin: &Plugin, request: &PluginSearchRequest) -> bool {
    if request.kind.is_some_and(|kind| kind != plugin.kind) {
        return false;
    }
    if !matches_status(plugin, request.status) {
        return false;
    }
    if let Some(query) = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        let query = query.to_lowercase();
        return plugin.name.to_lowercase().contains(&query)
            || plugin.description.to_lowercase().contains(&query);
    }
    true
}

pub(super) fn plugin_matches_list(plugin: &Plugin, request: &PluginListRequest) -> bool {
    if request
        .source_id
        .as_ref()
        .is_some_and(|source_id| plugin.source_id.as_ref() != Some(source_id))
    {
        return false;
    }
    match request.status {
        Some(PluginListStatusFilter::Installed) => {
            matches!(plugin.status, PluginStatus::Installed { enabled: true, .. })
        }
        Some(PluginListStatusFilter::Disabled) => {
            matches!(
                plugin.status,
                PluginStatus::Installed { enabled: false, .. }
            )
        }
        Some(PluginListStatusFilter::UpdateAvailable) => {
            matches!(plugin.status, PluginStatus::UpdateAvailable { .. })
        }
        Some(PluginListStatusFilter::SourceMissing) => {
            matches!(plugin.status, PluginStatus::SourceMissing { .. })
        }
        Some(PluginListStatusFilter::Conflict) => {
            matches!(plugin.status, PluginStatus::Conflict { .. })
        }
        None => true,
    }
}

fn matches_status(plugin: &Plugin, status: Option<PluginSearchStatusFilter>) -> bool {
    match status.unwrap_or(PluginSearchStatusFilter::Any) {
        PluginSearchStatusFilter::Any => true,
        PluginSearchStatusFilter::NotInstalled => {
            matches!(plugin.status, PluginStatus::NotInstalled)
        }
        PluginSearchStatusFilter::Installed => {
            matches!(plugin.status, PluginStatus::Installed { .. })
        }
        PluginSearchStatusFilter::UpdateAvailable => {
            matches!(plugin.status, PluginStatus::UpdateAvailable { .. })
        }
        PluginSearchStatusFilter::Conflict => {
            matches!(plugin.status, PluginStatus::Conflict { .. })
        }
    }
}
