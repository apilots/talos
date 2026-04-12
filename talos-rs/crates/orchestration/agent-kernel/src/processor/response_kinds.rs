#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResponseProcessorKind {
    Planning = 0,
    ToolCallExtraction = 1,
    FinalAnswerExtraction = 2,
    StateDelta = 3,
    EventProjection = 4,
    CodeExecution = 5,
}
