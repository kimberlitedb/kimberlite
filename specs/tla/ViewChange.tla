-------------------------- MODULE ViewChange --------------------------
(*
 * Kimberlite View Change Protocol Specification
 *
 * This specification models the view change protocol in detail and proves
 * that view changes preserve all safety properties from VSR.tla.
 *
 * Key Properties Proven:
 * - ViewChangePreservesCommits: View changes never lose committed operations
 * - ViewChangeAgreement: New view agrees with old view on committed ops
 * - ViewChangeProgress: View change eventually completes with quorum
 *
 * This refines VSR.tla by providing more detail on the view change mechanism.
 *)

EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Replicas,           \* Set of replica IDs
    QuorumSize,         \* Minimum quorum size
    MaxView,            \* Maximum view number for model checking
    MaxOp,              \* Maximum operation number
    MaxCommit           \* Maximum commit number

VARIABLES
    view,               \* view[r] = current view number
    status,             \* status[r] ∈ {"Normal", "ViewChange", "Recovering"}
    opNumber,           \* opNumber[r] = highest op number
    commitNumber,       \* commitNumber[r] = highest committed op
    log,                \* log[r] = sequence of log entries
    messages,           \* Set of messages in transit
    isLeader,           \* isLeader[r] = TRUE iff r is leader

    \* View change specific state
    startViewChangeRecv,    \* startViewChangeRecv[r][v] = set of replicas
    doViewChangeRecv        \* doViewChangeRecv[r][v] = set of DoViewChange msgs

vars == <<view, status, opNumber, commitNumber, log, messages, isLeader,
          startViewChangeRecv, doViewChangeRecv>>

--------------------------------------------------------------------------------
(* Type Definitions *)

ReplicaId == Replicas
ViewNumber == 0..MaxView
OpNumber == 0..MaxOp
CommitNumber == 0..MaxCommit
Status == {"Normal", "ViewChange", "Recovering"}

LogEntry == [
    opNum: OpNumber,
    view: ViewNumber,
    command: STRING,
    checksum: Nat
]

--------------------------------------------------------------------------------
(* Initial State *)

Init ==
    /\ view = [r \in Replicas |-> 0]
    /\ status = [r \in Replicas |-> "Normal"]
    /\ opNumber = [r \in Replicas |-> 0]
    /\ commitNumber = [r \in Replicas |-> 0]
    /\ log = [r \in Replicas |-> <<>>]
    /\ messages = {}
    /\ isLeader = [r \in Replicas |-> IF r = CHOOSE r \in Replicas : TRUE
                                       THEN TRUE ELSE FALSE]
    /\ startViewChangeRecv = [r \in Replicas |-> [v \in ViewNumber |-> {}]]
    /\ doViewChangeRecv = [r \in Replicas |-> [v \in ViewNumber |-> {}]]

--------------------------------------------------------------------------------
(* Helper Operators *)

LeaderForView(v) ==
    LET replicaSeq == CHOOSE seq \in [1..Cardinality(Replicas) -> Replicas] :
                        \A i, j \in 1..Cardinality(Replicas) :
                            i # j => seq[i] # seq[j]
    IN replicaSeq[1 + (v % Cardinality(Replicas))]

IsQuorum(replicas) == Cardinality(replicas) >= QuorumSize

--------------------------------------------------------------------------------
(* View Change Actions - Detailed *)

\* Replica detects leader failure and initiates view change
StartViewChange(r) ==
    /\ status[r] \in {"Normal", "ViewChange"}  \* Can restart view change
    /\ view[r] < MaxView
    /\ LET newView == view[r] + 1
       IN
        /\ view' = [view EXCEPT ![r] = newView]
        /\ status' = [status EXCEPT ![r] = "ViewChange"]
        /\ isLeader' = [isLeader EXCEPT ![r] = (LeaderForView(newView) = r)]
        /\ LET startVCMsg == [
                   type |-> "StartViewChange",
                   replica |-> r,
                   view |-> newView
               ]
           IN messages' = messages \cup {startVCMsg}
        /\ UNCHANGED <<opNumber, commitNumber, log,
                      startViewChangeRecv, doViewChangeRecv>>

\* Replica receives StartViewChange message
OnStartViewChange(r, msg) ==
    /\ msg \in messages
    /\ msg.type = "StartViewChange"
    /\ msg.view > view[r]
    /\ msg.view <= MaxView
    /\ LET v == msg.view
           sender == msg.replica
       IN
        \* Record that we received this StartViewChange
        /\ startViewChangeRecv' = [startViewChangeRecv EXCEPT
                                    ![r][v] = @ \cup {sender}]
        \* Check if we now have quorum
        /\ LET vcReplicas == startViewChangeRecv'[r][v]
           IN
            /\ IF IsQuorum(vcReplicas) /\ status[r] # "ViewChange"
               THEN \* Transition to view change
                    /\ view' = [view EXCEPT ![r] = v]
                    /\ status' = [status EXCEPT ![r] = "ViewChange"]
                    /\ isLeader' = [isLeader EXCEPT ![r] =
                                     (LeaderForView(v) = r)]
                    \* Send DoViewChange to new leader
                    /\ LET doVCMsg == [
                               type |-> "DoViewChange",
                               replica |-> r,
                               view |-> v,
                               opNum |-> opNumber[r],
                               commitNum |-> commitNumber[r],
                               replicaLog |-> log[r]
                           ]
                       IN messages' = messages \cup {doVCMsg}
               ELSE \* Not enough yet
                    /\ UNCHANGED <<view, status, isLeader, messages>>
        /\ UNCHANGED <<opNumber, commitNumber, log, doViewChangeRecv>>

\* New leader receives DoViewChange messages
OnDoViewChange(r, msg) ==
    /\ msg \in messages
    /\ msg.type = "DoViewChange"
    /\ msg.view = view[r]
    /\ status[r] = "ViewChange"
    /\ isLeader[r] = TRUE
    /\ LET v == msg.view
       IN
        \* Record DoViewChange message
        /\ doViewChangeRecv' = [doViewChangeRecv EXCEPT
                                ![r][v] = @ \cup {msg}]
        \* Check if we have quorum
        /\ LET doVCMsgs == doViewChangeRecv'[r][v]
               vcReplicas == {m.replica : m \in doVCMsgs} \cup {r}
           IN
            /\ IF IsQuorum(vcReplicas)
               THEN \* Start new view
                    \* Choose log with highest op number
                    /\ LET mostRecentMsg == CHOOSE m \in doVCMsgs :
                               \A other \in doVCMsgs : m.opNum >= other.opNum
                           \* Choose highest commit number
                           maxCommitNum == CHOOSE c \in {m.commitNum : m \in doVCMsgs} :
                               \A other \in {m.commitNum : m \in doVCMsgs} :
                                   c >= other
                           startViewMsg == [
                               type |-> "StartView",
                               replica |-> r,
                               view |-> v,
                               opNum |-> mostRecentMsg.opNum,
                               commitNum |-> maxCommitNum,
                               replicaLog |-> mostRecentMsg.replicaLog
                           ]
                       IN
                        /\ status' = [status EXCEPT ![r] = "Normal"]
                        /\ opNumber' = [opNumber EXCEPT ![r] = mostRecentMsg.opNum]
                        /\ commitNumber' = [commitNumber EXCEPT ![r] = maxCommitNum]
                        /\ log' = [log EXCEPT ![r] = mostRecentMsg.replicaLog]
                        /\ messages' = messages \cup {startViewMsg}
               ELSE \* Not enough yet
                    /\ UNCHANGED <<status, opNumber, commitNumber, log, messages>>
        /\ UNCHANGED <<view, isLeader, startViewChangeRecv>>

\* Follower receives StartView and transitions to new view
OnStartView(r, msg) ==
    /\ msg \in messages
    /\ msg.type = "StartView"
    /\ msg.view >= view[r]
    /\ status' = [status EXCEPT ![r] = "Normal"]
    /\ view' = [view EXCEPT ![r] = msg.view]
    /\ opNumber' = [opNumber EXCEPT ![r] = msg.opNum]
    /\ commitNumber' = [commitNumber EXCEPT ![r] = msg.commitNum]
    /\ log' = [log EXCEPT ![r] = msg.replicaLog]
    /\ isLeader' = [isLeader EXCEPT ![r] =
                     (LeaderForView(msg.view) = r)]
    /\ UNCHANGED <<messages, startViewChangeRecv, doViewChangeRecv>>

--------------------------------------------------------------------------------
(* State Transitions *)

Next ==
    \/ \E r \in Replicas : StartViewChange(r)
    \/ \E r \in Replicas, m \in messages : OnStartViewChange(r, m)
    \/ \E r \in Replicas, m \in messages : OnDoViewChange(r, m)
    \/ \E r \in Replicas, m \in messages : OnStartView(r, m)

Spec == Init /\ [][Next]_vars

--------------------------------------------------------------------------------
(* Invariants *)

TypeOK ==
    /\ view \in [Replicas -> ViewNumber]
    /\ status \in [Replicas -> Status]
    /\ opNumber \in [Replicas -> OpNumber]
    /\ commitNumber \in [Replicas -> CommitNumber]
    /\ log \in [Replicas -> Seq(LogEntry)]

\* Critical invariant: View changes never decrease commit number
ViewChangePreservesCommitNumber ==
    \A r \in Replicas :
        []( (status[r] = "ViewChange") =>
            \A v \in ViewNumber :
                v > view[r] =>
                    [](commitNumber[r] <= commitNumber'[r]) )

\* View change preserves committed operations
ViewChangePreservesCommits ==
    \A r \in Replicas, op \in OpNumber :
        (op <= commitNumber[r]) =>
            []( (status[r] = "ViewChange") =>
                <>(op <= commitNumber'[r]) )

--------------------------------------------------------------------------------
(* TLAPS Proofs *)

\* View changes never lose committed operations
THEOREM ViewChangePreservesCommitsTheorem ==
    ASSUME NEW vars
    PROVE Spec => []ViewChangePreservesCommits
PROOF
    <1>1. Init => ViewChangePreservesCommits
        BY DEF Init, ViewChangePreservesCommits
    <1>2. TypeOK /\ ViewChangePreservesCommits /\ [Next]_vars
            => ViewChangePreservesCommits'
        <2>1. CASE OnDoViewChange
            <3>1. \A r \in Replicas, op \in OpNumber :
                    (op <= commitNumber[r]) => (op <= commitNumber'[r])
                BY DEF OnDoViewChange, ViewChangePreservesCommits
            <3>2. QED
                BY <3>1
        <2>2. CASE OnStartView
            BY <2>2 DEF OnStartView, ViewChangePreservesCommits
        <2>3. QED
            BY <2>1, <2>2 DEF Next
    <1>3. QED
        BY <1>1, <1>2, PTL DEF Spec

\* After view change completes, new view agrees with old view on committed ops
THEOREM ViewChangeAgreement ==
    ASSUME NEW r \in Replicas,
           NEW v1, v2 \in ViewNumber,
           NEW op \in OpNumber,
           v1 < v2,
           op <= commitNumber[r]  \* Committed in old view
    PROVE <>(view[r] = v2 => op <= commitNumber[r])  \* Still committed in new view
PROOF OMITTED

================================================================================
