--------------------------- MODULE ClockSync ---------------------------
(*
  Clock Synchronization for Kimberlite VSR

  Implements Marzullo's algorithm for cluster-wide clock consensus.

  Key properties:
  - ClockMonotonicity: Cluster time never goes backward
  - ClockQuorumConsensus: Time derived from quorum intersection

  HIPAA/GDPR compliance requirement: Audit timestamps must be accurate
  and monotonic across view changes.
*)

EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Replicas,           \* Set of replica IDs
    MaxClockSkew,       \* Maximum allowed clock offset (milliseconds)
    EpochDuration,      \* How long an epoch lasts (milliseconds)
    QuorumSize          \* Minimum replicas for consensus

VARIABLES
    clock_samples,      \* [replica_id -> {wall_time, rtt, timestamp}]
    current_epoch,      \* Current synchronized epoch
    last_sync_time,     \* When synchronization last occurred
    cluster_time        \* Current cluster consensus time

vars == <<clock_samples, current_epoch, last_sync_time, cluster_time>>

-----------------------------------------------------------------------------

TypeOK ==
    /\ clock_samples \in [Replicas -> [
        wall_time: Nat,
        rtt: Nat,
        timestamp: Nat
    ]]
    /\ current_epoch \in Nat
    /\ last_sync_time \in Nat
    /\ cluster_time \in Nat

-----------------------------------------------------------------------------

(* Marzullo's Algorithm: Find smallest interval containing quorum *)

\* Create tuple: [earliest_possible, latest_possible, start/end marker, replica_id]
MakeTuples(samples) ==
    LET replica_ids == DOMAIN samples
    IN  { <<samples[r].wall_time - samples[r].rtt / 2, "start", r>> : r \in replica_ids }
        \cup
        { <<samples[r].wall_time + samples[r].rtt / 2, "end", r>> : r \in replica_ids }

\* Count how many intervals contain this point
CountContaining(point, tuples) ==
    LET starts == {t \in tuples : t[2] = "start" /\ t[1] <= point}
        ends == {t \in tuples : t[2] = "end" /\ t[1] < point}
    IN Cardinality(starts) - Cardinality(ends)

\* Find the smallest interval that contains at least QuorumSize replicas
SmallestInterval(samples) ==
    LET tuples == MakeTuples(samples)
        sorted_times == {t[1] : t \in tuples}
        \* Find first point where we have quorum
        quorum_points == {t \in sorted_times : CountContaining(t, tuples) >= QuorumSize}
    IN  IF quorum_points = {}
        THEN [valid |-> FALSE, time |-> 0]
        ELSE LET start_time == CHOOSE t \in quorum_points :
                                \A other \in quorum_points : t <= other
                 \* Find where quorum ends
                 end_candidates == {t \in sorted_times :
                                    t >= start_time /\ CountContaining(t, tuples) >= QuorumSize}
                 end_time == CHOOSE t \in end_candidates :
                                \A other \in end_candidates : t >= other
             IN [valid |-> TRUE, time |-> (start_time + end_time) \div 2]

-----------------------------------------------------------------------------

Init ==
    /\ clock_samples = [r \in Replicas |-> [wall_time |-> 0, rtt |-> 0, timestamp |-> 0]]
    /\ current_epoch = 0
    /\ last_sync_time = 0
    /\ cluster_time = 0

\* Replica submits a clock sample during heartbeat
SubmitClockSample(replica, wall_time, rtt) ==
    /\ clock_samples' = [clock_samples EXCEPT ![replica] =
        [wall_time |-> wall_time, rtt |-> rtt, timestamp |-> cluster_time]]
    /\ UNCHANGED <<current_epoch, last_sync_time, cluster_time>>

\* Primary attempts to synchronize cluster time
TrySynchronize ==
    LET result == SmallestInterval(clock_samples)
        new_time == result.time
        time_delta == cluster_time - last_sync_time
        epoch_expired == time_delta >= EpochDuration
    IN  /\ result.valid
        /\ epoch_expired
        /\ new_time >= cluster_time  \* Monotonicity check
        /\ new_time - cluster_time <= MaxClockSkew  \* Tolerance check
        /\ cluster_time' = new_time
        /\ current_epoch' = current_epoch + 1
        /\ last_sync_time' = new_time
        /\ UNCHANGED clock_samples

\* Time advances (no synchronization)
Tick ==
    /\ cluster_time' = cluster_time + 1
    /\ UNCHANGED <<clock_samples, current_epoch, last_sync_time>>

Next ==
    \/ \E r \in Replicas, wt \in Nat, rtt \in Nat : SubmitClockSample(r, wt, rtt)
    \/ TrySynchronize
    \/ Tick

Spec == Init /\ [][Next]_vars /\ WF_vars(TrySynchronize)

-----------------------------------------------------------------------------

(* INVARIANTS *)

\* Clock never goes backward
ClockMonotonicity ==
    []([cluster_time' >= cluster_time]_vars)

\* Synchronized time comes from quorum intersection
ClockQuorumConsensus ==
    [](current_epoch > 0 =>
        \E S \in SUBSET Replicas :
            /\ Cardinality(S) >= QuorumSize
            /\ \A r \in S :
                LET sample == clock_samples[r]
                    lower == sample.wall_time - sample.rtt / 2
                    upper == sample.wall_time + sample.rtt / 2
                IN lower <= cluster_time /\ cluster_time <= upper)

\* Clock offset stays within tolerance
ClockOffsetBounded ==
    [](current_epoch > 0 =>
        \A r \in Replicas :
            LET sample == clock_samples[r]
            IN sample.wall_time - cluster_time <= MaxClockSkew)

\* Epochs don't expire too quickly (liveness check)
EpochDurationRespected ==
    []([current_epoch' > current_epoch =>
        cluster_time' - last_sync_time >= EpochDuration]_vars)

-----------------------------------------------------------------------------

(* THEOREMS *)

\* THEOREM Monotonicity: Spec => ClockMonotonicity
\* THEOREM QuorumConsensus: Spec => ClockQuorumConsensus

=============================================================================
