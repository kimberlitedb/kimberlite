//! VSR liveness invariants (probabilistic checkers).
//!
//! The safety checkers in [`vsr_invariants`](crate::vsr_invariants) catch
//! agreement / prefix / monotonicity violations. They answer "did the
//! system do something it must never do?"
//!
//! This module answers the complementary question: "did the system make
//! progress?" — which TLA+ treats as fairness assumptions (`WF_vars` /
//! `SF_vars`) over `Spec => EventualCommit`. VOPR can't model unbounded
//! fairness, so these checkers sample progress over a fixed window of
//! events and flag windows that show livelock.
//!
//! Spec references:
//! - `specs/tla/VSR.tla::EventualCommit`
//! - `specs/tla/VSR.tla::EventualProgress`
//!
//! Traceability matrix rows 18, 19.
//!
//! ## Design
//!
//! `EventualCommitChecker` slides a 1000-iteration window over (prepared,
//! committed) events. If the window contains any op that was prepared and
//! never committed within the window (and the cluster had quorum the
//! whole time), we flag a liveness violation.
//!
//! `EventualProgressChecker` tracks view-change starts and completions.
//! If a view change starts and 500 iterations later no new view is in
//! normal status, we flag a liveness violation — livelocked view change.
//!
//! These are heuristics, not proofs. False positives are possible under
//! adversarial fault injection; use them as smoke tests that catch
//! regressions, not as ground-truth fairness verification (that's TLA+).

use std::collections::HashMap;

use crate::invariant::InvariantResult;

// ============================================================================
// EventualCommit
// ============================================================================

/// Over a sliding window, every prepared op eventually commits (assuming
/// live quorum). Matches `VSR.tla::EventualCommit` under weak fairness.
#[derive(Debug)]
pub struct EventualCommitChecker {
    /// Window size in iterations. The plan uses 1000.
    window: u64,
    /// Observed prepare events: iteration when op was prepared.
    prepares: HashMap<u64, u64>,
    /// Observed commit events: iteration when op was committed.
    commits: HashMap<u64, u64>,
    /// Current iteration count.
    iteration: u64,
    /// Whether quorum is currently live (caller feeds this signal).
    quorum_live: bool,
    /// Total violations detected.
    violations: u64,
}

impl EventualCommitChecker {
    /// Creates a new checker with the given sliding-window size.
    pub fn new(window: u64) -> Self {
        Self {
            window,
            prepares: HashMap::new(),
            commits: HashMap::new(),
            iteration: 0,
            quorum_live: true,
            violations: 0,
        }
    }

    /// Caller advances the iteration clock by one tick.
    pub fn tick(&mut self) {
        self.iteration = self.iteration.saturating_add(1);
    }

    /// Caller reports whether quorum is currently live (i.e., enough
    /// non-partitioned honest replicas to commit). Without quorum, we
    /// can't expect progress and the invariant is trivially satisfied.
    pub fn set_quorum_live(&mut self, live: bool) {
        self.quorum_live = live;
    }

    /// Record that an op was prepared at the current iteration.
    pub fn on_prepare(&mut self, op: u64) {
        self.prepares.entry(op).or_insert(self.iteration);
    }

    /// Record that an op was committed at the current iteration.
    pub fn on_commit(&mut self, op: u64) {
        self.commits.entry(op).or_insert(self.iteration);
    }

    /// Returns the current violation status. Prepared ops older than
    /// `window` iterations with no commit AND with continuous quorum are
    /// considered violations.
    pub fn check(&mut self) -> InvariantResult {
        if !self.quorum_live {
            return InvariantResult::Ok;
        }
        for (&op, &prepared_at) in &self.prepares {
            let age = self.iteration.saturating_sub(prepared_at);
            if age > self.window && !self.commits.contains_key(&op) {
                self.violations += 1;
                return InvariantResult::Violated {
                    invariant: "EventualCommit".into(),
                    message: format!(
                        "op {} prepared at iteration {} but not committed after {} iterations under live quorum",
                        op, prepared_at, age
                    ),
                    context: vec![
                        ("window".into(), self.window.to_string()),
                        ("current_iteration".into(), self.iteration.to_string()),
                        ("prepared_at".into(), prepared_at.to_string()),
                    ],
                };
            }
        }
        InvariantResult::Ok
    }

    /// Total violations detected in the checker's lifetime.
    pub fn violations(&self) -> u64 {
        self.violations
    }
}

impl Default for EventualCommitChecker {
    fn default() -> Self {
        Self::new(1000)
    }
}

// ============================================================================
// EventualProgress
// ============================================================================

/// Under partial synchrony, view changes eventually complete. Matches
/// `VSR.tla::EventualProgress` under weak fairness.
#[derive(Debug)]
pub struct EventualProgressChecker {
    /// Window size: a view change must complete within this many
    /// iterations once started. Plan uses 500.
    window: u64,
    /// View changes that started but haven't completed: view -> iteration.
    pending_view_changes: HashMap<u64, u64>,
    /// Current iteration count.
    iteration: u64,
    /// Total violations detected.
    violations: u64,
}

impl EventualProgressChecker {
    /// Creates a new checker with the given window.
    pub fn new(window: u64) -> Self {
        Self {
            window,
            pending_view_changes: HashMap::new(),
            iteration: 0,
            violations: 0,
        }
    }

    /// Advances the iteration clock by one tick.
    pub fn tick(&mut self) {
        self.iteration = self.iteration.saturating_add(1);
    }

    /// Record that a replica started a view change to view `v`.
    pub fn on_view_change_start(&mut self, v: u64) {
        self.pending_view_changes.entry(v).or_insert(self.iteration);
    }

    /// Record that view `v` transitioned to Normal status at some replica.
    pub fn on_view_change_complete(&mut self, v: u64) {
        self.pending_view_changes.remove(&v);
    }

    /// Returns the current violation status. Any view change older than
    /// `window` without a completion is flagged.
    pub fn check(&mut self) -> InvariantResult {
        for (&v, &started_at) in &self.pending_view_changes {
            let age = self.iteration.saturating_sub(started_at);
            if age > self.window {
                self.violations += 1;
                return InvariantResult::Violated {
                    invariant: "EventualProgress".into(),
                    message: format!(
                        "view change to view {} started at iteration {} never completed within {} iterations (livelock)",
                        v, started_at, age
                    ),
                    context: vec![
                        ("window".into(), self.window.to_string()),
                        ("current_iteration".into(), self.iteration.to_string()),
                        ("started_at".into(), started_at.to_string()),
                    ],
                };
            }
        }
        InvariantResult::Ok
    }

    /// Total violations detected in the checker's lifetime.
    pub fn violations(&self) -> u64 {
        self.violations
    }
}

impl Default for EventualProgressChecker {
    fn default() -> Self {
        Self::new(500)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eventual_commit_ok_when_commits_arrive_within_window() {
        let mut c = EventualCommitChecker::new(10);
        c.on_prepare(42);
        for _ in 0..5 {
            c.tick();
        }
        c.on_commit(42);
        assert!(c.check().is_ok());
    }

    #[test]
    fn eventual_commit_violated_when_prepare_never_commits() {
        let mut c = EventualCommitChecker::new(10);
        c.on_prepare(42);
        for _ in 0..20 {
            c.tick();
        }
        assert!(!c.check().is_ok());
    }

    #[test]
    fn eventual_commit_ok_under_lost_quorum() {
        let mut c = EventualCommitChecker::new(10);
        c.on_prepare(42);
        c.set_quorum_live(false);
        for _ in 0..20 {
            c.tick();
        }
        // No quorum means no progress expected — must not flag.
        assert!(c.check().is_ok());
    }

    #[test]
    fn eventual_progress_ok_when_view_change_completes() {
        let mut p = EventualProgressChecker::new(10);
        p.on_view_change_start(5);
        for _ in 0..5 {
            p.tick();
        }
        p.on_view_change_complete(5);
        assert!(p.check().is_ok());
    }

    #[test]
    fn eventual_progress_violated_when_view_change_livelocks() {
        let mut p = EventualProgressChecker::new(10);
        p.on_view_change_start(5);
        for _ in 0..20 {
            p.tick();
        }
        let r = p.check();
        assert!(!r.is_ok(), "expected livelock violation, got {:?}", r);
    }
}
