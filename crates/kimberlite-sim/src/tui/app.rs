//! Application state for the TUI.

use crate::vopr::VoprConfig;
use std::sync::{Arc, Mutex};
use std::thread;

/// Tab index for TUI navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabIndex {
    /// Overview tab.
    Overview,
    /// Logs tab.
    Logs,
    /// Config tab.
    Config,
}

impl TabIndex {
    /// Returns the next tab in the cycle.
    pub fn next(self) -> Self {
        match self {
            Self::Overview => Self::Logs,
            Self::Logs => Self::Config,
            Self::Config => Self::Overview,
        }
    }

    /// Returns the previous tab in the cycle.
    pub fn prev(self) -> Self {
        match self {
            Self::Overview => Self::Config,
            Self::Logs => Self::Overview,
            Self::Config => Self::Logs,
        }
    }

    /// Returns the tab index as a usize for backwards compatibility.
    pub fn as_usize(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Logs => 1,
            Self::Config => 2,
        }
    }
}

/// Application state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    /// Idle, waiting to start.
    Idle,
    /// Running simulation.
    Running { iteration: u64, total: u64 },
    /// Paused.
    Paused { iteration: u64 },
    /// Completed.
    Completed { successes: u64, failures: u64 },
}

/// Simulation results for display.
#[derive(Clone, Default)]
pub struct SimulationResults {
    pub iterations: u64,
    pub successes: u64,
    pub failures: u64,
    pub recent_results: Vec<String>,
}

/// Main TUI application.
pub struct App {
    /// VOPR configuration.
    config: VoprConfig,
    /// Current application state.
    state: AppState,
    /// Current tab index.
    current_tab: TabIndex,
    /// Simulation results.
    results: Arc<Mutex<SimulationResults>>,
    /// Log buffer.
    log_buffer: Arc<Mutex<Vec<String>>>,
    /// Simulation thread handle.
    simulation_thread: Option<thread::JoinHandle<()>>,
    /// Scroll offset for logs.
    scroll_offset: usize,
}

impl App {
    /// Creates a new TUI application.
    pub fn new(config: VoprConfig) -> Self {
        Self {
            config,
            state: AppState::Idle,
            current_tab: TabIndex::Overview,
            results: Arc::new(Mutex::new(SimulationResults::default())),
            log_buffer: Arc::new(Mutex::new(Vec::new())),
            simulation_thread: None,
            scroll_offset: 0,
        }
    }

    /// Starts the simulation.
    pub fn start_simulation(&mut self) {
        if matches!(self.state, AppState::Idle | AppState::Completed { .. }) {
            let config = self.config.clone();
            let results = Arc::clone(&self.results);
            let log_buffer = Arc::clone(&self.log_buffer);

            self.state = AppState::Running {
                iteration: 0,
                total: config.iterations,
            };

            // Spawn simulation thread (simplified - just updates counters)
            self.simulation_thread = Some(thread::spawn(move || {
                for i in 0..config.iterations {
                    // Simulate work
                    std::thread::sleep(std::time::Duration::from_millis(10));

                    // Update results
                    let mut res = results.lock().unwrap();
                    res.iterations = i + 1;
                    res.successes += 1; // Simplified

                    // Log progress
                    let mut logs = log_buffer.lock().unwrap();
                    logs.push(format!("Iteration {} completed", i + 1));
                    if logs.len() > 1000 {
                        logs.remove(0);
                    }
                }
            }));
        }
    }

    /// Toggles pause/resume.
    pub fn toggle_pause(&mut self) {
        match self.state {
            AppState::Running {
                iteration,
                total: _,
            } => {
                self.state = AppState::Paused { iteration };
            }
            AppState::Paused { iteration } => {
                self.state = AppState::Running {
                    iteration,
                    total: self.config.iterations,
                };
            }
            _ => {}
        }
    }

    /// Updates application state (called each tick).
    pub fn tick(&mut self) {
        // Check if simulation thread finished
        if let Some(handle) = &self.simulation_thread {
            if handle.is_finished() {
                let results = self.results.lock().unwrap();
                self.state = AppState::Completed {
                    successes: results.successes,
                    failures: results.failures,
                };
                self.simulation_thread = None;
            }
        }
    }

    /// Moves to next tab.
    pub fn next_tab(&mut self) {
        self.current_tab = self.current_tab.next();
    }

    /// Moves to previous tab.
    pub fn prev_tab(&mut self) {
        self.current_tab = self.current_tab.prev();
    }

    /// Scrolls up in logs.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Scrolls down in logs.
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    // Getters
    pub fn state(&self) -> AppState {
        self.state
    }

    /// Returns the current tab index.
    pub fn current_tab(&self) -> TabIndex {
        self.current_tab
    }

    /// Returns the current tab as a usize (for backwards compatibility).
    pub fn current_tab_index(&self) -> usize {
        self.current_tab.as_usize()
    }

    pub fn results(&self) -> SimulationResults {
        self.results.lock().unwrap().clone()
    }

    pub fn logs(&self) -> Vec<String> {
        self.log_buffer.lock().unwrap().clone()
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn config(&self) -> &VoprConfig {
        &self.config
    }
}
