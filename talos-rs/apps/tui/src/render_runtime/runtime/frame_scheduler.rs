//! Frame request coalescing for the render runtime event loop.

/// Coalesces frame requests so multiple state changes produce one draw.
#[derive(Debug, Default)]
pub(crate) struct FrameScheduler {
    frame_requested: bool,
}

impl FrameScheduler {
    pub(crate) fn request_frame(&mut self) {
        self.frame_requested = true;
    }

    pub(crate) fn take_frame_request(&mut self) -> bool {
        let requested = self.frame_requested;
        self.frame_requested = false;
        requested
    }
}

#[cfg(test)]
mod tests {
    use super::FrameScheduler;

    #[test]
    fn coalesces_frame_requests() {
        let mut scheduler = FrameScheduler::default();
        scheduler.request_frame();
        scheduler.request_frame();

        assert!(scheduler.take_frame_request());
        assert!(!scheduler.take_frame_request());
    }
}
