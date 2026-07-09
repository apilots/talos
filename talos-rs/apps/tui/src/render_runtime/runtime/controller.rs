//! Event-driven runtime controller for the render runtime.

use crate::render_runtime::runtime::dirty::DirtyFlags;
use crate::render_runtime::runtime::event_loop::TuiRuntimeEvent;
use crate::render_runtime::runtime::frame_scheduler::FrameScheduler;

/// Work that must be performed serially during one event-loop cycle.
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeCycle {
    pub(crate) flush_terminal_transcript: bool,
    pub(crate) commit_live_stream: bool,
    pub(crate) draw_frame: bool,
}

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
                self.scheduler.request_live_commit();
            }
            TuiRuntimeEvent::HistoryLoaded => {
                self.dirty.mark_transcript_viewport();
                self.dirty.mark_terminal_transcript();
                self.scheduler.request_frame();
            }
            TuiRuntimeEvent::LiveCommitTick => {
                self.dirty.mark_transcript_viewport();
                self.dirty.mark_terminal_transcript();
                self.scheduler.request_frame();
            }
            #[cfg(test)]
            TuiRuntimeEvent::FrameRequested => {
                self.scheduler.request_frame();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn take_cycle(&mut self) -> RuntimeCycle {
        let dirty = self.dirty;
        let frame_requested = self.scheduler.take_frame_request();
        let cycle = RuntimeCycle {
            flush_terminal_transcript: dirty.terminal_transcript,
            commit_live_stream: self.scheduler.take_live_commit_request(),
            draw_frame: frame_requested && dirty.any(),
        };
        self.dirty.clear();
        cycle
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
    use super::{RuntimeController, RuntimeCycle};
    use crate::render_runtime::runtime::event_loop::TuiRuntimeEvent;

    #[test]
    fn idle_cycle_does_not_draw_or_flush() {
        let mut runtime = RuntimeController::default();

        assert_eq!(runtime.take_cycle(), RuntimeCycle::default());
    }

    #[test]
    fn input_event_requests_interaction_draw_without_terminal_flush() {
        let mut runtime = RuntimeController::default();
        runtime.handle_event(TuiRuntimeEvent::Input);

        assert_eq!(
            runtime.take_cycle(),
            RuntimeCycle {
                flush_terminal_transcript: false,
                commit_live_stream: false,
                draw_frame: true,
            }
        );
        assert_eq!(runtime.take_cycle(), RuntimeCycle::default());
    }

    #[test]
    fn history_loaded_flushes_terminal_before_frame() {
        let mut runtime = RuntimeController::default();
        runtime.handle_event(TuiRuntimeEvent::HistoryLoaded);

        assert_eq!(
            runtime.take_cycle(),
            RuntimeCycle {
                flush_terminal_transcript: true,
                commit_live_stream: false,
                draw_frame: true,
            }
        );
    }

    #[test]
    fn stream_event_requests_live_commit_and_frame() {
        let mut runtime = RuntimeController::default();
        runtime.handle_event(TuiRuntimeEvent::Stream);

        assert_eq!(
            runtime.take_cycle(),
            RuntimeCycle {
                flush_terminal_transcript: false,
                commit_live_stream: true,
                draw_frame: true,
            }
        );
    }

    #[test]
    fn frame_request_without_dirty_state_does_not_draw() {
        let mut runtime = RuntimeController::default();
        runtime.handle_event(TuiRuntimeEvent::FrameRequested);

        assert_eq!(runtime.take_cycle(), RuntimeCycle::default());
    }

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
