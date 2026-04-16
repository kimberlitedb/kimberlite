//! Thread-local property registry for simulation tracking.
//!
//! Records every evaluation of ALWAYS/SOMETIMES/NEVER/REACHED properties
//! and provides snapshots for VOPR reporting and coverage analysis.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ============================================================================
// Property Record
// ============================================================================

/// Tracking data for a single property.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PropertyRecord {
    /// Property identifier (e.g., "vsr.offset_monotonicity")
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// Property kind
    pub kind: PropertyKind,
    /// Number of times this property was evaluated
    pub evaluations: u64,
    /// Number of times the property was violated (ALWAYS: false, NEVER: true)
    pub violations: u64,
    /// Whether the property was ever satisfied (for SOMETIMES/REACHED)
    pub satisfied: bool,
}

/// The kind of temporal property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PropertyKind {
    /// Must be true on every evaluation
    Always,
    /// Must be true at least once per run
    Sometimes,
    /// Must never be true
    Never,
    /// Code path must be reached at least once
    Reached,
    /// Code path must never be reached
    Unreachable,
}

impl std::fmt::Display for PropertyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Always => write!(f, "ALWAYS"),
            Self::Sometimes => write!(f, "SOMETIMES"),
            Self::Never => write!(f, "NEVER"),
            Self::Reached => write!(f, "REACHED"),
            Self::Unreachable => write!(f, "UNREACHABLE"),
        }
    }
}

// ============================================================================
// Thread-Local Registry
// ============================================================================

thread_local! {
    static REGISTRY: RefCell<HashMap<String, PropertyRecord>> = RefCell::new(HashMap::new());
}

/// Returns an existing property record, or creates one with default values.
pub fn ensure_entry(id: &str, kind: PropertyKind, description: &str) -> PropertyRecord {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        reg.entry(id.to_string())
            .or_insert_with(|| PropertyRecord {
                id: id.to_string(),
                description: description.to_string(),
                kind,
                evaluations: 0,
                violations: 0,
                satisfied: false,
            })
            .clone()
    })
}

// ============================================================================
// Recording Functions (called by macros)
// ============================================================================

/// Record an ALWAYS property evaluation.
pub fn record_always(id: &str, condition: bool, description: &str) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let entry = reg.entry(id.to_string()).or_insert_with(|| PropertyRecord {
            id: id.to_string(),
            description: description.to_string(),
            kind: PropertyKind::Always,
            evaluations: 0,
            violations: 0,
            satisfied: false,
        });
        entry.evaluations += 1;
        if !condition {
            entry.violations += 1;
        }
    });
}

/// Record a SOMETIMES property evaluation.
pub fn record_sometimes(id: &str, condition: bool, description: &str) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let entry = reg.entry(id.to_string()).or_insert_with(|| PropertyRecord {
            id: id.to_string(),
            description: description.to_string(),
            kind: PropertyKind::Sometimes,
            evaluations: 0,
            violations: 0,
            satisfied: false,
        });
        entry.evaluations += 1;
        if condition {
            entry.satisfied = true;
        }
    });
}

/// Record a NEVER property evaluation.
pub fn record_never(id: &str, condition: bool, description: &str) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let entry = reg.entry(id.to_string()).or_insert_with(|| PropertyRecord {
            id: id.to_string(),
            description: description.to_string(),
            kind: PropertyKind::Never,
            evaluations: 0,
            violations: 0,
            satisfied: false,
        });
        entry.evaluations += 1;
        if condition {
            entry.violations += 1;
        }
    });
}

/// Record that a code path was reached.
pub fn record_reached(id: &str, description: &str) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let entry = reg.entry(id.to_string()).or_insert_with(|| PropertyRecord {
            id: id.to_string(),
            description: description.to_string(),
            kind: PropertyKind::Reached,
            evaluations: 0,
            violations: 0,
            satisfied: false,
        });
        entry.evaluations += 1;
        entry.satisfied = true;
    });
}

/// Record that an unreachable code path was reached (this is a violation).
pub fn record_unreachable(id: &str, description: &str) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let entry = reg.entry(id.to_string()).or_insert_with(|| PropertyRecord {
            id: id.to_string(),
            description: description.to_string(),
            kind: PropertyKind::Unreachable,
            evaluations: 0,
            violations: 0,
            satisfied: false,
        });
        entry.evaluations += 1;
        entry.violations += 1;
    });
}

// ============================================================================
// Snapshot & Reset
// ============================================================================

/// Returns a snapshot of all registered properties and their tracking data.
pub fn snapshot() -> HashMap<String, PropertyRecord> {
    REGISTRY.with(|reg| reg.borrow().clone())
}

/// Resets all property tracking data. Call between simulation runs.
pub fn reset() {
    REGISTRY.with(|reg| reg.borrow_mut().clear());
}

/// Returns a summary report of all properties.
///
/// Format:
/// ```text
/// ALWAYS  [  OK ] offset_monotonicity (1234 evals, 0 violations)
/// SOMETIMES [ MISS ] view_change_with_crash (500 evals, never satisfied)
/// NEVER  [  OK ] dual_leader (1234 evals, 0 violations)
/// REACHED [ HIT  ] recovery_from_corrupt_log (3 hits)
/// ```
pub fn summary_report() -> String {
    let snap = snapshot();
    let mut lines: Vec<String> = Vec::new();

    // Sort by kind, then by id
    let mut entries: Vec<_> = snap.values().collect();
    entries.sort_by(|a, b| {
        let kind_order = |k: &PropertyKind| match k {
            PropertyKind::Always => 0,
            PropertyKind::Sometimes => 1,
            PropertyKind::Never => 2,
            PropertyKind::Reached => 3,
            PropertyKind::Unreachable => 4,
        };
        kind_order(&a.kind)
            .cmp(&kind_order(&b.kind))
            .then(a.id.cmp(&b.id))
    });

    for entry in entries {
        let status = match entry.kind {
            PropertyKind::Always | PropertyKind::Never => {
                if entry.violations == 0 {
                    "  OK "
                } else {
                    " FAIL"
                }
            }
            PropertyKind::Sometimes | PropertyKind::Reached => {
                if entry.satisfied {
                    " HIT "
                } else {
                    " MISS"
                }
            }
            PropertyKind::Unreachable => {
                if entry.violations == 0 {
                    "  OK "
                } else {
                    " FAIL"
                }
            }
        };

        let detail = match entry.kind {
            PropertyKind::Always | PropertyKind::Never | PropertyKind::Unreachable => {
                format!("{} evals, {} violations", entry.evaluations, entry.violations)
            }
            PropertyKind::Sometimes => {
                if entry.satisfied {
                    format!("{} evals, satisfied", entry.evaluations)
                } else {
                    format!("{} evals, never satisfied", entry.evaluations)
                }
            }
            PropertyKind::Reached => {
                format!("{} hits", entry.evaluations)
            }
        };

        lines.push(format!(
            "{:<10} [{status}] {} ({detail})",
            entry.kind, entry.id
        ));
    }

    lines.join("\n")
}

// ============================================================================
// Property Report — compact, serializable summary for VOPR integration
// ============================================================================

/// A compact summary of property tracking for a single simulation run.
///
/// Designed to be small and actionable: lists violated ALWAYS/NEVER properties,
/// unsatisfied SOMETIMES properties (coverage gaps), and counts.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PropertyReport {
    /// Total distinct properties observed during this run.
    pub total_properties: u64,
    /// Number of ALWAYS properties evaluated.
    pub always_evaluated: u64,
    /// Number of ALWAYS violations (any > 0 is a bug).
    pub always_violations: u64,
    /// Number of NEVER properties evaluated.
    pub never_evaluated: u64,
    /// Number of NEVER violations (any > 0 is a bug).
    pub never_violations: u64,
    /// Number of SOMETIMES properties satisfied at least once.
    pub sometimes_satisfied: u64,
    /// Number of SOMETIMES properties evaluated but never satisfied (coverage gap).
    pub sometimes_unsatisfied: u64,
    /// Number of REACHED markers hit.
    pub reached_hit: u64,
    /// Number of REACHED markers declared but never hit (coverage gap).
    pub reached_missed: u64,
    /// Property IDs for ALWAYS/NEVER violations (actionable bugs).
    pub violated_ids: Vec<String>,
    /// Property IDs for SOMETIMES properties never satisfied (coverage gaps).
    pub unsatisfied_sometimes_ids: Vec<String>,
    /// Property IDs for SOMETIMES properties that WERE satisfied this run.
    /// Batch-level aggregators need this because a SOMETIMES that is always
    /// satisfied on every seed would otherwise be invisible to them.
    pub satisfied_sometimes_ids: Vec<String>,
    /// Property IDs for REACHED markers never hit (coverage gaps).
    pub unreached_ids: Vec<String>,
    /// Property IDs for REACHED markers that were hit this run.
    pub reached_hit_ids: Vec<String>,
}

impl PropertyReport {
    /// Generates a report from the current registry state.
    #[must_use]
    pub fn from_registry() -> Self {
        let snap = snapshot();
        let mut report = PropertyReport {
            total_properties: snap.len() as u64,
            ..Default::default()
        };

        for record in snap.values() {
            match record.kind {
                PropertyKind::Always => {
                    report.always_evaluated += 1;
                    if record.violations > 0 {
                        report.always_violations += 1;
                        report.violated_ids.push(record.id.clone());
                    }
                }
                PropertyKind::Never => {
                    report.never_evaluated += 1;
                    if record.violations > 0 {
                        report.never_violations += 1;
                        report.violated_ids.push(record.id.clone());
                    }
                }
                PropertyKind::Sometimes => {
                    if record.satisfied {
                        report.sometimes_satisfied += 1;
                        report.satisfied_sometimes_ids.push(record.id.clone());
                    } else {
                        report.sometimes_unsatisfied += 1;
                        report.unsatisfied_sometimes_ids.push(record.id.clone());
                    }
                }
                PropertyKind::Reached => {
                    if record.satisfied {
                        report.reached_hit += 1;
                        report.reached_hit_ids.push(record.id.clone());
                    } else {
                        report.reached_missed += 1;
                        report.unreached_ids.push(record.id.clone());
                    }
                }
                PropertyKind::Unreachable => {
                    if record.violations > 0 {
                        report.violated_ids.push(record.id.clone());
                    }
                }
            }
        }

        // Stable ordering for deterministic output.
        report.violated_ids.sort();
        report.unsatisfied_sometimes_ids.sort();
        report.satisfied_sometimes_ids.sort();
        report.unreached_ids.sort();
        report.reached_hit_ids.sort();
        report
    }

    /// Returns true if the report contains no violations.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.always_violations == 0 && self.never_violations == 0
    }

    /// Returns total coverage gaps (unsatisfied sometimes + unreached markers).
    #[must_use]
    pub fn coverage_gaps(&self) -> u64 {
        self.sometimes_unsatisfied + self.reached_missed
    }

    /// Returns a compact human-readable summary line.
    #[must_use]
    pub fn summary_line(&self) -> String {
        format!(
            "always: {}/{} OK, never: {}/{} OK, sometimes: {}/{} hit, reached: {}/{} hit",
            self.always_evaluated - self.always_violations,
            self.always_evaluated,
            self.never_evaluated - self.never_violations,
            self.never_evaluated,
            self.sometimes_satisfied,
            self.sometimes_satisfied + self.sometimes_unsatisfied,
            self.reached_hit,
            self.reached_hit + self.reached_missed,
        )
    }
}

/// Returns properties that have coverage issues:
/// - SOMETIMES properties never satisfied
/// - REACHED properties never hit
/// - ALWAYS/NEVER properties never evaluated (dead code)
pub fn unsatisfied_properties() -> Vec<PropertyRecord> {
    let snap = snapshot();
    snap.into_values()
        .filter(|p| match p.kind {
            PropertyKind::Sometimes | PropertyKind::Reached => !p.satisfied,
            PropertyKind::Always | PropertyKind::Never | PropertyKind::Unreachable => {
                p.evaluations == 0
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_always_tracking() {
        reset();
        record_always("test.a1", true, "passes");
        record_always("test.a1", true, "passes");
        record_always("test.a1", false, "fails once");

        let snap = snapshot();
        let prop = snap.get("test.a1").unwrap();
        assert_eq!(prop.evaluations, 3);
        assert_eq!(prop.violations, 1);
        assert_eq!(prop.kind, PropertyKind::Always);
    }

    #[test]
    fn test_sometimes_tracking() {
        reset();
        record_sometimes("test.s1", false, "not yet");
        record_sometimes("test.s1", false, "not yet");
        assert!(!snapshot().get("test.s1").unwrap().satisfied);

        record_sometimes("test.s1", true, "now!");
        assert!(snapshot().get("test.s1").unwrap().satisfied);

        // Once satisfied, stays satisfied
        record_sometimes("test.s1", false, "doesn't matter");
        assert!(snapshot().get("test.s1").unwrap().satisfied);
    }

    #[test]
    fn test_never_tracking() {
        reset();
        record_never("test.n1", false, "good");
        record_never("test.n1", true, "violation!");

        let snap = snapshot();
        let prop = snap.get("test.n1").unwrap();
        assert_eq!(prop.evaluations, 2);
        assert_eq!(prop.violations, 1);
    }

    #[test]
    fn test_reached_tracking() {
        reset();
        record_reached("test.r1", "path");

        let snap = snapshot();
        let prop = snap.get("test.r1").unwrap();
        assert!(prop.satisfied);
        assert_eq!(prop.evaluations, 1);
    }

    #[test]
    fn test_unsatisfied_properties() {
        reset();
        record_sometimes("test.hit", true, "hit");
        record_sometimes("test.miss", false, "miss");
        record_reached("test.reached", "reached");

        let unsatisfied = unsatisfied_properties();
        assert_eq!(unsatisfied.len(), 1);
        assert_eq!(unsatisfied[0].id, "test.miss");
    }

    #[test]
    fn test_summary_report() {
        reset();
        record_always("kernel.offset_mono", true, "offset monotonicity");
        record_sometimes("vsr.view_change_crash", false, "view change with crash");
        record_never("vsr.dual_leader", false, "two leaders same view");
        record_reached("recovery.corrupt_log", "recovery from corrupt log");

        let report = summary_report();
        assert!(report.contains("kernel.offset_mono"));
        assert!(report.contains("vsr.view_change_crash"));
        assert!(report.contains("vsr.dual_leader"));
        assert!(report.contains("recovery.corrupt_log"));
    }

    #[test]
    fn test_property_report_clean_run() {
        reset();
        record_always("kernel.a", true, "always good");
        record_never("kernel.n", false, "never bad");
        record_sometimes("kernel.s", true, "sometimes hit");
        record_reached("kernel.r", "reached");

        let report = PropertyReport::from_registry();
        assert!(report.is_clean());
        assert_eq!(report.always_violations, 0);
        assert_eq!(report.never_violations, 0);
        assert_eq!(report.sometimes_satisfied, 1);
        assert_eq!(report.reached_hit, 1);
        assert_eq!(report.coverage_gaps(), 0);
    }

    #[test]
    fn test_property_report_with_gaps() {
        reset();
        record_always("kernel.a", true, "always good");
        record_sometimes("kernel.s_hit", true, "satisfied");
        record_sometimes("kernel.s_miss", false, "never satisfied");
        record_sometimes("kernel.s_miss2", false, "never satisfied either");

        let report = PropertyReport::from_registry();
        assert!(report.is_clean());
        assert_eq!(report.sometimes_satisfied, 1);
        assert_eq!(report.sometimes_unsatisfied, 2);
        assert_eq!(report.coverage_gaps(), 2);
        assert_eq!(report.unsatisfied_sometimes_ids.len(), 2);
        // Sorted alphabetically
        assert_eq!(report.unsatisfied_sometimes_ids[0], "kernel.s_miss");
        assert_eq!(report.unsatisfied_sometimes_ids[1], "kernel.s_miss2");
    }

    #[test]
    fn test_property_report_with_violations() {
        reset();
        record_always("kernel.a_bad", false, "violated always");
        record_never("kernel.n_bad", true, "violated never");

        let report = PropertyReport::from_registry();
        assert!(!report.is_clean());
        assert_eq!(report.always_violations, 1);
        assert_eq!(report.never_violations, 1);
        assert_eq!(report.violated_ids.len(), 2);
    }

    #[test]
    fn test_property_report_summary_line() {
        reset();
        record_always("a", true, "");
        record_always("b", true, "");
        record_sometimes("c", true, "");
        record_reached("d", "");

        let report = PropertyReport::from_registry();
        let line = report.summary_line();
        assert!(line.contains("always: 2/2"));
        assert!(line.contains("sometimes: 1/1"));
        assert!(line.contains("reached: 1/1"));
    }
}
