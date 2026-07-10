//! Event-driven runtime controller for the render runtime.

use crate::render_runtime::runtime::dirty::DirtyFlags;
use crate::render_runtime::runtime::event_loop::TuiRuntimeEvent;
use crate::render_runtime::runtime::frame_scheduler::FrameScheduler;

/// Owns render runtime redraw and flush decisions.
#[derive(Debug, Default)]
pub(crate) struct RuntimeController {
    dirty: DirtyFlags,
    scheduler: FrameScheduler,
    last_frame_fingerprint: Option<u64>,
}

impl RuntimeController {
    pub(crate) fn handle_event(&mut self, event: TuiRuntimeEvent) {
        match event {
            TuiRuntimeEvent::Input | TuiRuntimeEvent::Paste => {
                self.dirty.mark_interaction();
                self.scheduler.request_frame();
            }
            TuiRuntimeEvent::Mouse => {
                self.dirty.mark_transcript_viewport();
                self.scheduler.request_frame();
            }
            TuiRuntimeEvent::Resize => {
                self.dirty.mark_interaction();
                self.dirty.mark_transcript_viewport();
                self.scheduler.request_frame();
            }
            TuiRuntimeEvent::Stream => {
                self.dirty.mark_interaction();
                self.scheduler.request_frame();
            }
            TuiRuntimeEvent::HistoryLoaded => {
                self.dirty.mark_transcript_viewport();
                self.scheduler.request_frame();
            }
            TuiRuntimeEvent::LiveCommitTick => {
                self.dirty.mark_transcript_viewport();
                self.scheduler.request_frame();
            }
        }
    }

    pub(crate) fn should_draw_frame(&mut self, fingerprint: u64) -> bool {
        let dirty = self.dirty;
        let frame_requested = self.scheduler.take_frame_request();
        if frame_requested {
            self.dirty.clear();
        }
        if frame_requested && dirty.any() {
            self.last_frame_fingerprint = Some(fingerprint);
            return true;
        }
        if self.last_frame_fingerprint == Some(fingerprint) {
            return false;
        }
        self.last_frame_fingerprint = Some(fingerprint);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeController;
    use crate::render_runtime::runtime::event_loop::TuiRuntimeEvent;

    #[test]
    fn frame_fingerprint_gate_skips_unchanged_frames() {
        let mut runtime = RuntimeController::default();

        assert!(runtime.should_draw_frame(1));
        assert!(!runtime.should_draw_frame(1));
        assert!(runtime.should_draw_frame(2));
        assert!(!runtime.should_draw_frame(2));
    }

    #[test]
    fn runtime_frame_request_draws_even_when_fingerprint_is_unchanged() {
        let mut runtime = RuntimeController::default();

        assert!(runtime.should_draw_frame(1));
        assert!(!runtime.should_draw_frame(1));

        runtime.handle_event(TuiRuntimeEvent::Resize);

        assert!(runtime.should_draw_frame(1));
        assert!(!runtime.should_draw_frame(1));
    }
}
