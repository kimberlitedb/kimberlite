---
title: "TLC Caught Our First Consensus Bug: Why We Do Formal Verification"
slug: "tlc-caught-our-first-consensus-bug"
date: 2026-02-05
excerpt: "TLC model checking found a subtle Agreement violation in our VSR specification—before we implemented it. Here's the bug, the fix, and why formal verification is worth the investment."
author_name: "Jared Reyes"
author_avatar: "/public/images/jared-avatar.jpg"
---

# TLC Caught Our First Consensus Bug: Why We Do Formal Verification

We've been formally verifying Kimberlite's VSR consensus protocol with TLA+ and TLC model checking. Today, TLC found its first bug—a subtle violation of the Agreement invariant that could cause replicas to commit **different values at the same position**.

This is exactly the kind of bug that formal verification is meant to catch: subtle, hard to test, and catastrophic if it reaches production. Here's the story of the bug, how TLC found it, and why we're grateful we caught it at the specification level.

## The Smoking Gun

```
TLC2 Version 2.19
Model checking in progress...

Error: Invariant Agreement is violated.
Error: The behavior up to this point is:

State 1: <Initial predicate>
State 2: <LeaderPrepare>
State 3: <FollowerOnPrepare>
State 4: <LeaderOnPrepareOkQuorum>  # r1 commits operation 1
State 5: <StartViewChange>
...
State 13: <LeaderOnDoViewChangeQuorum>  # New leader chooses WRONG log
...
State 18: <LeaderOnPrepareOkQuorum>  # r3 commits DIFFERENT operation 1
```

TLC found a trace where:
- Replica r1 committed operation 1 with `view=0`
- View change happened to `view=1`
- New leader r2 chose an **empty log**, discarding the committed operation
- Leader r2 prepared a **new** operation 1 with `view=1`
- Replica r3 committed this **different** operation 1

**Result**: Two replicas have different values at position 1. Agreement violated! 🚨

## What is the Agreement Invariant?

In consensus, the **Agreement** property is fundamental:

> If two replicas commit a value at the same log position, they must commit the **same** value.

This is what makes distributed consensus useful. Without Agreement, you can't trust that replicas have the same data. It's the difference between "distributed database" and "distributed chaos."

In TLA+, we express this as:

```tla
Agreement ==
    \A r1, r2 \in Replicas, op \in OpNumber :
        (op <= commitNumber[r1] /\ op <= commitNumber[r2] /\ op > 0) =>
            (op <= Len(log[r1]) /\ op <= Len(log[r2]) =>
                EntriesEqual(log[r1][op], log[r2][op]))
```

If both replicas have committed operation `op`, their log entries at position `op` must be equal (same view, same command, same checksum).

## The Bug: Two Issues in View Change

TLC's counterexample revealed **two** distinct bugs:

### Bug #1: Incorrect Log Selection

**The Code** (lines 294-299 of `VSR.tla`):

```tla
\* Find log with highest op number
mostRecentLog == CHOOSE dvc \in doVCs :
    \A other \in doVCs : dvc.opNum >= other.opNum

\* Find highest commit number
maxCommit == CHOOSE c \in {dvc.commitNum : dvc \in doVCs} :
    \A other \in {dvc.commitNum : dvc \in doVCs} : c >= other
```

**The Problem**: The leader chose:
1. The log with the **highest opNum** (most operations)
2. The **max commitNum** from any replica

But these could come from **different** replicas!

**Scenario that breaks**:
- Replica A: `opNum=5, commitNum=0, log=[e1@v0, e2@v0, ..., e5@v1]` (has operations, didn't process Commit)
- Replica B: `opNum=1, commitNum=1, log=[e1@v0]` (fewer operations, saw Commit message)
- Leader incorrectly chooses: Replica A's log (highest opNum) + commitNum=1
- But Replica A's entry at position 1 might be **different** from Replica B's!

### Bug #2: StartView Re-Processing

**The Code** (line 358):

```tla
FollowerOnStartView(r, msg) ==
    /\ msg \in messages
    /\ msg.type = "StartView"
    /\ msg.view >= view[r]  \* WRONG: allows re-processing
    /\ ...
    /\ log' = [log EXCEPT ![r] = msg.replicaLog]
```

**The Problem**: The condition `msg.view >= view[r]` allows a replica to process StartView for the **same view** it's already in. This overwrites the log with potentially stale data!

**Scenario**:
1. Replica r3 receives StartView for view=1, transitions to Normal
2. Leader prepares new operations, r3 receives them
3. r3 receives the **old** StartView message again (still in message set)
4. r3 processes it and **overwrites** its log with the empty one from the StartView
5. View change happens, r3 becomes leader with empty log
6. Agreement violated!

## The Fix: Learn from TigerBeetle

We looked at [TigerBeetle's VSR implementation](https://github.com/tigerbeetle/tigerbeetle) to see how they handle view changes correctly.

### Fix #1: Use `log_view` for Canonicalization

TigerBeetle uses **`log_view`**: the view in which a replica's log was last updated. During view changes:

1. Find the **highest `log_view`** across all DoViewChange messages
2. Filter to "canonical" logs (those with `log_view == max`)
3. Among canonical logs, choose the one with **highest opNum**
4. Set `commitNum` to the max across **all** replicas

**The Corrected Code**:

```tla
\* Helper: Get the log_view (view of highest entry, or 0 if empty)
LogView(dvc) == IF dvc.opNum > 0 /\ Len(dvc.replicaLog) > 0
                THEN dvc.replicaLog[Len(dvc.replicaLog)].view
                ELSE 0

\* Find maximum log_view (canonical view)
maxLogView == CHOOSE lv \in {LogView(dvc) : dvc \in allDvcs} :
    \A other \in {LogView(dvc) : dvc \in allDvcs} : lv >= other

\* Filter to canonical DVCs (those with max log_view)
canonicalDvcs == {dvc \in allDvcs : LogView(dvc) = maxLogView}

\* Among canonical DVCs, choose the one with highest op number
mostRecentLog == CHOOSE dvc \in canonicalDvcs :
    \A other \in canonicalDvcs : dvc.opNum >= other.opNum

\* Find maximum commit number across ALL replicas
maxCommit == CHOOSE c \in {dvc.commitNum : dvc \in allDvcs} :
    \A other \in {dvc.commitNum : dvc \in allDvcs} : c >= other
```

**Why this works**:
- `log_view` tracks which view a log was last updated in
- Logs with `log_view < max` have been superseded by view changes
- Only logs with `log_view = max` are "canonical" (current)
- Among canonical logs, the one with highest opNum has the most prepared operations
- By quorum intersection, at least one canonical log contains all committed operations

### Fix #2: Guard StartView Processing

**The Fix**:

```tla
FollowerOnStartView(r, msg) ==
    /\ msg \in messages
    /\ msg.type = "StartView"
    \* Only process StartView if:
    \* 1. It's for a newer view (msg.view > view[r]), OR
    \* 2. It's for the current view AND we're in ViewChange status
    /\ (msg.view > view[r] \/ (msg.view = view[r] /\ status[r] = "ViewChange"))
    /\ ...
```

**Why this works**:
- Replicas in Normal status won't re-process StartView for the same view
- Replicas in ViewChange status can still transition to Normal for the same view
- Prevents log overwrites from stale messages

## Verification Results

After the fixes, TLC model checking passes with no violations:

```
Model checking completed. No error has been found.
45,102 states generated, 23,879 distinct states found, 0 states left on queue.
The depth of the complete state graph search is 27.

All 6 safety invariants passed:
✅ TypeOK
✅ CommitNotExceedOp
✅ ViewMonotonic
✅ LeaderUniquePerView
✅ Agreement            # The critical one!
✅ PrefixConsistency
```

## Why This Matters

This bug demonstrates the **value of formal verification**:

### 1. Found Before Implementation

We caught this bug at the **specification level**, before writing any production code. Fixing a spec is a few lines of TLA+. Fixing production code after discovering data corruption would be:
- Emergency incident response
- Customer data analysis
- Patch deployment
- Reputation damage

### 2. Subtle Consensus Bug

This bug involves:
- Subtle interaction between view change and commit protocols
- Edge case requiring specific timing (committed op + view change + message reordering)
- Quorum intersection reasoning
- Agreement violation that would manifest rarely

Traditional testing would likely **never** catch this. Even VOPR (our deterministic simulator) might miss it due to the specific state space required.

### 3. Textbook Verification Win

This is a **textbook example** of why databases need formal verification:
- The protocol is complex (VSR view changes)
- The bug is subtle (requires understanding log_view semantics)
- The consequences are catastrophic (Agreement violation = data corruption)
- The fix is straightforward once identified

### 4. Builds Confidence

Knowing that TLC has exhaustively explored 45,000+ states and found **no Agreement violations** gives us confidence that the protocol is correct. Not just "probably works," but **proven correct** for the state space explored.

## Lessons Learned

### 1. Formal Verification Works

We invested time in:
- Writing TLA+ specifications
- Setting up TLC model checking
- Learning TLAPS for proofs
- Documenting invariants

That investment **paid off** by catching a critical bug before implementation.

### 2. Learn from Production Systems

When our spec failed, we looked at **TigerBeetle's implementation** to understand the correct algorithm. They've run billions of transactions in production—learning from them is smart engineering.

### 3. State Space Matters

With the full configuration (`MaxView=3, MaxOp=4`), TLC was exploring 31M+ states and taking too long. We reduced parameters (`MaxView=2, MaxOp=2`) to get fast feedback (45K states in 1 second).

For thorough verification, we'll run overnight with larger state spaces.

### 4. Multiple Verification Layers

We're using multiple verification approaches:
- **TLC**: Bounded model checking (exhaustive for small state spaces)
- **TLAPS**: Unbounded mechanical proofs (covers all cases, but harder)
- **VOPR**: Deterministic simulation testing (tests implementation, not spec)
- **Property testing**: QuickCheck-style testing with random inputs

Each catches different classes of bugs.

## What's Next

### 1. Update Implementation

Our Rust VSR implementation needs to be reviewed against the corrected spec. We expect it to be correct (we followed TigerBeetle's approach), but we'll verify carefully.

### 2. Add Test Cases

We'll add specific test cases to VOPR based on TLC's counterexample:
- Committed operation + view change
- StartView message reordering
- View change with mismatched commitNum

### 3. Deeper Verification

Run TLC with larger parameters:
- `MaxView=4, MaxOp=6` overnight (~1M states)
- Full cluster (`Replicas=5`) for production realism

### 4. TLAPS Proofs

We have TLAPS proofs in `VSR_Proofs.tla` that need updating for the corrected algorithm. The proofs will provide **unbounded** verification—true for all possible states, not just the ones TLC explored.

## Conclusion

Formal verification found a critical consensus bug before we implemented it. The bug was subtle, the fix was clear, and we're now confident in our protocol.

This is exactly why we invest in formal methods. Consensus is hard. Data corruption is catastrophic. Proving correctness upfront is worth every hour spent on TLA+.

If you're building a distributed system and not using formal verification, you're taking unnecessary risks. The bugs are out there, waiting. TLC will find them. Better it finds them in your spec than your customers find them in production.

---

**Code**: The fixes are in [`specs/tla/VSR.tla`](https://github.com/kimberlite/kimberlite/blob/main/specs/tla/VSR.tla)
**Changelog**: See [`CHANGELOG.md`](https://github.com/kimberlite/kimberlite/blob/main/CHANGELOG.md) for full details
**Learn More**: [Formal Verification Docs](https://docs.kimberlite.io/internals/testing/formal-verification)

*Want to see more? Follow along as we build Kimberlite: [kimberlite.io](https://kimberlite.io)*
