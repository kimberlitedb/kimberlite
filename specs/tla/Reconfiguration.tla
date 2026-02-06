----------------------- MODULE Reconfiguration -----------------------
(*
  Cluster Reconfiguration for Kimberlite VSR

  Implements joint consensus algorithm (Raft-style) for safe cluster
  membership changes without downtime.

  Key properties:
  - ConfigurationSafety: Never have conflicting committed configs
  - QuorumOverlap: Any two quorums always overlap (at least one replica)
  - Progress: Joint consensus eventually transitions to stable
  - ViewChangePreservesReconfig: Reconfig state survives leader failures

  Based on:
  - Raft consensus (Ongaro & Ousterhout, 2014), Section 6
  - Kimberlite implementation in crates/kimberlite-vsr/src/reconfiguration.rs
*)

EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    InitialReplicas,    \* Initial set of replica IDs
    MaxReplicas,        \* Maximum cluster size for model checking
    MaxOp,              \* Maximum operation number
    MaxView             \* Maximum view number

VARIABLES
    replicas,           \* Current set of active replicas
    reconfig_state,     \* Per-replica: "Stable" or "Joint"
    old_config,         \* Old configuration (for joint consensus)
    new_config,         \* New configuration (for joint consensus)
    joint_op,           \* Operation number where joint consensus started
    op_number,          \* Highest operation number per replica
    commit_number,      \* Highest committed operation number per replica
    view,               \* Current view number per replica
    committed_configs,  \* Set of all committed configurations
    messages            \* Messages in transit

vars == <<replicas, reconfig_state, old_config, new_config, joint_op,
          op_number, commit_number, view, committed_configs, messages>>

-----------------------------------------------------------------------------

(* Type Definitions *)

ReplicaId == 1..MaxReplicas
ViewNumber == 0..MaxView
OpNumber == 0..MaxOp

ReconfigState == {"Stable", "Joint"}

ClusterConfig == SUBSET ReplicaId

ReconfigCommand ==
    [type: {"AddReplica"}, replica_id: ReplicaId]
    \cup
    [type: {"RemoveReplica"}, replica_id: ReplicaId]
    \cup
    [type: {"Replace"}, old_id: ReplicaId, new_id: ReplicaId]

MessageType == {
    "Prepare",
    "PrepareOk",
    "Commit",
    "DoViewChange",
    "StartView"
}

Message ==
    [type: {"Prepare"},
     view: ViewNumber,
     op_num: OpNumber,
     reconfig_cmd: ReconfigCommand \cup {"null"}]
    \cup
    [type: {"PrepareOk"},
     replica: ReplicaId,
     view: ViewNumber,
     op_num: OpNumber]
    \cup
    [type: {"Commit"},
     view: ViewNumber,
     commit_num: OpNumber]
    \cup
    [type: {"DoViewChange"},
     replica: ReplicaId,
     view: ViewNumber,
     reconfig_state: ReconfigState,
     old_config: ClusterConfig,
     new_config: ClusterConfig,
     joint_op: OpNumber \cup {0}]
    \cup
    [type: {"StartView"},
     view: ViewNumber,
     reconfig_state: ReconfigState,
     old_config: ClusterConfig,
     new_config: ClusterConfig,
     joint_op: OpNumber \cup {0}]

-----------------------------------------------------------------------------

(* Initial State *)

Init ==
    /\ replicas = InitialReplicas
    /\ reconfig_state = [r \in InitialReplicas |-> "Stable"]
    /\ old_config = [r \in InitialReplicas |-> InitialReplicas]
    /\ new_config = [r \in InitialReplicas |-> {}]
    /\ joint_op = [r \in InitialReplicas |-> 0]
    /\ op_number = [r \in InitialReplicas |-> 0]
    /\ commit_number = [r \in InitialReplicas |-> 0]
    /\ view = [r \in InitialReplicas |-> 0]
    /\ committed_configs = {InitialReplicas}
    /\ messages = {}

-----------------------------------------------------------------------------

(* Helper Operators *)

\* Quorum size for a configuration
QuorumSize(config) == (Cardinality(config) \div 2) + 1

\* Check if a set forms a quorum in the given config
IsQuorum(voters, config) == Cardinality(voters) >= QuorumSize(config)

\* Check if we have quorum in joint consensus (need quorum in BOTH configs)
IsJointQuorum(voters, cfg_old, cfg_new) ==
    /\ IsQuorum(voters \cap cfg_old, cfg_old)
    /\ IsQuorum(voters \cap cfg_new, cfg_new)

\* Apply reconfiguration command to a config
ApplyReconfigCommand(cmd, current_config) ==
    CASE cmd.type = "AddReplica" ->
            current_config \cup {cmd.replica_id}
      [] cmd.type = "RemoveReplica" ->
            current_config \ {cmd.replica_id}
      [] cmd.type = "Replace" ->
            (current_config \ {cmd.old_id}) \cup {cmd.new_id}

\* Check if replica is in stable state
IsStable(r) == reconfig_state[r] = "Stable"

\* Check if replica is in joint consensus
IsJoint(r) == reconfig_state[r] = "Joint"

\* Current active config for a replica
CurrentConfig(r) ==
    IF IsStable(r) THEN old_config[r]
    ELSE old_config[r] \cup new_config[r]

-----------------------------------------------------------------------------

(* Actions *)

\* Leader proposes reconfiguration command
\* Transitions from Stable -> Joint
ProposeReconfiguration(leader, cmd) ==
    /\ IsStable(leader)
    /\ op_number[leader] < MaxOp
    /\ LET current_cfg == old_config[leader]
           new_cfg == ApplyReconfigCommand(cmd, current_cfg)
           new_op == op_number[leader] + 1
       IN  /\ new_cfg # current_cfg  \* Must actually change something
           /\ Cardinality(new_cfg) >= 3  \* Maintain minimum cluster size
           /\ Cardinality(new_cfg) <= MaxReplicas
           /\ reconfig_state' = [reconfig_state EXCEPT ![leader] = "Joint"]
           /\ old_config' = old_config  \* Keep old config
           /\ new_config' = [new_config EXCEPT ![leader] = new_cfg]
           /\ joint_op' = [joint_op EXCEPT ![leader] = new_op]
           /\ op_number' = [op_number EXCEPT ![leader] = new_op]
           /\ messages' = messages \cup {[
                type |-> "Prepare",
                view |-> view[leader],
                op_num |-> new_op,
                reconfig_cmd |-> cmd
              ]}
           /\ UNCHANGED <<replicas, commit_number, view, committed_configs>>

\* Backup receives Prepare with reconfiguration command
\* Transitions from Stable -> Joint
BackupProcessesPrepare(backup, prepare_msg) ==
    /\ prepare_msg \in messages
    /\ prepare_msg.type = "Prepare"
    /\ prepare_msg.reconfig_cmd # "null"
    /\ IsStable(backup)
    /\ view[backup] = prepare_msg.view
    /\ LET cmd == prepare_msg.reconfig_cmd
           current_cfg == old_config[backup]
           new_cfg == ApplyReconfigCommand(cmd, current_cfg)
       IN  /\ reconfig_state' = [reconfig_state EXCEPT ![backup] = "Joint"]
           /\ new_config' = [new_config EXCEPT ![backup] = new_cfg]
           /\ joint_op' = [joint_op EXCEPT ![backup] = prepare_msg.op_num]
           /\ op_number' = [op_number EXCEPT ![backup] = prepare_msg.op_num]
           /\ messages' = messages \cup {[
                type |-> "PrepareOk",
                replica |-> backup,
                view |-> view[backup],
                op_num |-> prepare_msg.op_num
              ]}
           /\ UNCHANGED <<replicas, old_config, commit_number, view, committed_configs>>

\* Leader commits operation after receiving quorum of PrepareOk
\* In joint consensus: requires quorum in BOTH old and new configs
CommitOperation(leader, op) ==
    /\ op <= op_number[leader]
    /\ op > commit_number[leader]
    /\ LET prepare_oks == {m \in messages :
                            /\ m.type = "PrepareOk"
                            /\ m.view = view[leader]
                            /\ m.op_num = op}
           voters == {m.replica : m \in prepare_oks} \cup {leader}
           has_quorum == IF IsJoint(leader)
                         THEN IsJointQuorum(voters, old_config[leader], new_config[leader])
                         ELSE IsQuorum(voters, old_config[leader])
       IN  /\ has_quorum
           /\ commit_number' = [commit_number EXCEPT ![leader] = op]
           /\ messages' = messages \cup {[
                type |-> "Commit",
                view |-> view[leader],
                commit_num |-> op
              ]}
           /\ UNCHANGED <<replicas, reconfig_state, old_config, new_config,
                          joint_op, op_number, view, committed_configs>>

\* Transition from Joint -> Stable after committing joint operation
\* Updates cluster config to new config
TransitionToStable(replica) ==
    /\ IsJoint(replica)
    /\ commit_number[replica] >= joint_op[replica]
    /\ joint_op[replica] > 0
    /\ LET cfg == new_config[replica]
       IN  /\ reconfig_state' = [reconfig_state EXCEPT ![replica] = "Stable"]
           /\ old_config' = [old_config EXCEPT ![replica] = cfg]
           /\ new_config' = [new_config EXCEPT ![replica] = {}]
           /\ joint_op' = [joint_op EXCEPT ![replica] = 0]
           /\ committed_configs' = committed_configs \cup {cfg}
           /\ replicas' = IF replica \in cfg THEN replicas ELSE replicas \ {replica}
           /\ UNCHANGED <<op_number, commit_number, view, messages>>

\* View change: Leader sends DoViewChange with reconfig state
SendDoViewChange(replica, new_view) ==
    /\ new_view > view[replica]
    /\ messages' = messages \cup {[
         type |-> "DoViewChange",
         replica |-> replica,
         view |-> new_view,
         reconfig_state |-> reconfig_state[replica],
         old_config |-> old_config[replica],
         new_config |-> new_config[replica],
         joint_op |-> joint_op[replica]
       ]}
    /\ view' = [view EXCEPT ![replica] = new_view]
    /\ UNCHANGED <<replicas, reconfig_state, old_config, new_config,
                   joint_op, op_number, commit_number, committed_configs>>

\* New leader receives quorum of DoViewChange, sends StartView
\* Extracts reconfig state from best DoViewChange
SendStartView(leader, v) ==
    /\ view[leader] = v
    /\ LET dvc_msgs == {m \in messages :
                         /\ m.type = "DoViewChange"
                         /\ m.view = v}
           voters == {m.replica : m \in dvc_msgs} \cup {leader}
           \* In stable state, use old_config. In joint, need both.
           leader_cfg == old_config[leader]
           has_quorum == IsQuorum(voters, leader_cfg)
           \* Pick DoViewChange with highest op_number (most up-to-date)
           best_dvc == CHOOSE m \in dvc_msgs :
                         \A other \in dvc_msgs : m.op_num >= other.op_num
       IN  /\ has_quorum
           /\ dvc_msgs # {}
           \* Restore reconfig state from best DoViewChange
           /\ reconfig_state' = [reconfig_state EXCEPT ![leader] = best_dvc.reconfig_state]
           /\ old_config' = [old_config EXCEPT ![leader] = best_dvc.old_config]
           /\ new_config' = [new_config EXCEPT ![leader] = best_dvc.new_config]
           /\ joint_op' = [joint_op EXCEPT ![leader] = best_dvc.joint_op]
           /\ messages' = messages \cup {[
                type |-> "StartView",
                view |-> v,
                reconfig_state |-> best_dvc.reconfig_state,
                old_config |-> best_dvc.old_config,
                new_config |-> best_dvc.new_config,
                joint_op |-> best_dvc.joint_op
              ]}
           /\ UNCHANGED <<replicas, op_number, commit_number, view, committed_configs>>

\* Backup receives StartView, restores reconfig state
ReceiveStartView(backup, sv_msg) ==
    /\ sv_msg \in messages
    /\ sv_msg.type = "StartView"
    /\ sv_msg.view >= view[backup]
    /\ view' = [view EXCEPT ![backup] = sv_msg.view]
    /\ reconfig_state' = [reconfig_state EXCEPT ![backup] = sv_msg.reconfig_state]
    /\ old_config' = [old_config EXCEPT ![backup] = sv_msg.old_config]
    /\ new_config' = [new_config EXCEPT ![backup] = sv_msg.new_config]
    /\ joint_op' = [joint_op EXCEPT ![backup] = sv_msg.joint_op]
    /\ UNCHANGED <<replicas, op_number, commit_number, committed_configs, messages>>

-----------------------------------------------------------------------------

(* State Machine *)

Next ==
    \/ \E r \in replicas, cmd \in ReconfigCommand : ProposeReconfiguration(r, cmd)
    \/ \E r \in replicas, m \in messages : BackupProcessesPrepare(r, m)
    \/ \E r \in replicas, op \in OpNumber : CommitOperation(r, op)
    \/ \E r \in replicas : TransitionToStable(r)
    \/ \E r \in replicas, v \in ViewNumber : SendDoViewChange(r, v)
    \/ \E r \in replicas, v \in ViewNumber : SendStartView(r, v)
    \/ \E r \in replicas, m \in messages : ReceiveStartView(r, m)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------

(* INVARIANTS *)

TypeOK ==
    /\ replicas \subseteq ReplicaId
    /\ reconfig_state \in [replicas -> ReconfigState]
    /\ old_config \in [replicas -> ClusterConfig]
    /\ new_config \in [replicas -> ClusterConfig]
    /\ joint_op \in [replicas -> (OpNumber \cup {0})]
    /\ op_number \in [replicas -> OpNumber]
    /\ commit_number \in [replicas -> OpNumber]
    /\ view \in [replicas -> ViewNumber]
    /\ committed_configs \subseteq ClusterConfig

\* Never have conflicting committed configurations
ConfigurationSafety ==
    \A c1, c2 \in committed_configs :
        c1 # c2 => c1 \cap c2 # {}  \* Configs must overlap

\* Any two quorums in any committed config must overlap
QuorumOverlap ==
    \A config \in committed_configs :
        \A q1, q2 \in SUBSET config :
            (IsQuorum(q1, config) /\ IsQuorum(q2, config))
            => q1 \cap q2 # {}

\* Joint consensus invariants
JointConsensusInvariants ==
    \A r \in replicas :
        /\ IsJoint(r) => new_config[r] # {}
        /\ IsJoint(r) => joint_op[r] > 0
        /\ IsStable(r) => new_config[r] = {}
        /\ IsStable(r) => joint_op[r] = 0

\* Commit number never exceeds operation number
CommitNumberBounded ==
    \A r \in replicas : commit_number[r] <= op_number[r]

\* View change preserves reconfig state
ViewChangePreservesReconfig ==
    \A m \in messages :
        /\ m.type = "DoViewChange" =>
            /\ (m.reconfig_state = "Joint") => m.new_config # {}
            /\ (m.reconfig_state = "Stable") => m.new_config = {}
        /\ m.type = "StartView" =>
            /\ (m.reconfig_state = "Joint") => m.new_config # {}
            /\ (m.reconfig_state = "Stable") => m.new_config = {}

-----------------------------------------------------------------------------

(* PROPERTIES *)

\* Progress: Joint consensus eventually becomes stable
Progress ==
    \A r \in replicas :
        [](IsJoint(r) => <>(IsStable(r)))

\* Liveness: Reconfiguration completes if no further failures
ReconfigurationCompletes ==
    \A r \in replicas :
        [](IsJoint(r) /\ commit_number[r] >= joint_op[r]
           => <>(IsStable(r)))

-----------------------------------------------------------------------------

(* THEOREMS *)

\* THEOREM ConfigSafety: Spec => []ConfigurationSafety
\* THEOREM QuorumInvariant: Spec => []QuorumOverlap
\* THEOREM JointInvariants: Spec => []JointConsensusInvariants
\* THEOREM ViewChangeCorrect: Spec => []ViewChangePreservesReconfig
\* THEOREM Liveness: Spec => Progress

=============================================================================
