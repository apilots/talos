use crate::store::SessionTranscriptRecord;

pub trait SessionTranscriptMapper<T>: Send + Sync {
    fn to_record(&self, value: &T) -> SessionTranscriptRecord;

    fn from_record(&self, record: SessionTranscriptRecord) -> T;
}
