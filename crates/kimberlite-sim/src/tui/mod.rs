//! Terminal UI for interactive VOPR simulation.
//!
//! Provides a rich terminal interface using ratatui for running simulations,
//! viewing progress, and exploring results in real-time.

#[cfg(feature = "tui")]
pub mod app;
#[cfg(feature = "tui")]
pub mod ui;

#[cfg(feature = "tui")]
pub use app::{App, AppState};

#[cfg(feature = "tui")]
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
#[cfg(feature = "tui")]
use ratatui::{backend::CrosstermBackend, Terminal};
#[cfg(feature = "tui")]
use std::io;

/// Runs the TUI application.
#[cfg(feature = "tui")]
pub fn run_tui(config: crate::vopr::VoprConfig) -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(config);

    // Run app loop
    let result = run_app_loop(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

#[cfg(feature = "tui")]
fn run_app_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    // EXPLICIT ITERATION LIMIT: prevent infinite loop
    const MAX_ITERATIONS: usize = 10_000_000; // ~277 hours at 100ms poll
    let mut iterations = 0;

    loop {
        // Check iteration bound
        assert!(
            iterations < MAX_ITERATIONS,
            "TUI event loop exceeded maximum iterations"
        );
        iterations += 1;

        terminal.draw(|f| ui::draw(f, app))?;

        // Handle input
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char(' ') => app.toggle_pause(),
                    KeyCode::Char('s') => app.start_simulation(),
                    KeyCode::Up => app.scroll_up(),
                    KeyCode::Down => app.scroll_down(),
                    KeyCode::Tab => app.next_tab(),
                    _ => {}
                }
            }
        }

        // Update app state
        app.tick();
    }
}

// Stub implementations when TUI feature is disabled
#[cfg(not(feature = "tui"))]
use std::io;

#[cfg(not(feature = "tui"))]
pub fn run_tui(_config: crate::vopr::VoprConfig) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "TUI feature not enabled. Rebuild with --features tui",
    ))
}
