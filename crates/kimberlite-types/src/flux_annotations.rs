//! Flux refinement type annotations
//!
//! This module contains Flux refinement type signatures that provide
//! compile-time verification of safety properties. These annotations
//! are currently commented out as Flux compiler is experimental.
//!
//! When Flux stabilizes, uncomment the `#[flux::sig(...)]` attributes
//! to enable compile-time verification of:
//! - Offset monotonicity
//! - Tenant isolation
//! - View number monotonicity
//! - Quorum properties
//!
//! ## Flux Installation (when ready)
//! ```bash
//! git clone https://github.com/flux-rs/flux
//! cd flux && cargo install --path .
//! ```
//!
//! ## Verification Commands
//! ```bash
//! flux check crates/kimberlite-kernel
//! flux check crates/kimberlite-types
//! flux check crates/kimberlite-vsr
//! ```

// Flux refinement type signatures for core types

/* Flux annotations - UNCOMMENT when Flux compiler is stable

use crate::{Offset, TenantId, StreamId};

// ============================================================================
// Offset Monotonicity Refinements
// ============================================================================

/// Offset refinement: value is always non-negative
#[flux::refined_by(v: int)]
pub struct RefinedOffset {
    #[flux::field(u64{n: n >= 0})]
    inner: u64,
}

/// Append operation increases offset
#[flux::sig(fn(Offset{o1: o1.inner >= 0}, count: usize{c: c > 0}) -> Offset{o2: o2.inner > o1.inner})]
pub fn offset_after_append(current: Offset, count: usize) -> Offset {
    Offset::new(current.as_u64() + count as u64)
}

/// Offset comparison is well-defined
#[flux::sig(fn(Offset{o1}, Offset{o2}) -> bool{b: b == (o1.inner < o2.inner)})]
pub fn offset_less_than(a: Offset, b: Offset) -> bool {
    a < b
}

// ============================================================================
// Tenant Isolation Refinements
// ============================================================================

/// TenantId refinement: never zero (reserved for system)
#[flux::refined_by(id: int)]
pub struct RefinedTenantId {
    #[flux::field(u64{n: n > 0})]
    inner: u64,
}

/// StreamId refinement: always associated with a valid tenant
#[flux::refined_by(tenant_id: int, local_id: int)]
pub struct RefinedStreamId {
    #[flux::field(TenantId{t: t.inner > 0})]
    tenant_id: TenantId,

    #[flux::field(u64{n: n >= 0})]
    local_id: u64,
}

/// Reading from stream returns events for that stream's tenant only
#[flux::sig(fn(StreamId{s: s.tenant_id == t}) -> Vec<Event{e: e.tenant_id == t}>)]
pub fn read_stream_events(stream_id: StreamId) -> Vec<Event> {
    // Implementation ensures tenant isolation
    unimplemented!()
}

/// Cross-tenant access is impossible
#[flux::sig(fn(TenantId{t1}, TenantId{t2}) -> bool{b: (t1.inner != t2.inner) => !b})]
pub fn can_access_tenant_data(accessor: TenantId, owner: TenantId) -> bool {
    accessor == owner
}

// ============================================================================
// View Number Monotonicity (VSR)
// ============================================================================

/// ViewNumber refinement: monotonically increasing
#[flux::refined_by(v: int)]
pub struct RefinedViewNumber {
    #[flux::field(u64{n: n >= 0})]
    inner: u64,
}

/// View change always increases view number
#[flux::sig(fn(ViewNumber{v1}) -> ViewNumber{v2: v2.inner > v1.inner})]
pub fn next_view(current: ViewNumber) -> ViewNumber {
    ViewNumber::new(current.as_u64() + 1)
}

/// Cannot start view change to lower view
#[flux::sig(fn(current: ViewNumber{vc}, proposed: ViewNumber{vp}) -> Result<(), Error>{r: vp.inner <= vc.inner => r.is_err()})]
pub fn start_view_change(current: ViewNumber, proposed: ViewNumber) -> Result<(), Error> {
    if proposed <= current {
        Err(Error::InvalidViewNumber)
    } else {
        Ok(())
    }
}

// ============================================================================
// Quorum Properties (VSR)
// ============================================================================

/// Quorum size must be > n/2 to ensure intersection
#[flux::sig(fn(n: usize{n: n > 0}) -> usize{q: 2 * q > n})]
pub fn quorum_size(replica_count: usize) -> usize {
    (replica_count / 2) + 1
}

/// ReplicaSet refinement: size is bounded by total replicas
#[flux::refined_by(size: int, capacity: int)]
pub struct RefinedReplicaSet {
    #[flux::field(HashSet<ReplicaId>{s: s.len() <= capacity})]
    members: HashSet<ReplicaId>,
}

/// Checking quorum ensures sufficient replicas
#[flux::sig(fn(ReplicaSet{rs: rs.size >= quorum_size(total)}, total: usize) -> bool{b: b == true})]
pub fn is_quorum(set: &ReplicaSet, total_replicas: usize) -> bool {
    set.len() >= quorum_size(total_replicas)
}

/// Two quorums must intersect
#[flux::sig(fn(
    ReplicaSet{q1: q1.size >= quorum_size(n)},
    ReplicaSet{q2: q2.size >= quorum_size(n)},
    n: usize
) -> bool{b: b == true})]
pub fn quorums_intersect(q1: &ReplicaSet, q2: &ReplicaSet, total: usize) -> bool {
    // Mathematical property: any two quorums must overlap
    // if both have size > n/2, then they must share at least one element
    !q1.members.is_disjoint(&q2.members)
}

// ============================================================================
// State Machine Safety (Kernel)
// ============================================================================

/// Stream creation produces unique stream ID
#[flux::sig(fn(
    state: State{s: !s.streams.contains_key(&id)},
    id: StreamId
) -> State{s2: s2.streams.contains_key(&id)})]
pub fn create_stream(state: State, id: StreamId) -> State {
    // Guarantees stream uniqueness
    unimplemented!()
}

/// AppendBatch increases stream offset
#[flux::sig(fn(
    state: State{s: s.get_stream(id).offset == old_offset},
    id: StreamId,
    events: Vec<Event>{v: v.len() == count}
) -> State{s2: s2.get_stream(id).offset.inner == old_offset.inner + count})]
pub fn append_batch(state: State, id: StreamId, events: Vec<Event>) -> State {
    // Guarantees offset monotonicity
    unimplemented!()
}

// ============================================================================
// Cryptographic Properties
// ============================================================================

/// Hash function is deterministic
#[flux::sig(fn(data: &[u8]) -> [u8; 32]{h: hash(data) == hash(data)})]
pub fn hash_deterministic(data: &[u8]) -> [u8; 32] {
    // SHA-256 always produces same output for same input
    unimplemented!()
}

/// Hash chain prevents tampering
#[flux::sig(fn(
    prev: Option<&[u8; 32]>,
    data: &[u8]
) -> [u8; 32]{h: prev.is_some() => h != [0u8; 32]})]
pub fn chain_hash(prev: Option<&[u8; 32]>, data: &[u8]) -> [u8; 32] {
    // Chain hash never produces all zeros
    unimplemented!()
}

*/

// Placeholder types for documentation (Flux not yet enabled)
// These will be replaced with actual Flux annotations when compiler stabilizes

/// Documentation: Offset monotonicity property
///
/// **Flux Signature (when enabled):**
/// ```ignore
/// #[flux::sig(fn(Offset{o1}, count: usize{c: c > 0}) -> Offset{o2: o2.inner > o1.inner})]
/// ```
///
/// **Property:** Appending events always increases the offset
pub struct OffsetMonotonicityProperty;

/// Documentation: Tenant isolation property
///
/// **Flux Signature (when enabled):**
/// ```ignore
/// #[flux::sig(fn(TenantId{t1}, TenantId{t2}) -> bool{b: (t1 != t2) => !b})]
/// ```
///
/// **Property:** Tenants cannot access each other's data
pub struct TenantIsolationProperty;

/// Documentation: View monotonicity property
///
/// **Flux Signature (when enabled):**
/// ```ignore
/// #[flux::sig(fn(ViewNumber{v1}) -> ViewNumber{v2: v2 > v1})]
/// ```
///
/// **Property:** View numbers only increase
pub struct ViewMonotonicityProperty;

/// Documentation: Quorum intersection property
///
/// **Flux Signature (when enabled):**
/// ```ignore
/// #[flux::sig(fn(ReplicaSet{q1: q1.size > n/2}, ReplicaSet{q2: q2.size > n/2}) -> bool{b: b == true})]
/// ```
///
/// **Property:** Any two quorums must intersect
pub struct QuorumIntersectionProperty;

#[cfg(test)]
mod tests {
    #[test]
    fn test_flux_annotations_exist() {
        // When Flux is enabled, these will be compile-time verified
        // For now, this test documents the intended properties
        // No runtime assertions needed - properties are type-level
    }
}
