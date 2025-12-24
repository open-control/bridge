//! Terminal UI using ratatui

pub mod theme;
pub mod widgets;

use crate::app::App;
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
        MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    Frame, Terminal,
};
use std::io;
use widgets::{actions::ActionsWidget, log::LogWidget, status::StatusWidget};

/// Run the TUI event loop
pub async fn run(app: &mut App) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    loop {
        // Poll for bridge logs and update state
        app.poll();

        // Draw UI
        terminal.draw(|f| draw(f, app))?;

        // Handle input with timeout (16ms = ~60 FPS)
        if event::poll(std::time::Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        app.quit();
                        break;
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        app.toggle_bridge();
                    }
                    KeyCode::Char('i') | KeyCode::Char('I') => {
                        app.install_service();
                    }
                    KeyCode::Char('u') | KeyCode::Char('U') => {
                        if app.state().service_installed {
                            app.uninstall_service();
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
                        app.scroll_up();
                    }
                    KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
                        app.scroll_down();
                    }
                    KeyCode::PageUp => {
                        for _ in 0..10 {
                            app.scroll_up();
                        }
                    }
                    KeyCode::PageDown => {
                        for _ in 0..10 {
                            app.scroll_down();
                        }
                    }
                    KeyCode::Home => {
                        app.scroll_to_top();
                    }
                    KeyCode::End => {
                        app.scroll_to_bottom();
                    }
                    // Mode toggle
                    KeyCode::Char('m') | KeyCode::Char('M') => {
                        app.toggle_mode();
                    }
                    // Filter shortcuts
                    KeyCode::Char('1') => {
                        app.filter_protocol_only();
                    }
                    KeyCode::Char('2') => {
                        app.filter_debug_only();
                    }
                    KeyCode::Char('3') => {
                        app.filter_show_all();
                    }
                    // Copy logs to clipboard
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        app.copy_logs_to_clipboard();
                    }
                    // Pause toggle
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        app.toggle_pause();
                    }
                    // Export logs
                    KeyCode::Char('o') | KeyCode::Char('O') => {
                        app.export_logs();
                    }
                    // Open config
                    KeyCode::Char('f') | KeyCode::Char('F') => {
                        app.open_config();
                    }
                    // Debug level filters (only when in Debug mode)
                    // Note: D/I/W/E/A only work when filter is "Debug"
                    KeyCode::Char('d') => {
                        if app.filter_name() == "Debug" {
                            app.filter_debug_level(Some(crate::bridge::LogLevel::Debug));
                        }
                    }
                    KeyCode::Char('w') => {
                        if app.filter_name() == "Debug" {
                            app.filter_debug_level(Some(crate::bridge::LogLevel::Warn));
                        }
                    }
                    KeyCode::Char('e') => {
                        if app.filter_name() == "Debug" {
                            app.filter_debug_level(Some(crate::bridge::LogLevel::Error));
                        }
                    }
                    KeyCode::Char('a') => {
                        if app.filter_name() == "Debug" {
                            app.filter_debug_level(None); // All levels
                        }
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        app.scroll_up();
                    }
                    MouseEventKind::ScrollDown => {
                        app.scroll_down();
                    }
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
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(5), // Status widget (reduced from 7)
        Constraint::Min(5),    // Log widget
        Constraint::Length(3), // Actions widget
    ])
    .split(frame.area());

    let state = app.state();
    let filter_name = app.filter_name();

    // Status widget
    let status = StatusWidget::new(&state);
    frame.render_widget(status, chunks[0]);

    // Log widget
    let log = LogWidget::new(
        app.logs(),
        app.filter(),
        app.scroll_position(),
        state.paused,
    );
    frame.render_widget(log, chunks[1]);

    // Actions widget
    let actions = ActionsWidget::new(&state, filter_name);
    frame.render_widget(actions, chunks[2]);
}
