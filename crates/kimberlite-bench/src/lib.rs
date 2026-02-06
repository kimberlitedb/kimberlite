//! # kmb-bench: Performance benchmarks and load tests for Kimberlite
//!
//! This crate provides comprehensive performance benchmarking and load testing
//! infrastructure for Kimberlite components.

// Benchmark code intentionally uses patterns that trigger some clippy lints
#![allow(clippy::cast_precision_loss)] // Latency stats use f64 for percentile calculations
#![allow(clippy::cast_sign_loss)] // Benchmark counts and sizes use unsigned arithmetic
#![allow(clippy::cast_possible_truncation)] // Benchmark conversions between numeric types
//!
//! ## Benchmarks
//!
//! - **crypto**: Cryptographic operations (hash, encrypt, sign)
//! - **storage**: Storage layer (write, read, fsync)
//! - **kernel**: State machine transitions
//! - **wire**: Protocol serialization/deserialization
//! - **`end_to_end`**: Full system throughput
//!
//! ## Running Benchmarks
//!
//! ```bash
//! # Run all benchmarks
//! cargo bench -p kmb-bench
//!
//! # Run specific benchmark
//! cargo bench -p kmb-bench crypto
//!
//! # Save baseline for comparison
//! cargo bench -p kmb-bench --bench crypto -- --save-baseline main
//!
//! # Compare against baseline
//! cargo bench -p kmb-bench --bench crypto -- --baseline main
//! ```

use std::io::Write;
use std::time::Instant;

use hdrhistogram::Histogram;

/// Tracks latency percentiles for operations.
#[derive(Debug)]
pub struct LatencyTracker {
    histogram: Histogram<u64>,
}

impl LatencyTracker {
    /// Creates a new latency tracker.
    ///
    /// Tracks latencies from 1ns to 1 hour with 3 significant digits.
    pub fn new() -> Self {
        Self {
            histogram: Histogram::new(3).expect("valid histogram config"),
        }
    }

    /// Records a latency measurement in nanoseconds.
    pub fn record(&mut self, latency_ns: u64) {
        self.histogram.record(latency_ns).ok();
    }

    /// Returns the total number of recorded samples.
    pub fn count(&self) -> u64 {
        self.histogram.len()
    }

    /// Returns the p50 (median) latency in nanoseconds.
    pub fn p50(&self) -> u64 {
        self.histogram.value_at_quantile(0.50)
    }

    /// Returns the p95 latency in nanoseconds.
    pub fn p95(&self) -> u64 {
        self.histogram.value_at_quantile(0.95)
    }

    /// Returns the p99 latency in nanoseconds.
    pub fn p99(&self) -> u64 {
        self.histogram.value_at_quantile(0.99)
    }

    /// Returns the p99.9 latency in nanoseconds.
    pub fn p999(&self) -> u64 {
        self.histogram.value_at_quantile(0.999)
    }

    /// Returns the maximum latency in nanoseconds.
    pub fn max(&self) -> u64 {
        self.histogram.max()
    }

    /// Returns the mean latency in nanoseconds.
    pub fn mean(&self) -> f64 {
        self.histogram.mean()
    }

    /// Returns the minimum latency in nanoseconds.
    pub fn min(&self) -> u64 {
        self.histogram.min()
    }

    /// Exports the latency distribution as eCDF CSV.
    ///
    /// Format: `latency_ns,percentile`
    /// Useful for plotting latency distributions and tracking trends across runs.
    pub fn export_ecdf_csv<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writeln!(writer, "latency_ns,percentile")?;
        for v in self.histogram.iter_quantiles(1) {
            writeln!(
                writer,
                "{},{}",
                v.value_iterated_to(),
                v.percentile() / 100.0
            )?;
        }
        Ok(())
    }

    /// Exports latency statistics as JSON for CI integration.
    ///
    /// Includes all percentiles, count, min, max, and mean.
    pub fn to_json(&self, operation: &str) -> String {
        serde_json::json!({
            "operation": operation,
            "count": self.count(),
            "min_ns": self.min(),
            "p50_ns": self.p50(),
            "p95_ns": self.p95(),
            "p99_ns": self.p99(),
            "p999_ns": self.p999(),
            "max_ns": self.max(),
            "mean_ns": self.mean(),
        })
        .to_string()
    }

    /// Prints a summary of latency statistics.
    pub fn print_summary(&self, operation: &str) {
        println!("{operation} Latency Statistics:");
        println!(
            "  p50:   {:>10} ns ({:>8.2} μs)",
            self.p50(),
            self.p50() as f64 / 1000.0
        );
        println!(
            "  p95:   {:>10} ns ({:>8.2} μs)",
            self.p95(),
            self.p95() as f64 / 1000.0
        );
        println!(
            "  p99:   {:>10} ns ({:>8.2} μs)",
            self.p99(),
            self.p99() as f64 / 1000.0
        );
        println!(
            "  p99.9: {:>10} ns ({:>8.2} μs)",
            self.p999(),
            self.p999() as f64 / 1000.0
        );
        println!(
            "  max:   {:>10} ns ({:>8.2} μs)",
            self.max(),
            self.max() as f64 / 1000.0
        );
        println!(
            "  mean:  {:>10.0} ns ({:>8.2} μs)",
            self.mean(),
            self.mean() / 1000.0
        );
    }
}

impl Default for LatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Open-loop latency tracker that accounts for coordinated omission.
///
/// Unlike closed-loop benchmarks (which wait for each response before sending the
/// next request), this tracker sends at fixed intervals and records the *intended*
/// start time. This captures queuing delay that closed-loop benchmarks miss.
///
/// Per Gil Tene's "How NOT to Measure Latency" and latency.md: coordinated omission
/// causes closed-loop benchmarks to underreport tail latency by orders of magnitude.
#[derive(Debug)]
pub struct OpenLoopTracker {
    tracker: LatencyTracker,
    interval_ns: u64,
    next_send_time: Instant,
}

impl OpenLoopTracker {
    /// Creates a new open-loop tracker with the given target send interval.
    ///
    /// `interval_ns` is the desired time between sends (e.g., `1_000_000` for 1ms = 1K req/s).
    pub fn new(interval_ns: u64) -> Self {
        Self {
            tracker: LatencyTracker::new(),
            interval_ns,
            next_send_time: Instant::now(),
        }
    }

    /// Returns the intended start time for the next operation.
    ///
    /// The caller should record this *before* performing the operation,
    /// then call `record_completion` with this timestamp when done.
    pub fn intended_start(&mut self) -> Instant {
        let start = self.next_send_time;
        self.next_send_time += std::time::Duration::from_nanos(self.interval_ns);
        start
    }

    /// Records the completion of an operation started at `intended_start`.
    ///
    /// The latency includes any queuing delay from the operation not being
    /// sent at its intended time (which is the coordinated omission correction).
    pub fn record_completion(&mut self, intended_start: Instant) {
        let elapsed = intended_start.elapsed();
        self.tracker.record(elapsed.as_nanos() as u64);
    }

    /// Returns the underlying latency tracker for accessing statistics.
    pub fn tracker(&self) -> &LatencyTracker {
        &self.tracker
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_tracker() {
        let mut tracker = LatencyTracker::new();

        // Record some latencies
        for i in 1..=100 {
            tracker.record(i * 1000); // 1μs to 100μs
        }

        // Verify percentiles are in expected ranges
        assert!(tracker.p50() > 0);
        assert!(tracker.p99() > tracker.p50());
        assert!(tracker.p999() > tracker.p99());
        assert!(tracker.max() >= tracker.p999());
    }

    #[test]
    fn test_latency_statistics() {
        let mut tracker = LatencyTracker::new();

        // Record known values
        tracker.record(1000); // 1μs
        tracker.record(2000); // 2μs
        tracker.record(3000); // 3μs
        tracker.record(10000); // 10μs (outlier)

        // Mean should be around 4μs
        assert!((tracker.mean() - 4000.0).abs() < 500.0);

        // Max should be the outlier
        assert!(tracker.max() >= 10000);
    }

    #[test]
    fn test_ecdf_csv_export() {
        let mut tracker = LatencyTracker::new();
        for i in 1..=1000 {
            tracker.record(i * 1000);
        }

        let mut buf = Vec::new();
        tracker.export_ecdf_csv(&mut buf).unwrap();

        let csv = String::from_utf8(buf).unwrap();
        assert!(csv.starts_with("latency_ns,percentile\n"));
        // Header + at least some quantile rows
        assert!(csv.lines().count() > 2);
    }

    #[test]
    fn test_json_export() {
        let mut tracker = LatencyTracker::new();
        tracker.record(1000);
        tracker.record(5000);
        tracker.record(10000);

        let json = tracker.to_json("test_op");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["operation"], "test_op");
        assert_eq!(parsed["count"], 3);
        assert!(parsed["p50_ns"].as_u64().unwrap() > 0);
        assert!(parsed["max_ns"].as_u64().unwrap() >= 10000);
    }

    #[test]
    fn test_open_loop_tracker() {
        let mut tracker = OpenLoopTracker::new(1_000_000); // 1ms intervals

        for _ in 0..10 {
            let start = tracker.intended_start();
            // Simulate some work
            std::thread::sleep(std::time::Duration::from_millis(1));
            tracker.record_completion(start);
        }

        assert_eq!(tracker.tracker().count(), 10);
        // Latencies should be at least 1ms = 1_000_000ns
        assert!(tracker.tracker().max() >= 1_000_000);
    }
}
