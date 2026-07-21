use talos_memory::{MemoryScope, MemoryWriteBatch, MemoryWriter};

pub struct NoopMemoryWriter;

impl MemoryWriter for NoopMemoryWriter {
    fn write<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _batch: MemoryWriteBatch,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = talos_error::TalosResult<()>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}
