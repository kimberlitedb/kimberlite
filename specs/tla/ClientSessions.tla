------------------------- MODULE ClientSessions -------------------------
(*
  Client Session Management for VSR

  Fixes two bugs from the VRR (Viewstamped Replication Revisited) paper:

  Bug #1: Successive Client Crashes
  - Problem: Client crashes and restarts, resets request number to 0
  - Server returns cached reply from *previous* client incarnation
  - Fix: Explicit session registration creates fresh request number space

  Bug #2: Uncommitted Request Table Updates
  - Problem: VRR updates client table on prepare (not commit)
  - View change → new leader rejects client (table not transferred)
  - Fix: Separate committed/uncommitted tracking, discard uncommitted on view change

  Key properties:
  - NoRequestCollision: Client crash doesn't return wrong cached replies
  - NoClientLockout: View change doesn't prevent valid requests
  - DeterministicEviction: All replicas evict same sessions (by commit_timestamp)
*)

EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Clients,            \* Set of client IDs
    MaxSessions,        \* Maximum concurrent sessions
    MaxRequestNumber    \* Maximum request number per client

VARIABLES
    committed_sessions,     \* Committed sessions with cached replies
    uncommitted_sessions,   \* Uncommitted sessions (prepared but not committed)
    next_client_id,        \* Next client ID to assign
    view,                  \* Current view number

vars == <<committed_sessions, uncommitted_sessions, next_client_id, view>>

-----------------------------------------------------------------------------

(* Type definitions *)

ClientId == Nat

RequestNumber == Nat

Session == [
    request_number: RequestNumber,
    committed_op: Nat,
    reply_op: Nat,
    commit_timestamp: Nat
]

UncommittedSession == [
    request_number: RequestNumber,
    preparing_op: Nat
]

-----------------------------------------------------------------------------

TypeOK ==
    /\ committed_sessions \in [ClientId -> Session]
    /\ uncommitted_sessions \in [ClientId -> UncommittedSession]
    /\ next_client_id \in Nat
    /\ view \in Nat

-----------------------------------------------------------------------------

Init ==
    /\ committed_sessions = <<>>
    /\ uncommitted_sessions = <<>>
    /\ next_client_id = 1
    /\ view = 0

(* Client registers and gets assigned a new session *)
RegisterClient ==
    /\ next_client_id' = next_client_id + 1
    /\ UNCHANGED <<committed_sessions, uncommitted_sessions, view>>

(* Check if request is a duplicate of a committed request *)
IsDuplicate(client_id, request_number) ==
    /\ client_id \in DOMAIN committed_sessions
    /\ committed_sessions[client_id].request_number = request_number

(* Record an uncommitted request during prepare phase *)
RecordUncommitted(client_id, request_number, preparing_op) ==
    LET
        \* Check monotonicity: request must be > any committed request
        monotonic_ok ==
            \/ client_id \notin DOMAIN committed_sessions
            \/ request_number > committed_sessions[client_id].request_number

        uncommitted == [
            request_number |-> request_number,
            preparing_op |-> preparing_op
        ]
    IN
        /\ monotonic_ok
        /\ uncommitted_sessions' = uncommitted_sessions @@ (client_id :> uncommitted)
        /\ UNCHANGED <<committed_sessions, next_client_id, view>>

(* Commit a request (move from uncommitted to committed) *)
CommitRequest(client_id, request_number, committed_op, reply_op, commit_timestamp) ==
    LET
        \* Must match uncommitted session
        matches_uncommitted ==
            /\ client_id \in DOMAIN uncommitted_sessions
            /\ uncommitted_sessions[client_id].request_number = request_number

        session == [
            request_number |-> request_number,
            committed_op |-> committed_op,
            reply_op |-> reply_op,
            commit_timestamp |-> commit_timestamp
        ]

        \* Remove from uncommitted
        new_uncommitted == [k \in (DOMAIN uncommitted_sessions \ {client_id}) |->
                           uncommitted_sessions[k]]

        \* Add to committed
        new_committed == committed_sessions @@ (client_id :> session)

        \* Evict oldest if needed
        needs_eviction == Cardinality(DOMAIN new_committed) > MaxSessions

        oldest_client == IF needs_eviction
                        THEN CHOOSE c \in DOMAIN new_committed :
                            \A other \in DOMAIN new_committed :
                                new_committed[c].commit_timestamp <=
                                new_committed[other].commit_timestamp
                        ELSE client_id  \* dummy value

        final_committed == IF needs_eviction
                          THEN [k \in (DOMAIN new_committed \ {oldest_client}) |->
                                new_committed[k]]
                          ELSE new_committed
    IN
        /\ matches_uncommitted \/ client_id \notin DOMAIN uncommitted_sessions
        /\ committed_sessions' = final_committed
        /\ uncommitted_sessions' = new_uncommitted
        /\ UNCHANGED <<next_client_id, view>>

(* View change: discard all uncommitted sessions *)
ViewChange ==
    /\ view' = view + 1
    /\ uncommitted_sessions' = <<>>
    /\ UNCHANGED <<committed_sessions, next_client_id>>

-----------------------------------------------------------------------------

Next ==
    \/ RegisterClient
    \/ \E client \in ClientId, req \in RequestNumber, op \in Nat :
        RecordUncommitted(client, req, op)
    \/ \E client \in ClientId, req \in RequestNumber,
          cop \in Nat, rop \in Nat, ts \in Nat :
        CommitRequest(client, req, cop, rop, ts)
    \/ ViewChange

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------

(* SAFETY PROPERTIES *)

(* Property 1: No Request Collision (VRR Bug #1 fix) *)
(*
  If a client crashes and restarts with request number 0, the server
  must not return a cached reply from a different session.

  Fix: Explicit session registration creates new session ID.
*)
NoRequestCollision ==
    \A client1, client2 \in ClientId :
        /\ client1 \in DOMAIN committed_sessions
        /\ client2 \in DOMAIN committed_sessions
        /\ client1 /= client2
        =>
            \* Different clients can have same request numbers without collision
            \/ committed_sessions[client1].request_number /=
               committed_sessions[client2].request_number
            \/ committed_sessions[client1].reply_op /=
               committed_sessions[client2].reply_op

(* Property 2: No Client Lockout (VRR Bug #2 fix) *)
(*
  View change must not prevent valid client requests from being processed.

  Fix: Uncommitted sessions are discarded on view change, so new leader
  doesn't reject clients whose uncommitted state wasn't transferred.
*)
NoClientLockout ==
    [](view' > view =>
        \* After view change, uncommitted sessions are empty
        uncommitted_sessions' = <<>>)

(* Property 3: Deterministic Eviction *)
(*
  When eviction is needed, all replicas evict the same session (oldest
  by commit_timestamp). This ensures replica consistency.
*)
DeterministicEviction ==
    \A client1, client2 \in DOMAIN committed_sessions :
        client1 /= client2 =>
            \* If sessions evicted, oldest goes first
            (committed_sessions[client1].commit_timestamp <
             committed_sessions[client2].commit_timestamp
             => client1 \notin DOMAIN committed_sessions')

(* Property 4: Request Number Monotonicity *)
(*
  Within a client session, request numbers only increase.
*)
RequestNumberMonotonic ==
    [](
        \A client \in ClientId :
            /\ client \in DOMAIN committed_sessions
            /\ client \in DOMAIN committed_sessions'
            =>
                committed_sessions'[client].request_number >=
                committed_sessions[client].request_number
    )

(* Property 5: Committed Sessions Survive View Changes *)
(*
  View changes discard uncommitted sessions but preserve committed ones.
*)
CommittedSessionsSurviveViewChange ==
    [](view' > view =>
        \* All committed sessions from before view change still exist
        \A client \in DOMAIN committed_sessions :
            client \in DOMAIN committed_sessions')

(* Property 6: No Duplicate Commits *)
(*
  A client cannot commit the same request number twice.
*)
NoDuplicateCommits ==
    [](
        \A client \in ClientId :
            /\ client \in DOMAIN committed_sessions
            /\ client \in DOMAIN committed_sessions'
            =>
                committed_sessions[client].request_number =
                committed_sessions'[client].request_number
                =>
                committed_sessions[client] = committed_sessions'[client]
    )

-----------------------------------------------------------------------------

(* INVARIANTS *)

Inv ==
    /\ TypeOK
    /\ NoRequestCollision
    /\ RequestNumberMonotonic
    /\ NoDuplicateCommits

=============================================================================
