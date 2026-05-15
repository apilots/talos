use talos_types::talos::TalosContext;

pub(super) fn route_channel(context: &TalosContext) -> String {
    match context.channel_id.as_deref() {
        Some(channel_id) => format!("{}:{}", context.provider, channel_id),
        None => context.provider.to_string(),
    }
}
