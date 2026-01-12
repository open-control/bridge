//! Terminal UI using ratatui
//!
//! Thin layer responsible only for terminal I/O. All business logic
//! is delegated to App via handle_key() and handle_scroll().

pub mod theme;
pub mod widgets;

use crate::app::App;
use crate::constants::FRAME_DURATION_MS;
use crate::error::{BridgeError, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    Frame, Terminal,
};
use std::io;
use widgets::{actions::ActionsWidget, log::LogWidget, mode::ModePopup, status::StatusWidget};

/// Map io::Error to BridgeError::Runtime
fn map_io_err(e: io::Error) -> BridgeError {
    BridgeError::Runtime { source: e }
}

/// Run the TUI event loop
pub async fn run(app: &mut App) -> Result<()> {
    // Setup terminal
    enable_raw_mode().map_err(map_io_err)?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(map_io_err)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(map_io_err)?;

    // Main loop
    loop {
        // Poll for bridge logs and update state
        app.poll();

        // Draw UI
        terminal.draw(|f| draw(f, app)).map_err(map_io_err)?;

        // Handle input with timeout
        if event::poll(std::time::Duration::from_millis(FRAME_DURATION_MS)).map_err(map_io_err)? {
            match event::read().map_err(map_io_err)? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if app.handle_key(key) {
                        break;
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => app.handle_scroll(true),
                    MouseEventKind::ScrollDown => app.handle_scroll(false),
                    _ => {}
                },
                _ => {}
            }
        }

        if app.should_quit() {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode().map_err(map_io_err)?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .map_err(map_io_err)?;
    terminal.show_cursor().map_err(map_io_err)?;

    Ok(())
}

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let is_wide = area.width > 80;

    // Status widget height depends on layout:
    // - Wide: border(2) + header(1) + boxes side-by-side(4) = 7
    // - Narrow: border(2) + header(1) + 2 stacked boxes(3+3) = 9
    let status_height = if is_wide { 7 } else { 9 };

    let chunks = Layout::vertical([
        Constraint::Length(status_height), // Status widget (responsive)
        Constraint::Min(5),                // Log widget
        Constraint::Length(3),             // Actions widget
    ])
    .split(area);

    let state = app.state();
    let filter_mode = app.filter_mode();

    // Status widget
    let status = StatusWidget::new(&state);
    frame.render_widget(status, chunks[0]);

    // Log widget
    let log = LogWidget::new(
        app.logs(),
        app.filter(),
        filter_mode,
        app.scroll_position(),
        state.paused,
    );
    frame.render_widget(log, chunks[1]);

    // Actions widget
    let actions = ActionsWidget::new(&state);
    frame.render_widget(actions, chunks[2]);

    // Mode settings popup (rendered on top)
    if let Some(settings) = app.mode_settings() {
        let popup = ModePopup::new(settings);
        frame.render_widget(popup, frame.area());
    }
}
