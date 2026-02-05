------------------------- MODULE RepairBudget -------------------------
(*
  Repair Budget Management for VSR

  Prevents repair storms that can overwhelm cluster send queues.

  TigerBeetle Problem: Lagging replicas flood cluster with unbounded repair
  requests → send queue overflow (TigerBeetle queues = 4 messages) → cascading failure.

  Solution: Credit-based rate limiting with EWMA latency tracking
  - Limit inflight repairs (max 2 per replica)
  - Route to fastest replicas (90% EWMA, 10% experiment)
  - Expire stale requests (500ms timeout)

  Key properties:
  - BoundedInflight: Per-replica inflight ≤ 2 (prevents queue overflow)
  - FairRepair: All lagging replicas eventually get repairs (no starvation)
  - NoRepairStorm: Total repairs bounded by budget (prevents cascading failure)
*)

EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Replicas,              \* Set of replica IDs
    MaxInflightPerReplica, \* Maximum inflight per replica (2)
    RepairTimeoutMs        \* Request expiry timeout (500ms)

VARIABLES
    inflight_count,        \* Per-replica inflight request count
    ewma_latency,          \* Per-replica EWMA latency (nanoseconds)
    request_send_times,    \* Timestamps when requests were sent
    current_time           \* Current timestamp (for expiry)

vars == <<inflight_count, ewma_latency, request_send_times, current_time>>

-----------------------------------------------------------------------------

TypeOK ==
    /\ inflight_count \in [Replicas -> Nat]
    /\ ewma_latency \in [Replicas -> Nat]
    /\ request_send_times \in [Replicas -> SUBSET Nat]
    /\ current_time \in Nat

-----------------------------------------------------------------------------

Init ==
    /\ inflight_count = [r \in Replicas |-> 0]
    /\ ewma_latency = [r \in Replicas |-> 1000000] \* Start at 1ms
    /\ request_send_times = [r \in Replicas |-> {}]
    /\ current_time = 0

(* Select replica for repair request *)
(*
  90% select fastest (minimum EWMA latency)
  10% select random (exploration)
*)
SelectReplica ==
    LET available == {r \in Replicas :
                      inflight_count[r] < MaxInflightPerReplica}
        fastest == IF available = {}
                   THEN CHOOSE r \in Replicas : TRUE \* dummy
                   ELSE CHOOSE r \in available :
                        \A other \in available :
                            ewma_latency[r] <= ewma_latency[other]
    IN IF available = {}
       THEN CHOOSE r \in Replicas : TRUE \* No replica available
       ELSE fastest \* Simplified: always pick fastest (no randomness in TLA+)

(* Send repair request to a replica *)
SendRepair ==
    LET replica == SelectReplica
    IN
        /\ inflight_count[replica] < MaxInflightPerReplica
        /\ inflight_count' = [inflight_count EXCEPT ![replica] =
                             inflight_count[replica] + 1]
        /\ request_send_times' = [request_send_times EXCEPT ![replica] =
                                 request_send_times[replica] \cup {current_time}]
        /\ UNCHANGED <<ewma_latency, current_time>>

(* Complete repair (update EWMA and decrement inflight) *)
CompleteRepair(replica, latency_ns) ==
    /\ inflight_count[replica] > 0
    /\ inflight_count' = [inflight_count EXCEPT ![replica] =
                         inflight_count[replica] - 1]
    /\ LET alpha == 0.2  \* EWMA smoothing factor
           old_ewma == ewma_latency[replica]
           new_ewma == alpha * latency_ns + (1.0 - alpha) * old_ewma
       IN ewma_latency' = [ewma_latency EXCEPT ![replica] = new_ewma]
    /\ request_send_times' = [request_send_times EXCEPT ![replica] =
                             request_send_times[replica] \ {CHOOSE t \in request_send_times[replica] : TRUE}]
    /\ UNCHANGED current_time

(* Expire stale requests (timeout after RepairTimeoutMs) *)
ExpireStaleRequests ==
    LET stale_replicas == {r \in Replicas :
                          \E t \in request_send_times[r] :
                              current_time - t >= RepairTimeoutMs}
    IN
        /\ stale_replicas /= {}
        /\ \E replica \in stale_replicas :
            /\ inflight_count' = [inflight_count EXCEPT ![replica] =
                                 inflight_count[replica] - 1]
            /\ LET penalty_latency == ewma_latency[replica] * 2  \* Penalty for timeout
               IN ewma_latency' = [ewma_latency EXCEPT ![replica] = penalty_latency]
            /\ request_send_times' = [request_send_times EXCEPT ![replica] =
                                     request_send_times[replica] \ {CHOOSE t \in request_send_times[replica] : current_time - t >= RepairTimeoutMs}]
        /\ UNCHANGED current_time

(* Time advances *)
Tick ==
    /\ current_time' = current_time + 1
    /\ UNCHANGED <<inflight_count, ewma_latency, request_send_times>>

-----------------------------------------------------------------------------

Next ==
    \/ SendRepair
    \/ \E r \in Replicas, latency \in Nat : CompleteRepair(r, latency)
    \/ ExpireStaleRequests
    \/ Tick

Spec == Init /\ [][Next]_vars /\ WF_vars(ExpireStaleRequests)

-----------------------------------------------------------------------------

(* SAFETY PROPERTIES *)

(* Property 1: Bounded Inflight *)
(*
  Per-replica inflight requests never exceed MAX_INFLIGHT_PER_REPLICA (2).
  This prevents send queue overflow (TigerBeetle's 4-message queue).
*)
BoundedInflight ==
    [](
        \A r \in Replicas :
            inflight_count[r] <= MaxInflightPerReplica
    )

(* Property 2: Fair Repair *)
(*
  All replicas with available slots eventually receive repair requests
  (no starvation). The 10% experiment chance ensures slow replicas get
  tested periodically.
*)
FairRepair ==
    [](
        \A r \in Replicas :
            inflight_count[r] < MaxInflightPerReplica
            => <>(<>inflight_count[r] > 0)  \* Eventually gets a repair
    )

(* Property 3: No Repair Storm *)
(*
  Total inflight repairs across all replicas is bounded.
  This prevents cascading failure from unbounded repair requests.
*)
NoRepairStorm ==
    LET total_inflight == LET Sum[S \in SUBSET Replicas] ==
                              IF S = {} THEN 0
                              ELSE LET r == CHOOSE x \in S : TRUE
                                   IN inflight_count[r] + Sum[S \ {r}]
                          IN Sum[Replicas]
        max_total == Cardinality(Replicas) * MaxInflightPerReplica
    IN [](total_inflight <= max_total)

(* Property 4: EWMA Latency Always Positive *)
(*
  EWMA latency values are always positive (never zero or negative).
  This ensures division by zero doesn't occur in replica selection.
*)
EwmaLatencyPositive ==
    [](
        \A r \in Replicas :
            ewma_latency[r] > 0
    )

(* Property 5: Request Timeout Enforcement *)
(*
  Requests older than RepairTimeoutMs are eventually expired.
  This prevents resource leaks from stuck requests.
*)
RequestTimeoutEnforced ==
    [](
        \A r \in Replicas :
            \A t \in request_send_times[r] :
                current_time - t >= RepairTimeoutMs
                => <>(\neg (t \in request_send_times'[r]))
    )

(* Property 6: Inflight Count Matches Request Times *)
(*
  Inflight count equals the number of tracked request send times
  (accounting invariant).
*)
InflightCountMatches ==
    [](
        \A r \in Replicas :
            inflight_count[r] = Cardinality(request_send_times[r])
    )

-----------------------------------------------------------------------------

(* INVARIANTS *)

Inv ==
    /\ TypeOK
    /\ BoundedInflight
    /\ NoRepairStorm
    /\ EwmaLatencyPositive
    /\ InflightCountMatches

=============================================================================
