//! TUI event loop and top-level entry points.

use std::collections::hash_map::DefaultHasher;
use std::{
    error::Error,
    fmt::Debug,
    hash::{Hash, Hasher},
    io,
    sync::Arc,
};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};
#[cfg(test)]
use ratatui::{
    backend::Backend,
    widgets::{Paragraph, Widget},
    Terminal,
};
use ratatui::{
    layout::Size,
    style::{Color, Style},
    text::{Line, Span},
};

use crate::render::chat_lines::render_transcript_block_lines;
use crate::render_runtime::runtime::event_loop::{
    CrosstermRuntimeEventSource, TerminalRuntimeEvent,
};
use crate::render_runtime::transcript::block::TranscriptBlockKind;
use crate::render_runtime::transcript::store::PendingTerminalBlock;
use crate::terminal_adapter::PrimaryTerminal;

use super::{GatewayClient, TuiApp};

pub async fn run_tui_app_with_gateway_client(
    client: Arc<dyn GatewayClient>,
) -> Result<(), Box<dyn Error>> {
    let mut stdout = io::stdout();
    reset_primary_screen_viewport(&mut stdout)?;
    let shutdown_client = client.clone();
    let mut app = TuiApp::new_with_gateway_client(client);
    app.initialize_session().await;

    let (terminal_width, terminal_height) = crossterm::terminal::size()?;
    let inline_viewport_height = app.inline_viewport_height(terminal_width, terminal_height.max(1));
    let mut terminal = PrimaryTerminal::new(
        terminal_width,
        inline_viewport_height,
        terminal_height.max(1),
    )?;
    enable_raw_mode()?;
    if let Err(err) = enter_primary_screen_interaction(terminal.backend_mut()) {
        disable_raw_mode()?;
        return Err(Box::new(err));
    }

    let run_result = run_event_loop(&mut terminal, &mut app).await;
    let shutdown_result = shutdown_client.shutdown().await;

    let flush_result = flush_terminal_transcript(&mut terminal, &mut app);
    let final_clear_result = if app.should_exit {
        terminal.clear_viewport()
    } else {
        Ok(())
    };
    let leave_result = leave_primary_screen_interaction(terminal.backend_mut());
    let raw_mode_result = disable_raw_mode();

    run_result?;
    flush_result?;
    final_clear_result?;
    leave_result?;
    raw_mode_result?;
    shutdown_result?;
    Ok(())
}

async fn run_event_loop(
    terminal: &mut PrimaryTerminal,
    app: &mut TuiApp,
) -> Result<(), Box<dyn Error>> {
    let mut event_source = CrosstermRuntimeEventSource::default();
    loop {
        app.on_tick().await;
        if app.should_exit {
            return Ok(());
        }
        sync_terminal_viewport(terminal, app)?;
        let flushed_transcript = flush_terminal_transcript(terminal, app)?;
        let frame_fingerprint = tui_frame_fingerprint(app);
        if flushed_transcript || app.render_runtime.should_draw_frame(frame_fingerprint) {
            terminal.draw(|frame| app.render_ui(frame))?;
        }

        let terminal_event = event_source.next_event()?;
        if let Some(runtime_event) = terminal_event.runtime_event() {
            app.render_runtime.handle_runtime_event(runtime_event);
        }
        match terminal_event {
            TerminalRuntimeEvent::Key(key) => {
                if app.handle_input(key).await? {
                    return Ok(());
                }
            }
            TerminalRuntimeEvent::Paste(text) => app.handle_paste(text),
            TerminalRuntimeEvent::Mouse(_) => {}
            TerminalRuntimeEvent::Resize(_, _)
            | TerminalRuntimeEvent::Tick
            | TerminalRuntimeEvent::Ignored => {}
        }
    }
}

fn reset_primary_screen_viewport(output: &mut impl io::Write) -> io::Result<()> {
    execute!(
        output,
        MoveTo(0, 0),
        Clear(ClearType::All),
        Clear(ClearType::Purge),
        MoveTo(0, 0)
    )
}

fn enter_primary_screen_interaction(output: &mut impl io::Write) -> io::Result<()> {
    execute!(output, EnableBracketedPaste, Hide)
}

fn leave_primary_screen_interaction(output: &mut impl io::Write) -> io::Result<()> {
    execute!(output, DisableBracketedPaste, Show)
}

fn sync_terminal_viewport(terminal: &mut PrimaryTerminal, app: &TuiApp) -> io::Result<()> {
    let (width, height) = crossterm::terminal::size()?;
    let viewport_height = app.inline_viewport_height(width, height.max(1));
    terminal.resize_viewport(width, viewport_height, height.max(1))
}

fn flush_terminal_transcript<T: TerminalTranscriptTarget>(
    terminal: &mut T,
    app: &mut TuiApp,
) -> io::Result<bool> {
    if !app.in_flight && app.turn_metrics_live {
        return Ok(false);
    }

    let pending = app.render_runtime.pending_terminal_projection();
    if pending.is_empty() {
        return flush_terminal_round_footer(terminal, app);
    }
    if app.in_flight && pending.iter().any(is_assistant_text_block) {
        return Ok(false);
    }
    if pending.len() == 1 {
        match pending[0].kind {
            TranscriptBlockKind::UserInput if !app.should_exit && !app.in_flight => {
                return Ok(false);
            }
            TranscriptBlockKind::CommandInput => return Ok(false),
            _ => {}
        }
    }

    let terminal_width = terminal.size()?.width.max(1) as usize;
    let content_width = terminal_width.saturating_sub(crate::CHAT_CONTENT_PADDING_LEFT as usize);
    let block_ids = pending
        .iter()
        .map(|block| block.block_id)
        .collect::<Vec<_>>();
    let mut rendered_lines = render_terminal_transcript_lines(app, &pending, content_width);
    let segment_footer_target = terminal_projection_footer_target(app, &pending);
    let response_footer_target = if segment_footer_target.is_none() {
        terminal_projection_pending_response_footer_target(app, &pending)
    } else {
        None
    };
    let footer_target = segment_footer_target.or(response_footer_target);
    if response_footer_target.is_some() && !rendered_lines.is_empty() {
        rendered_lines.push(Line::from(""));
        rendered_lines.push(terminal_round_footer_line(
            app.turn_metrics
                .as_ref()
                .and_then(|metrics| metrics.duration_ms),
        ));
    }
    if rendered_lines.is_empty() {
        app.render_runtime.mark_terminal_blocks_synced(block_ids);
        app.rendered_chat_cache_key = None;
        return Ok(false);
    }

    app.rendered_chat_cache_key = None;
    app.suppress_transcript_for_terminal_flush = true;
    let clean_draw_result = terminal.draw_app(app);
    app.suppress_transcript_for_terminal_flush = false;
    clean_draw_result?;
    terminal.insert_history_lines(&rendered_lines)?;
    app.render_runtime.mark_terminal_blocks_synced(block_ids);
    if let Some(block_id) = footer_target {
        app.terminal_footer_synced_for = Some(block_id);
    }
    Ok(true)
}

fn is_assistant_text_block(block: &PendingTerminalBlock) -> bool {
    matches!(
        block.kind,
        TranscriptBlockKind::Thinking
            | TranscriptBlockKind::Commentary
            | TranscriptBlockKind::FinalAnswer
    )
}

fn flush_terminal_round_footer<T: TerminalTranscriptTarget>(
    terminal: &mut T,
    app: &mut TuiApp,
) -> io::Result<bool> {
    let Some(block_id) = completed_round_footer_target(app) else {
        return Ok(false);
    };
    if app.terminal_footer_synced_for == Some(block_id) {
        return Ok(false);
    }

    let rendered_lines = vec![
        Line::from(""),
        terminal_round_footer_line(
            app.turn_metrics
                .as_ref()
                .and_then(|metrics| metrics.duration_ms),
        ),
    ];
    app.rendered_chat_cache_key = None;
    app.suppress_transcript_for_terminal_flush = true;
    let clean_draw_result = terminal.draw_app(app);
    app.suppress_transcript_for_terminal_flush = false;
    clean_draw_result?;
    terminal.insert_history_lines(&rendered_lines)?;
    app.terminal_footer_synced_for = Some(block_id);
    Ok(true)
}

trait TerminalTranscriptTarget {
    fn size(&self) -> io::Result<Size>;
    fn draw_app(&mut self, app: &mut TuiApp) -> io::Result<()>;
    fn insert_history_lines(&mut self, lines: &[Line<'static>]) -> io::Result<()>;
}

impl TerminalTranscriptTarget for PrimaryTerminal {
    fn size(&self) -> io::Result<Size> {
        PrimaryTerminal::size(self)
    }

    fn draw_app(&mut self, app: &mut TuiApp) -> io::Result<()> {
        self.draw(|frame| app.render_ui(frame))
    }

    fn insert_history_lines(&mut self, lines: &[Line<'static>]) -> io::Result<()> {
        PrimaryTerminal::insert_history_lines(self, lines)
    }
}

#[cfg(test)]
impl<B: Backend> TerminalTranscriptTarget for Terminal<B> {
    fn size(&self) -> io::Result<Size> {
        Terminal::size(self)
    }

    fn draw_app(&mut self, app: &mut TuiApp) -> io::Result<()> {
        self.draw(|frame| app.render_ui(frame)).map(|_| ())
    }

    fn insert_history_lines(&mut self, lines: &[Line<'static>]) -> io::Result<()> {
        let height = lines.len().min(u16::MAX as usize) as u16;
        self.insert_before(height, |buf| {
            Paragraph::new(lines.to_vec()).render(buf.area, buf);
            suppress_wide_symbol_placeholder_cells(buf);
        })
    }
}

fn render_terminal_transcript_lines(
    app: &TuiApp,
    blocks: &[PendingTerminalBlock],
    content_width: usize,
) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    if terminal_projection_needs_leading_gap(app, blocks) {
        rendered.push(Line::from(""));
    }
    let mut block_index = 0usize;
    while block_index < blocks.len() {
        let segment_start = block_index;
        let segment_end = ((segment_start + 1)..blocks.len())
            .find(|&index| is_terminal_segment_start(blocks[index].kind))
            .unwrap_or(blocks.len());

        let segment_blocks = terminal_segment_blocks(segment_start, segment_end);
        for (offset, block_index) in segment_blocks.iter().enumerate() {
            let render_kind = terminal_render_kind_for_block(
                blocks,
                segment_start,
                segment_end,
                *block_index,
                app,
            );
            let lines = blocks[*block_index]
                .lines
                .iter()
                .cloned()
                .map(|line| (None, line))
                .collect::<Vec<_>>();
            rendered.extend(
                render_transcript_block_lines(render_kind, &lines, content_width)
                    .into_iter()
                    .map(|line| line.display),
            );
            if offset + 1 < segment_blocks.len() {
                rendered.push(Line::from(""));
            }
        }

        if terminal_segment_is_completed_round(app, blocks, segment_start, segment_end) {
            rendered.push(Line::from(""));
            rendered.push(terminal_round_footer_line(terminal_segment_duration_ms(
                app,
                blocks,
                segment_end,
            )));
        }

        if segment_end < blocks.len() {
            rendered.push(Line::from(""));
        }
        block_index = segment_end;
    }
    rendered
}

fn terminal_projection_needs_leading_gap(app: &TuiApp, blocks: &[PendingTerminalBlock]) -> bool {
    let Some(first_pending) = blocks.first() else {
        return false;
    };
    let Some(first_pending_index) = app
        .render_runtime
        .store()
        .blocks()
        .iter()
        .position(|block| block.id == first_pending.block_id)
    else {
        return false;
    };
    app.render_runtime.store().blocks()[..first_pending_index]
        .iter()
        .any(|block| block.terminal_synced)
}

fn is_terminal_segment_start(kind: TranscriptBlockKind) -> bool {
    matches!(
        kind,
        TranscriptBlockKind::UserInput | TranscriptBlockKind::CommandInput
    )
}

fn terminal_segment_blocks(segment_start: usize, segment_end: usize) -> Vec<usize> {
    (segment_start..segment_end).collect()
}

fn terminal_segment_is_completed_round(
    app: &TuiApp,
    blocks: &[PendingTerminalBlock],
    segment_start: usize,
    segment_end: usize,
) -> bool {
    if segment_start >= blocks.len() || blocks[segment_start].kind != TranscriptBlockKind::UserInput
    {
        return false;
    }
    if app.in_flight || app.turn_metrics_live {
        return false;
    }
    blocks[segment_start..segment_end].iter().any(|block| {
        matches!(
            block.kind,
            TranscriptBlockKind::Commentary | TranscriptBlockKind::FinalAnswer
        )
    })
}

fn terminal_projection_footer_target(
    app: &TuiApp,
    blocks: &[PendingTerminalBlock],
) -> Option<crate::render_runtime::transcript::block::TranscriptBlockId> {
    if app.in_flight || app.turn_metrics_live {
        return None;
    }
    let mut block_index = 0usize;
    let mut footer_target = None;
    while block_index < blocks.len() {
        let segment_start = block_index;
        let segment_end = ((segment_start + 1)..blocks.len())
            .find(|&index| is_terminal_segment_start(blocks[index].kind))
            .unwrap_or(blocks.len());
        if terminal_segment_is_completed_round(app, blocks, segment_start, segment_end) {
            footer_target = Some(blocks[segment_start].block_id);
        }
        block_index = segment_end;
    }
    footer_target
}

fn terminal_projection_pending_response_footer_target(
    app: &TuiApp,
    blocks: &[PendingTerminalBlock],
) -> Option<crate::render_runtime::transcript::block::TranscriptBlockId> {
    let block_id = completed_round_footer_target(app)?;
    if app.terminal_footer_synced_for == Some(block_id) {
        return None;
    }
    blocks
        .iter()
        .any(|block| {
            matches!(
                block.kind,
                TranscriptBlockKind::Commentary | TranscriptBlockKind::FinalAnswer
            )
        })
        .then_some(block_id)
}

fn completed_round_footer_target(
    app: &TuiApp,
) -> Option<crate::render_runtime::transcript::block::TranscriptBlockId> {
    if app.in_flight || app.turn_metrics_live {
        return None;
    }
    let blocks = app.render_runtime.store().blocks();
    let segment_start = blocks
        .iter()
        .rposition(|block| is_terminal_segment_start(block.kind))?;
    let first = &blocks[segment_start];
    if first.kind != TranscriptBlockKind::UserInput {
        return None;
    }
    let has_response = blocks[segment_start..].iter().any(|block| {
        matches!(
            block.kind,
            TranscriptBlockKind::Commentary | TranscriptBlockKind::FinalAnswer
        )
    });
    has_response.then_some(first.id)
}

fn terminal_segment_duration_ms(
    app: &TuiApp,
    blocks: &[PendingTerminalBlock],
    segment_end: usize,
) -> Option<u64> {
    if segment_end == blocks.len() && !app.in_flight && !app.turn_metrics_live {
        return app
            .turn_metrics
            .as_ref()
            .and_then(|metrics| metrics.duration_ms);
    }
    None
}

fn terminal_render_kind_for_block(
    blocks: &[PendingTerminalBlock],
    segment_start: usize,
    segment_end: usize,
    block_index: usize,
    app: &TuiApp,
) -> TranscriptBlockKind {
    if blocks[block_index].kind != TranscriptBlockKind::Commentary
        || app.in_flight
        || app.turn_metrics_live
        || blocks[segment_start..segment_end]
            .iter()
            .any(|block| block.kind == TranscriptBlockKind::FinalAnswer)
    {
        return blocks[block_index].kind;
    }

    let last_commentary = blocks[segment_start..segment_end]
        .iter()
        .enumerate()
        .filter(|(_, block)| block.kind == TranscriptBlockKind::Commentary)
        .map(|(offset, _)| segment_start + offset)
        .next_back();

    if last_commentary == Some(block_index) {
        TranscriptBlockKind::FinalAnswer
    } else {
        blocks[block_index].kind
    }
}

fn terminal_round_footer_line(duration_ms: Option<u64>) -> Line<'static> {
    let duration_text = duration_ms.map_or_else(
        || "✻ Brewed for a moment".to_string(),
        TuiApp::format_round_duration,
    );
    Line::from(Span::styled(
        duration_text,
        Style::default().fg(Color::White),
    ))
}

#[cfg(test)]
fn suppress_wide_symbol_placeholder_cells(buffer: &mut ratatui::buffer::Buffer) {
    for y in buffer.area.top()..buffer.area.bottom() {
        let mut cells_to_skip = 0usize;
        for x in buffer.area.left()..buffer.area.right() {
            let cell = &mut buffer[(x, y)];
            if cells_to_skip > 0 {
                cell.set_symbol("");
                cells_to_skip -= 1;
                continue;
            }
            cells_to_skip = crate::utils::text_display_width(cell.symbol()).saturating_sub(1);
        }
    }
}

fn tui_frame_fingerprint(app: &TuiApp) -> u64 {
    let mut hasher = DefaultHasher::new();
    app.input_buffer.hash(&mut hasher);
    app.cursor_index.hash(&mut hasher);
    hash_debug(&app.pasted_blocks, &mut hasher);
    app.selected_pasted_block.hash(&mut hasher);
    app.current_model.hash(&mut hasher);
    app.connection_status.hash(&mut hasher);
    hash_debug(&app.rendered_chat_cache_key, &mut hasher);
    app.suppress_transcript_for_terminal_flush.hash(&mut hasher);
    hash_debug(&app.render_runtime.live_projection_lines(), &mut hasher);
    hash_debug(&app.message_queue, &mut hasher);
    app.in_flight.hash(&mut hasher);
    app.received_output_delta.hash(&mut hasher);
    hash_debug(&app.turn_metrics, &mut hasher);
    app.turn_metrics_live.hash(&mut hasher);
    app.session_usage.total_tokens.hash(&mut hasher);
    app.trace_event_count.hash(&mut hasher);
    app.status_hint
        .as_ref()
        .map(|(text, _)| text)
        .hash(&mut hasher);
    if app.is_agent_working() {
        app.spinner_index.hash(&mut hasher);
    }
    app.history_sessions.len().hash(&mut hasher);
    app.show_model_picker.hash(&mut hasher);
    app.model_picker_selected.hash(&mut hasher);
    app.show_sessions_picker.hash(&mut hasher);
    app.sessions_picker_selected.hash(&mut hasher);
    app.show_command_autocomplete.hash(&mut hasher);
    app.command_autocomplete_selected.hash(&mut hasher);
    app.command_input_buffer.hash(&mut hasher);
    hash_debug(&app.command_suggestions, &mut hasher);
    app.show_skill_autocomplete.hash(&mut hasher);
    app.skill_autocomplete_selected.hash(&mut hasher);
    hash_debug(&app.skill_suggestions, &mut hasher);
    app.close_in_progress.hash(&mut hasher);
    app.close_timeout_reported.hash(&mut hasher);
    app.should_exit.hash(&mut hasher);
    hasher.finish()
}

fn hash_debug<T: Debug>(value: &T, hasher: &mut DefaultHasher) {
    format!("{value:?}").hash(hasher);
}

#[cfg(test)]
mod tests {
    use super::{enter_primary_screen_interaction, leave_primary_screen_interaction};
    use crate::utils::line_plain_text;
    use ratatui::layout::Size;
    use ratatui::text::Line;
    use ratatui::{backend::TestBackend, widgets::Paragraph, Terminal, TerminalOptions, Viewport};
    use std::io;

    struct RecordingTerminal {
        size: Size,
        inserted: Vec<String>,
        draw_count: usize,
        fail_insert: bool,
    }

    impl RecordingTerminal {
        fn new(width: u16, height: u16) -> Self {
            Self {
                size: Size { width, height },
                inserted: Vec::new(),
                draw_count: 0,
                fail_insert: false,
            }
        }

        fn output(&self) -> String {
            self.inserted.join("\n")
        }
    }

    impl super::TerminalTranscriptTarget for RecordingTerminal {
        fn size(&self) -> io::Result<Size> {
            Ok(self.size)
        }

        fn draw_app(&mut self, _app: &mut crate::TuiApp) -> io::Result<()> {
            self.draw_count += 1;
            Ok(())
        }

        fn insert_history_lines(&mut self, lines: &[Line<'static>]) -> io::Result<()> {
            if self.fail_insert {
                return Err(io::Error::other("forced insert history failure"));
            }
            self.inserted.extend(lines.iter().map(line_plain_text));
            Ok(())
        }
    }

    #[test]
    fn reset_primary_screen_viewport_clears_current_screen_without_alt_screen() {
        let mut output = Vec::new();

        super::reset_primary_screen_viewport(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("\u{1b}[2J"));
        assert!(output.contains("\u{1b}[3J"));
        assert!(output.contains("\u{1b}[1;1H"));
        assert!(!output.contains("\u{1b}[?1049h"));
        assert!(!output.contains("\u{1b}[?1049l"));
    }

    #[test]
    fn primary_screen_interaction_never_emits_alternate_screen_sequences() {
        let mut output = Vec::new();

        enter_primary_screen_interaction(&mut output).unwrap();
        leave_primary_screen_interaction(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(!output.contains("\u{1b}[?1049h"));
        assert!(!output.contains("\u{1b}[?1049l"));
    }

    #[test]
    fn flush_terminal_transcript_writes_pending_transcript_once() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .unwrap();
        let mut app = crate::TuiApp::build(None);
        app.render_runtime.append_user_input("persisted");
        append_test_final_answer(&mut app, "done");

        let flushed = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();
        let skipped = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("live"), frame.area()))
            .unwrap();

        assert!(flushed);
        assert!(!skipped);
        assert!(app
            .render_runtime
            .store()
            .pending_terminal_projection()
            .is_empty());
        let output = format!("{}", terminal.backend());
        assert!(output.contains("● persisted"));
        assert!(output.contains("● done"));
        assert!(output.contains("✻ Brewed for a moment"));
        assert!(output.contains("live"));
    }

    #[test]
    fn terminal_transcript_projection_preserves_spacing_and_noninteractive_footer() {
        let mut app = crate::TuiApp::build(None);
        app.render_runtime.append_user_input("persisted");
        append_test_final_answer(&mut app, "done");

        let pending = app.render_runtime.pending_terminal_projection();
        let lines = super::render_terminal_transcript_lines(&app, &pending, 80)
            .iter()
            .map(crate::utils::line_plain_text)
            .collect::<Vec<_>>();

        assert_eq!(
            lines,
            vec![
                "● persisted".to_string(),
                String::new(),
                "● done".to_string(),
                String::new(),
                "✻ Brewed for a moment".to_string(),
            ]
        );
    }

    #[test]
    fn terminal_transcript_projection_separates_synced_history_from_next_round() {
        let mut app = crate::TuiApp::build(None);
        app.sync_session_header();
        let header_ids = app
            .render_runtime
            .pending_terminal_projection()
            .into_iter()
            .map(|block| block.block_id)
            .collect::<Vec<_>>();
        app.render_runtime.mark_terminal_blocks_synced(header_ids);
        app.rendered_chat_cache_key = None;
        app.render_runtime.append_user_input("next question");
        append_test_final_answer(&mut app, "next answer");

        let pending = app.render_runtime.pending_terminal_projection();
        let lines = super::render_terminal_transcript_lines(&app, &pending, 80)
            .iter()
            .map(crate::utils::line_plain_text)
            .collect::<Vec<_>>();

        assert_eq!(lines.first().map(String::as_str), Some(""));
        assert_eq!(lines.get(1).map(String::as_str), Some("● next question"));
    }

    #[test]
    fn terminal_transcript_projection_renders_thinking_before_final_answer() {
        let mut app = crate::TuiApp::build(None);
        app.render_runtime.append_user_input("analyze");
        app.render_runtime.append_history([
            crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
                crate::render_runtime::transcript::block::TranscriptBlockKind::FinalAnswer,
                vec![crate::render_runtime::common::style::StyledLine::plain(
                    "final response",
                )],
            ),
            crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
                crate::render_runtime::transcript::block::TranscriptBlockKind::Thinking,
                vec![crate::render_runtime::common::style::StyledLine::plain(
                    "reasoning preview",
                )],
            ),
        ]);

        let pending = app.render_runtime.pending_terminal_projection();
        let lines = super::render_terminal_transcript_lines(&app, &pending, 80)
            .iter()
            .map(crate::utils::line_plain_text)
            .collect::<Vec<_>>();
        let thinking_index = lines
            .iter()
            .position(|line| line.contains("Thinking: reasoning preview"))
            .expect("thinking should render");
        let answer_index = lines
            .iter()
            .position(|line| line == "● final response")
            .expect("final answer should render");

        assert!(thinking_index < answer_index);
    }

    #[test]
    fn flush_terminal_transcript_writes_command_and_notice_once() {
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .unwrap();
        let mut app = crate::TuiApp::build(None);
        app.should_exit = true;
        app.render_runtime.append_command_input("/quit");
        app.render_runtime
            .append_system_notice("Shutting down TUI. Closed session `sess-test`.");

        let flushed = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();

        assert!(flushed);
        let output = format!("{}", terminal.backend());
        assert_eq!(output.matches("/quit").count(), 1);
        assert_eq!(output.matches("Shutting down TUI").count(), 1);
        assert!(!output.contains("● /quit"));
    }

    #[test]
    fn flush_terminal_transcript_writes_stable_blocks_while_turn_is_in_flight() {
        let mut terminal = RecordingTerminal::new(20, 3);
        let mut app = crate::TuiApp::build(None);
        app.in_flight = true;
        app.render_runtime.append_user_input("pending");

        let flushed = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();

        assert!(flushed);
        assert!(app
            .render_runtime
            .store()
            .pending_terminal_projection()
            .is_empty());
        let output = terminal.output();
        assert!(output.contains("● pending"));
        assert!(!output.contains("Brewed"));

        app.in_flight = false;
        append_test_final_answer(&mut app, "done");
        let flushed_answer = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();

        assert!(flushed_answer);
        assert!(app
            .render_runtime
            .store()
            .pending_terminal_projection()
            .is_empty());
        let output = terminal.output();
        assert!(output.contains("● done"));
        assert_eq!(output.matches("✻ Brewed").count(), 1);
    }

    #[test]
    fn flush_terminal_transcript_keeps_pending_blocks_when_insert_fails() {
        let mut terminal = RecordingTerminal::new(20, 3);
        terminal.fail_insert = true;
        let mut app = crate::TuiApp::build(None);
        app.render_runtime.append_user_input("persisted");
        append_test_final_answer(&mut app, "done");

        let result = super::flush_terminal_transcript(&mut terminal, &mut app);

        assert!(result.is_err());
        let pending = app.render_runtime.pending_terminal_projection();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn flush_terminal_transcript_defers_single_pending_user_block() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .unwrap();
        let mut app = crate::TuiApp::build(None);
        app.render_runtime.append_user_input("pending");

        let deferred = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();

        assert!(!deferred);
        assert!(!app
            .render_runtime
            .store()
            .pending_terminal_projection()
            .is_empty());
    }

    #[test]
    fn flush_terminal_transcript_writes_single_pending_user_block_on_exit() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .unwrap();
        let mut app = crate::TuiApp::build(None);
        app.should_exit = true;
        app.render_runtime.append_user_input("/quit");

        let flushed = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();

        assert!(flushed);
        assert!(app
            .render_runtime
            .store()
            .pending_terminal_projection()
            .is_empty());
        assert!(format!("{}", terminal.backend()).contains("● /quit"));
    }

    #[test]
    fn flush_terminal_transcript_defers_single_pending_command_block_on_exit() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .unwrap();
        let mut app = crate::TuiApp::build(None);
        app.should_exit = true;
        app.render_runtime.append_command_input("/quit");

        let flushed = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();

        assert!(!flushed);
        assert!(!app
            .render_runtime
            .store()
            .pending_terminal_projection()
            .is_empty());
        assert!(!format!("{}", terminal.backend()).contains("/quit"));
    }

    #[test]
    fn flush_terminal_transcript_defers_assistant_text_while_turn_metrics_are_live() {
        let mut terminal = RecordingTerminal::new(20, 3);
        let mut app = crate::TuiApp::build(None);
        app.in_flight = true;
        app.turn_metrics_live = true;
        app.render_runtime.append_user_input("pending");
        append_test_final_answer(&mut app, "partial");

        let deferred = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();

        assert!(!deferred);
        assert!(!app
            .render_runtime
            .store()
            .pending_terminal_projection()
            .is_empty());
        app.in_flight = false;
        app.turn_metrics_live = false;
        let flushed = super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();
        assert!(flushed);
        assert!(app
            .render_runtime
            .store()
            .pending_terminal_projection()
            .is_empty());
        let output = terminal.output();
        assert!(output.contains("● pending"));
        assert!(output.contains("● partial"));
        assert_eq!(output.matches("✻ Brewed").count(), 1);
    }

    #[test]
    fn primary_mode_exit_keeps_transcript_and_excludes_transient_ui() {
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .unwrap();
        let mut app = crate::TuiApp::build(None);
        app.input_buffer = "draft input must stay interactive only".to_string();
        app.status_hint = Some((
            "transient status must stay interactive only".to_string(),
            std::time::Instant::now(),
        ));
        app.render_runtime.append_user_input("persisted transcript");
        append_test_final_answer(&mut app, "persisted answer");

        super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();
        let output = format!("{}", terminal.backend());

        assert!(output.contains("persisted transcript"));
        assert!(output.contains("persisted answer"));
        assert!(output.contains("✻ Brewed for a moment"));
        assert!(!output.contains("draft input must stay interactive only"));
        assert!(!output.contains("transient status must stay interactive only"));
    }

    #[test]
    fn flush_terminal_transcript_does_not_print_wide_character_placeholders() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .unwrap();
        let mut app = crate::TuiApp::build(None);
        app.render_runtime.append_user_input("hello");
        append_test_final_answer(&mut app, "你好，世界");

        super::flush_terminal_transcript(&mut terminal, &mut app).unwrap();

        let output = format!("{}", terminal.backend());
        assert!(output.contains("你好，世界"));
        assert!(!output.contains("你 好"));
    }

    fn append_test_final_answer(app: &mut crate::TuiApp, text: &str) {
        app.render_runtime.append_history([
            crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
                crate::render_runtime::transcript::block::TranscriptBlockKind::FinalAnswer,
                vec![crate::render_runtime::common::style::StyledLine::plain(
                    text,
                )],
            ),
        ]);
    }

    #[test]
    fn runtime_frame_gate_skips_idle_draws_and_redraws_after_input_change() {
        let mut app = crate::TuiApp::build(None);

        let first_fingerprint = super::tui_frame_fingerprint(&app);
        assert!(app.render_runtime.should_draw_frame(first_fingerprint));
        assert!(!app.render_runtime.should_draw_frame(first_fingerprint));

        app.input_buffer = "hello".to_string();
        app.cursor_index = app.input_buffer.chars().count();

        let updated_fingerprint = super::tui_frame_fingerprint(&app);
        assert!(app.render_runtime.should_draw_frame(updated_fingerprint));
        assert!(!app.render_runtime.should_draw_frame(updated_fingerprint));
    }

    #[test]
    fn runtime_frame_fingerprint_tracks_live_projection_changes() {
        let mut app = crate::TuiApp::build(None);
        let first_fingerprint = super::tui_frame_fingerprint(&app);

        app.render_runtime.start_live_text(
            "item-1",
            crate::render_runtime::render::live::LiveTextKind::FinalAnswer,
            "streaming",
        );

        assert_ne!(first_fingerprint, super::tui_frame_fingerprint(&app));
    }
}
