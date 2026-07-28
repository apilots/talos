mod control;
mod projection;
mod service;

pub(super) const MAX_AGENT_TREE_NODES: usize = 128;

pub(crate) use service::SubagentService;

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
