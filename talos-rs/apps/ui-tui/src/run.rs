//! TUI event loop and top-level entry points.

use std::{error::Error, io, sync::Arc};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    Terminal,
};
use talos_client::TalosClient;

use super::TuiApp;

/// Run the TUI connected to a remote gateway via `client`.
pub async fn run_tui_app_with_client(client: Arc<TalosClient>) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut app = TuiApp::new_with_client(client);
    app.initialize_remote().await;
    let run_result = run_event_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    run_result
}

async fn run_event_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut TuiApp,
) -> Result<(), Box<dyn Error>> {
    loop {
        app.on_tick().await;
        if app.should_exit {
            return Ok(());
        }
        terminal.draw(|frame| app.render_ui(frame))?;

        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        if app.handle_input(key).await? {
                            return Ok(());
                        }
                    }
                }
                Event::Mouse(mouse) => app.handle_mouse(mouse),
                _ => {}
            }
        }
    }
}
