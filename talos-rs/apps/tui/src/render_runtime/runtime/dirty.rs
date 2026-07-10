//! Dirty flags used to avoid full redraws when render runtime state is unchanged.

/// Tracks which render runtime UI projections need repainting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirtyFlags {
    pub(crate) interaction: bool,
    pub(crate) transcript_viewport: bool,
}

impl DirtyFlags {
    pub(crate) fn any(self) -> bool {
        self.interaction || self.transcript_viewport
    }

    pub(crate) fn mark_interaction(&mut self) {
        self.interaction = true;
    }

    pub(crate) fn mark_transcript_viewport(&mut self) {
        self.transcript_viewport = true;
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}
