//! Ephemeral tool file bytes authorized by the original Turn/File owner.
//! Never serialize these values into model arguments, events or persisted messages.
#[derive(Clone)]
pub struct ToolInputFile {
    pub file_id: String,
    pub filename: String,
    pub bytes: bytes::Bytes,
}
#[derive(Clone)]
pub struct ToolOutputFile {
    pub filename: String,
    pub bytes: bytes::Bytes,
}
impl std::fmt::Debug for ToolInputFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolInputFile")
            .field("file_id", &self.file_id)
            .field("filename", &self.filename)
            .field("size", &self.bytes.len())
            .finish()
    }
}
