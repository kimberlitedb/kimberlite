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
}
