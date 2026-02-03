//! Dashboard request handlers.

use super::router::DashboardState;
use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Sse, sse::Event},
};
use std::time::Duration;
use tokio_stream::{wrappers::IntervalStream, StreamExt};

// ============================================================================
// Dashboard Template
// ============================================================================

/// Coverage statistics for template.
#[derive(Clone, Debug)]
pub struct DashboardStats {
    pub state_coverage: usize,
    pub message_sequences: usize,
    pub fault_combinations: usize,
    pub event_sequences: usize,
    pub total: usize,
}

impl DashboardStats {
    /// Calculates percentage, handling zero total.
    pub fn percentage(&self, value: usize) -> usize {
        if self.total == 0 {
            0
        } else {
            (value * 100) / self.total
        }
    }
}

/// Askama template for the VOPR dashboard.
#[derive(Template)]
#[template(path = "vopr/dashboard.html")]
pub struct DashboardTemplate {
    /// Page title.
    pub title: String,
    /// Coverage statistics.
    pub stats: DashboardStats,
    /// Number of seeds in corpus.
    pub corpus_size: usize,
    /// Top seeds by coverage.
    pub top_seeds: Vec<SeedInfo>,
    /// Build version for cache busting.
    pub v: &'static str,
}

/// Information about a seed in the corpus.
#[derive(Clone, Debug)]
pub struct SeedInfo {
    /// Seed value.
    pub seed: u64,
    /// Unique coverage points discovered by this seed.
    pub coverage_unique: usize,
    /// Number of times this seed has been selected for fuzzing.
    pub selection_count: usize,
    /// Energy value (selection probability).
    pub energy: f64,
}

// ============================================================================
// Request Handlers
// ============================================================================

/// Main dashboard handler.
///
/// Returns the HTML page with current coverage statistics.
pub async fn dashboard(
    State(state): State<DashboardState>,
) -> impl IntoResponse {
    let fuzzer = state.fuzzer.lock().unwrap();

    let coverage_stats = fuzzer.coverage_stats();
    let total = coverage_stats.unique_states
        + coverage_stats.unique_message_sequences
        + coverage_stats.unique_fault_combinations
        + coverage_stats.unique_event_sequences;
    let stats = DashboardStats {
        state_coverage: coverage_stats.unique_states,
        message_sequences: coverage_stats.unique_message_sequences,
        fault_combinations: coverage_stats.unique_fault_combinations,
        event_sequences: coverage_stats.unique_event_sequences,
        total,
    };

    let corpus = fuzzer.corpus();

    // Get top 10 seeds by unique coverage
    let mut top_seeds: Vec<SeedInfo> = corpus
        .iter()
        .map(|s| {
            let total_coverage = s.coverage_snapshot.unique_states
                + s.coverage_snapshot.unique_message_sequences
                + s.coverage_snapshot.unique_fault_combinations
                + s.coverage_snapshot.unique_event_sequences;
            SeedInfo {
                seed: s.seed,
                coverage_unique: total_coverage,
                selection_count: s.selection_count,
                energy: s.energy,
            }
        })
        .collect();

    // Sort by unique coverage (descending)
    top_seeds.sort_by(|a, b| b.coverage_unique.cmp(&a.coverage_unique));
    top_seeds.truncate(10);

    DashboardTemplate {
        title: "VOPR Coverage Dashboard".to_string(),
        stats,
        corpus_size: corpus.len(),
        top_seeds,
        v: env!("CARGO_PKG_VERSION"),
    }
}

/// Server-Sent Events handler for real-time coverage updates.
///
/// Sends coverage statistics every 2 seconds to connected clients.
pub async fn coverage_updates_sse(
    State(state): State<DashboardState>,
) -> impl IntoResponse {
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(2)))
        .map(move |_| {
            let fuzzer = state.fuzzer.lock().unwrap();

            let stats = fuzzer.coverage_stats();
            let corpus_size = fuzzer.corpus_size();

            // Send JSON update via SSE
            let data = serde_json::json!({
                "state_coverage": stats.unique_states,
                "message_sequences": stats.unique_message_sequences,
                "fault_combinations": stats.unique_fault_combinations,
                "event_sequences": stats.unique_event_sequences,
                "corpus_size": corpus_size,
            });

            Event::default()
                .event("coverage-update")
                .data(data.to_string())
        })
        .map(Ok::<_, std::convert::Infallible>);

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage_fuzzer::{CoverageFuzzer, SelectionStrategy};
    use std::sync::{Arc, Mutex};

    #[test]
    fn seed_info_creation() {
        let info = SeedInfo {
            seed: 12345,
            coverage_unique: 100,
            selection_count: 5,
            energy: 0.75,
        };

        assert_eq!(info.seed, 12345);
        assert_eq!(info.coverage_unique, 100);
        assert_eq!(info.selection_count, 5);
        assert!((info.energy - 0.75).abs() < 1e-6);
    }

    #[tokio::test]
    async fn dashboard_handler_renders() {
        let fuzzer = Arc::new(Mutex::new(CoverageFuzzer::new(
            SelectionStrategy::EnergyBased,
        )));

        let state = DashboardState::new(fuzzer);
        let response = dashboard(State(state)).await;

        // Just verify it renders without panicking
        // Full rendering test would require HTML parsing
        let _ = response.into_response();
    }
}
