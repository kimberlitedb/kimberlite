//! Marzullo's algorithm for clock synchronization.
//!
//! Invented by Keith Marzullo for his Ph.D. dissertation in 1984, this agreement
//! algorithm selects sources for estimating accurate time from multiple noisy
//! time sources. NTP uses a modified form called the Intersection algorithm, which
//! returns a larger interval for further statistical sampling. However, we want
//! the smallest interval for maximum precision.
//!
//! # Algorithm Overview
//!
//! Given a set of clock samples (each with an interval [lower, upper] representing
//! uncertainty), the algorithm finds the smallest interval that is consistent with
//! the largest number of sources. This interval represents the synchronized time.
//!
//! # References
//!
//! - Marzullo, K. (1984). "Maintaining the Time in a Distributed System"
//! - Wikipedia: <https://en.wikipedia.org/wiki/Marzullo%27s_algorithm>
//! - TigerBeetle blog: "Three Clocks are Better than One"

use crate::types::ReplicaId;

/// The smallest interval consistent with the largest number of sources.
///
/// This represents the synchronized time window that has agreement from
/// the maximum number of replicas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    /// The lower bound on the minimum clock offset (nanoseconds).
    pub lower_bound: i64,

    /// The upper bound on the maximum clock offset (nanoseconds).
    pub upper_bound: i64,

    /// The number of "true chimers" consistent with the largest number of sources.
    ///
    /// A "true chimer" is a source whose interval overlaps with the optimal interval.
    pub sources_true: u8,

    /// The number of "false chimers" falling outside this interval.
    ///
    /// Where `sources_false + sources_true = total_sources`.
    pub sources_false: u8,
}

impl Interval {
    /// Returns the width of the interval in nanoseconds.
    pub fn width(&self) -> u64 {
        (self.upper_bound - self.lower_bound) as u64
    }

    /// Returns true if this interval represents a quorum agreement.
    pub fn has_quorum(&self, quorum_size: usize) -> bool {
        self.sources_true as usize >= quorum_size
    }

    /// Returns the midpoint of the interval (synchronized time).
    pub fn midpoint(&self) -> i64 {
        self.lower_bound + (self.upper_bound - self.lower_bound) / 2
    }
}

/// A tuple represents either the lower or upper end of a bound.
///
/// Fed as input to the Marzullo algorithm to compute the smallest interval
/// across all tuples.
///
/// # Example
///
/// Given a clock offset to a remote replica of 3s, a round trip time of 1s,
/// and a maximum tolerance between clocks of 100ms on either side, we create
/// two tuples:
/// - Lower bound: offset = 2.4s (3s - 0.5s RTT - 0.1s tolerance)
/// - Upper bound: offset = 3.6s (3s + 0.5s RTT + 0.1s tolerance)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuple {
    /// Identifier for the clock source (replica).
    pub source: ReplicaId,

    /// Clock offset in nanoseconds.
    pub offset: i64,

    /// Whether this is a lower or upper bound.
    pub bound: Bound,
}

/// Bound type for a tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// Lower bound of the interval.
    Lower,
    /// Upper bound of the interval.
    Upper,
}

/// Returns the smallest interval consistent with the largest number of sources.
///
/// # Algorithm
///
/// 1. Sort all tuples by offset (lower bounds before upper bounds at same offset)
/// 2. Sweep through sorted tuples, maintaining a count of overlapping intervals
/// 3. Track the maximum count and corresponding interval
/// 4. Handle ties by selecting the smallest interval
///
/// # Arguments
///
/// * `tuples` - Slice of tuples (2 per source: lower and upper bounds)
///
/// # Returns
///
/// The smallest interval with maximum source agreement.
///
/// # Panics
///
/// Panics if `tuples.len()` is odd (each source must have 2 bounds).
pub fn smallest_interval(tuples: &mut [Tuple]) -> Interval {
    assert!(
        tuples.len() % 2 == 0,
        "tuples must have even length (2 per source)"
    );

    let sources = (tuples.len() / 2) as u8;

    if sources == 0 {
        return Interval {
            lower_bound: 0,
            upper_bound: 0,
            sources_true: 0,
            sources_false: 0,
        };
    }

    // Sort tuples by offset, then by bound type (lower before upper)
    tuples.sort_by(tuple_less_than);

    // Sweep algorithm: track overlapping intervals
    let mut best_count: i64 = 0;
    let mut current_count: i64 = 0;
    let mut interval = Interval {
        lower_bound: 0,
        upper_bound: 0,
        sources_true: 0,
        sources_false: 0,
    };

    for i in 0..tuples.len() {
        let tuple = tuples[i];

        // Verify sort correctness (debug builds only)
        if i > 0 {
            let prev = tuples[i - 1];
            debug_assert!(
                prev.offset <= tuple.offset,
                "tuples not sorted by offset"
            );
            if prev.offset == tuple.offset {
                if prev.bound != tuple.bound {
                    debug_assert!(
                        matches!(prev.bound, Bound::Lower) && matches!(tuple.bound, Bound::Upper),
                        "lower bounds must come before upper bounds"
                    );
                } else {
                    debug_assert!(
                        prev.source.as_u8() < tuple.source.as_u8(),
                        "tuples not sorted by source"
                    );
                }
            }
        }

        // Update current count of overlapping intervals
        match tuple.bound {
            Bound::Lower => current_count += 1,
            Bound::Upper => current_count -= 1,
        }

        // The last upper bound tuple will have count one less than lower.
        // Therefore, we should never see current_count >= best_count for last tuple.
        if current_count > best_count {
            best_count = current_count;
            interval.lower_bound = tuple.offset;
            interval.upper_bound = tuples[i + 1].offset;
        } else if current_count == best_count && matches!(tuples[i + 1].bound, Bound::Upper) {
            // Tie for best overlap. Both intervals have same number of sources.
            // Choose the smaller interval.
            let alternative_width = tuples[i + 1].offset - tuple.offset;
            let current_width = interval.upper_bound - interval.lower_bound;
            if alternative_width < current_width {
                interval.lower_bound = tuple.offset;
                interval.upper_bound = tuples[i + 1].offset;
            }
        }
    }

    // Verify last tuple is an upper bound
    debug_assert!(
        matches!(tuples[tuples.len() - 1].bound, Bound::Upper),
        "last tuple must be upper bound"
    );

    // Calculate true/false sources
    assert!(
        best_count <= sources as i64,
        "best count exceeds total sources"
    );
    interval.sources_true = best_count as u8;
    interval.sources_false = sources - interval.sources_true;

    assert_eq!(
        interval.sources_true + interval.sources_false,
        sources,
        "source count mismatch"
    );

    interval
}

/// Comparison function for sorting tuples.
///
/// Sort order:
/// 1. By offset (ascending)
/// 2. If offsets equal, lower bounds before upper bounds
/// 3. If offsets and bounds equal, by source ID (for stability)
///
/// This ensures that when one interval ends just as another begins
/// (pathological overlap with zero duration), the algorithm can still
/// detect it by sorting the lower bound before the upper bound.
fn tuple_less_than(a: &Tuple, b: &Tuple) -> std::cmp::Ordering {
    match a.offset.cmp(&b.offset) {
        std::cmp::Ordering::Less => std::cmp::Ordering::Less,
        std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
        std::cmp::Ordering::Equal => {
            // Same offset: lower bounds come first
            match (a.bound, b.bound) {
                (Bound::Lower, Bound::Upper) => std::cmp::Ordering::Less,
                (Bound::Upper, Bound::Lower) => std::cmp::Ordering::Greater,
                _ => {
                    // Same bound: sort by source for stability
                    a.source.as_u8().cmp(&b.source.as_u8())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create tuples from offset pairs.
    fn make_tuples(bounds: &[i64]) -> Vec<Tuple> {
        assert!(bounds.len() % 2 == 0, "bounds must be in pairs");
        bounds
            .chunks(2)
            .enumerate()
            .flat_map(|(i, pair)| {
                let source = ReplicaId::new(i as u8);
                vec![
                    Tuple {
                        source,
                        offset: pair[0],
                        bound: Bound::Lower,
                    },
                    Tuple {
                        source,
                        offset: pair[1],
                        bound: Bound::Upper,
                    },
                ]
            })
            .collect()
    }

    fn test_interval(bounds: &[i64], expected: Interval) {
        let mut tuples = make_tuples(bounds);
        let result = smallest_interval(&mut tuples);
        assert_eq!(
            result, expected,
            "interval mismatch for bounds {bounds:?}"
        );
    }

    #[test]
    fn basic_overlap() {
        // Three sources with full overlap at [11, 12]
        test_interval(
            &[11, 13, 10, 12, 8, 12],
            Interval {
                lower_bound: 11,
                upper_bound: 12,
                sources_true: 3,
                sources_false: 0,
            },
        );
    }

    #[test]
    fn partial_overlap() {
        // Three sources, one outlier: best overlap is [11, 12]
        test_interval(
            &[8, 12, 11, 13, 14, 15],
            Interval {
                lower_bound: 11,
                upper_bound: 12,
                sources_true: 2,
                sources_false: 1,
            },
        );
    }

    #[test]
    fn zero_width_interval() {
        // Three sources with exact agreement at offset 0
        test_interval(
            &[-10, 10, -1, 1, 0, 0],
            Interval {
                lower_bound: 0,
                upper_bound: 0,
                sources_true: 3,
                sources_false: 0,
            },
        );
    }

    #[test]
    fn inclusive_overlap() {
        // Upper bound of first interval overlaps with lower of last
        test_interval(
            &[8, 12, 10, 11, 8, 10],
            Interval {
                lower_bound: 10,
                upper_bound: 10,
                sources_true: 3,
                sources_false: 0,
            },
        );
    }

    #[test]
    fn tie_smallest_interval() {
        // Two intervals with equal overlap: choose smallest [8, 9]
        test_interval(
            &[8, 12, 10, 12, 8, 9],
            Interval {
                lower_bound: 8,
                upper_bound: 9,
                sources_true: 2,
                sources_false: 1,
            },
        );
    }

    #[test]
    fn tie_smallest_interval_last() {
        // Alternative interval is larger: choose [10, 11]
        test_interval(
            &[7, 9, 7, 12, 10, 11],
            Interval {
                lower_bound: 10,
                upper_bound: 11,
                sources_true: 2,
                sources_false: 1,
            },
        );
    }

    #[test]
    fn negative_offsets() {
        // Same pattern as above but with negative offsets
        test_interval(
            &[-9, -7, -12, -7, -11, -10],
            Interval {
                lower_bound: -11,
                upper_bound: -10,
                sources_true: 2,
                sources_false: 1,
            },
        );
    }

    #[test]
    fn empty_sources() {
        // Cluster of one with no remote sources
        test_interval(
            &[],
            Interval {
                lower_bound: 0,
                upper_bound: 0,
                sources_true: 0,
                sources_false: 0,
            },
        );
    }

    #[test]
    fn single_remote_source() {
        // Cluster of two with one remote source
        test_interval(
            &[1, 3],
            Interval {
                lower_bound: 1,
                upper_bound: 3,
                sources_true: 1,
                sources_false: 0,
            },
        );
    }

    #[test]
    fn two_remote_sources_agreement() {
        // Cluster of three with agreement at offset 2
        test_interval(
            &[1, 3, 2, 2],
            Interval {
                lower_bound: 2,
                upper_bound: 2,
                sources_true: 2,
                sources_false: 0,
            },
        );
    }

    #[test]
    fn two_remote_sources_no_overlap() {
        // Cluster of three with no agreement, still returns smallest interval
        test_interval(
            &[1, 3, 4, 5],
            Interval {
                lower_bound: 4,
                upper_bound: 5,
                sources_true: 1,
                sources_false: 1,
            },
        );
    }

    #[test]
    fn interval_width() {
        let interval = Interval {
            lower_bound: 10,
            upper_bound: 20,
            sources_true: 3,
            sources_false: 0,
        };
        assert_eq!(interval.width(), 10);
    }

    #[test]
    fn interval_midpoint() {
        let interval = Interval {
            lower_bound: 10,
            upper_bound: 20,
            sources_true: 3,
            sources_false: 0,
        };
        assert_eq!(interval.midpoint(), 15);
    }

    #[test]
    fn has_quorum() {
        let interval = Interval {
            lower_bound: 0,
            upper_bound: 10,
            sources_true: 3,
            sources_false: 1,
        };
        assert!(interval.has_quorum(2));
        assert!(interval.has_quorum(3));
        assert!(!interval.has_quorum(4));
    }

    #[test]
    #[should_panic(expected = "tuples must have even length")]
    fn odd_tuples_panics() {
        let mut tuples = vec![Tuple {
            source: ReplicaId::new(0),
            offset: 0,
            bound: Bound::Lower,
        }];
        smallest_interval(&mut tuples);
    }
}
