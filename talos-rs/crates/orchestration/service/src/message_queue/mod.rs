mod service;

pub(crate) use service::MessageQueueReservation;
pub use service::SessionMessageQueueService;

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
