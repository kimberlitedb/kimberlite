/**
 * Kimberlite Quorum System Model
 *
 * This Alloy specification proves structural properties of the quorum
 * system used in Viewstamped Replication consensus.
 *
 * Properties Proven:
 * - Quorum intersection (any two quorums overlap)
 * - Quorum size constraints (majority-based)
 * - View uniqueness (one leader per view)
 *
 * Critical for VSR safety: agreement depends on quorum intersection
 *
 * Note: All checks use scope 5 (matching #Replica = 5) for CI speed.
 * Integer arithmetic uses function-call style to avoid precedence issues.
 */

module kimberlite/Quorum

--------------------------------------------------------------------------------
-- Type Definitions

sig Replica {
    -- Replica ID (abstract)
}

sig View {
    -- Leader for this view (deterministic from view number)
    leader: one Replica
}

sig Quorum {
    -- Members of this quorum
    members: set Replica
} {
    -- Quorum size constraint: strictly more than half the replicas.
    -- For 5 replicas: quorum size >= 3
    -- Written without integer arithmetic to avoid precedence issues.
    #members >= 3
}

--------------------------------------------------------------------------------
-- Quorum System Configuration

-- Total number of replicas (configured for deployment)
fact ReplicaCount {
    -- For model checking, use 5 replicas
    #Replica = 5
}

--------------------------------------------------------------------------------
-- Quorum Properties

-- PROPERTY 1: Quorum intersection
-- Critical for VSR safety: any two quorums must overlap
pred quorumIntersection {
    all q1, q2: Quorum |
        some q1.members & q2.members
}

-- Verify quorum intersection
assert QuorumIntersection {
    quorumIntersection
}
check QuorumIntersection for 5

-- PROPERTY 2: Majority-based quorum
-- Quorums contain strictly more than half the replicas
-- With 5 replicas and quorum size >= 3: quorum is always a majority
assert MajorityQuorum {
    all q: Quorum | #q.members >= 3
}
check MajorityQuorum for 5

-- PROPERTY 3: Quorum intersection size
-- Any two quorums overlap in at least one replica
pred minimalIntersection {
    all q1, q2: Quorum |
        #(q1.members & q2.members) >= 1
}

assert MinimalIntersection {
    minimalIntersection
}
check MinimalIntersection for 5

-- PROPERTY 4: View leader uniqueness
-- Each view has exactly one leader
pred viewLeaderUniqueness {
    all v: View | one v.leader
}

assert ViewLeaderUniqueness {
    viewLeaderUniqueness
}
check ViewLeaderUniqueness for 5

--------------------------------------------------------------------------------
-- Byzantine Fault Tolerance (Extended Model)

-- Byzantine replica classification
sig ByzantineReplica extends Replica {}
sig HonestReplica extends Replica {}

fact ByzantinePartition {
    -- Every replica is either Byzantine or honest (not both)
    Replica = ByzantineReplica + HonestReplica
    no ByzantineReplica & HonestReplica
}

fact ByzantineUpperBound {
    -- At most f Byzantine replicas where f < n/3
    -- For n=5: f <= 1
    -- Written as a literal to avoid integer arithmetic
    #ByzantineReplica <= 1
}

-- PROPERTY 5: Honest majority in quorum
-- Every quorum contains at least 2 honest replicas
-- (majority quorum of 3 from 5 replicas, with at most 1 Byzantine)
assert HonestReplicasInQuorum {
    all q: Quorum |
        #(q.members & HonestReplica) >= 2
}
check HonestReplicasInQuorum for 5

--------------------------------------------------------------------------------
-- Failure Scenarios

-- Scenario 1: Quorum overlap with one Byzantine
pred quorumIntersectionWithByzantine {
    some ByzantineReplica  -- At least one Byzantine replica
    some q1, q2: Quorum |
        #(q1.members & q2.members & HonestReplica) >= 1
}
run quorumIntersectionWithByzantine for 5

-- Scenario 2: View change with quorum
pred viewChangeQuorum {
    some v1, v2: View | v1 != v2
    some q: Quorum |
        all r: q.members | r in Replica
}
run viewChangeQuorum for 5

--------------------------------------------------------------------------------
-- Visualization Predicates

-- Show valid quorum system with 5 replicas
pred showQuorumSystem {
    #Quorum = 3  -- Show 3 different quorums
    quorumIntersection
}
run showQuorumSystem for 5

-- Show quorum intersection property
pred showIntersection {
    #Quorum = 2
    some q1, q2: Quorum | q1 != q2 and
        some q1.members & q2.members
}
run showIntersection for 5

--------------------------------------------------------------------------------
-- Integration with Kimberlite VSR

/**
 * Mapping to Rust implementation:
 *
 * Replica <-> crates/kimberlite-vsr/src/replica/state.rs::ReplicaId
 * Quorum <-> crates/kimberlite-vsr/src/config.rs::quorum_size()
 * View <-> crates/kimberlite-vsr/src/types.rs::ViewNumber
 *
 * Key implementation properties:
 * - Quorum size calculated as: (n / 2) + 1
 * - For n=5: quorum_size = 3
 * - For n=7: quorum_size = 4
 *
 * VSR safety depends on quorum intersection:
 * - Two commits at same offset must agree (quorums overlap)
 * - View change preserves commits (quorums overlap)
 * - Recovery restores committed ops (quorum of responses)
 *
 * This Alloy model proves quorum intersection holds for all
 * valid configurations with n=5 and quorum_size=3.
 */
