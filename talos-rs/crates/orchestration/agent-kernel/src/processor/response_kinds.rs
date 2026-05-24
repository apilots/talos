#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResponseProcessorKind {
    Planning = 0,
    StateDelta = 3,
    EventProjection = 4,
    CodeExecution = 5,
}
