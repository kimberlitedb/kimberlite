//! UI rendering for the TUI.

use super::app::{App, AppState};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Tabs},
};

/// Draws the entire UI.
pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Content
            Constraint::Length(3), // Status
        ])
        .split(f.size());

    draw_header(f, chunks[0], app);
    draw_content(f, chunks[1], app);
    draw_status(f, chunks[2], app);
}

fn draw_header(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let tabs = Tabs::new(vec!["Overview", "Logs", "Config"])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("VOPR Simulation"),
        )
        .select(app.current_tab_index())
        .style(Style::default().fg(Color::Cyan))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, area);
}

fn draw_content(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    use crate::tui::app::TabIndex;

    match app.current_tab() {
        TabIndex::Overview => draw_overview(f, area, app),
        TabIndex::Logs => draw_logs(f, area, app),
        TabIndex::Config => draw_config(f, area, app),
    }
}

fn draw_overview(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // Progress
            Constraint::Length(7), // Stats
            Constraint::Min(0),    // Recent results
        ])
        .split(area);

    // Progress gauge
    let (progress, label) = match app.state() {
        AppState::Running { iteration, total } => {
            let pct = ((iteration as f64 / total as f64) * 100.0) as u16;
            (pct, format!("{}/{} iterations", iteration, total))
        }
        AppState::Paused { iteration } => (0, format!("Paused at iteration {}", iteration)),
        AppState::Completed { .. } => (100, "Completed".to_string()),
        AppState::Idle => (0, "Press 's' to start".to_string()),
    };

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progress"))
        .gauge_style(Style::default().fg(Color::Green))
        .label(label)
        .percent(progress);

    f.render_widget(gauge, chunks[0]);

    // Stats
    let results = app.results();
    let stats_text = vec![
        Line::from(vec![
            Span::raw("Iterations: "),
            Span::styled(
                results.iterations.to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::raw("Successes:  "),
            Span::styled(
                results.successes.to_string(),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::raw("Failures:   "),
            Span::styled(
                results.failures.to_string(),
                Style::default().fg(Color::Red),
            ),
        ]),
    ];

    let stats = Paragraph::new(stats_text)
        .block(Block::default().borders(Borders::ALL).title("Statistics"));

    f.render_widget(stats, chunks[1]);

    // Recent results
    let items: Vec<ListItem> = results
        .recent_results
        .iter()
        .rev()
        .take(chunks[2].height.saturating_sub(2) as usize)
        .map(|r| ListItem::new(r.as_str()))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Recent Results"),
    );

    f.render_widget(list, chunks[2]);
}

fn draw_logs(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let logs = app.logs();
    let offset = app.scroll_offset();

    let items: Vec<ListItem> = logs
        .iter()
        .skip(offset)
        .take(area.height.saturating_sub(2) as usize)
        .map(|log| ListItem::new(log.as_str()))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Logs (scroll: {})", offset)),
    );

    f.render_widget(list, area);
}

fn draw_config(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let config = app.config();

    let text = vec![
        Line::from(vec![Span::raw(format!("Seed: {}", config.seed))]),
        Line::from(vec![Span::raw(format!(
            "Iterations: {}",
            config.iterations
        ))]),
        Line::from(vec![Span::raw(format!(
            "Scenario: {}",
            config
                .scenario
                .as_ref()
                .map(|s| format!("{:?}", s))
                .unwrap_or_else(|| "All".to_string())
        ))]),
    ];

    let paragraph = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Configuration"),
    );

    f.render_widget(paragraph, area);
}

fn draw_status(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let status_text = match app.state() {
        AppState::Idle => "Press 's' to start | 'q' to quit | Tab to switch tabs",
        AppState::Running { .. } => "Space to pause | 'q' to quit | Tab to switch tabs",
        AppState::Paused { .. } => "Space to resume | 'q' to quit | Tab to switch tabs",
        AppState::Completed { .. } => "Press 's' to restart | 'q' to quit",
    };

    let paragraph = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}
