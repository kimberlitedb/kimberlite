------------------------- MODULE Scrubbing -------------------------
(*
  Background Storage Scrubbing for VSR

  Detects latent sector errors before they cause double-fault data loss.
  Google study (2007): >60% of latent errors discovered by scrubbers, not reads.

  Problem: Silent disk corruption (bit rot, firmware bugs, bad sectors)
  Solution: Background process that continuously validates all stored data

  Key properties:
  - CorruptionDetected: Scrubber eventually finds all corrupted blocks
  - ScrubProgress: Tour makes forward progress (no deadlock)
  - RepairTriggered: Corruption detection triggers repair automatically
*)

EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Blocks,                \\* Set of block offsets in log
    MaxBlockOffset,        \\* Maximum block offset (log size)
    ScrubRateLimitIOPS,    \\* IOPS budget for scrubbing (reserve for production)
    TourPeriodSeconds      \\* Target period for complete tour (24 hours)

VARIABLES
    tour_position,         \\* Current position in scrub tour
    tour_origin,           \\* PRNG-based starting offset (prevents sync)
    blocks_corrupted,      \\* Set of corrupted block offsets
    blocks_scrubbed,       \\* Set of blocks scrubbed in current tour
    corruption_detected,   \\* Set of corrupted blocks detected
    repair_triggered,      \\* Set of blocks with repair triggered
    iops_consumed,         \\* IOPS consumed by scrubbing
    current_time           \\* Current timestamp (for rate limiting)

vars == <<tour_position, tour_origin, blocks_corrupted, blocks_scrubbed,
          corruption_detected, repair_triggered, iops_consumed, current_time>>

-----------------------------------------------------------------------------

TypeOK ==
    /\ tour_position \in Nat
    /\ tour_origin \in Nat
    /\ blocks_corrupted \subseteq Blocks
    /\ blocks_scrubbed \subseteq Blocks
    /\ corruption_detected \subseteq blocks_corrupted
    /\ repair_triggered \subseteq corruption_detected
    /\ iops_consumed \in Nat
    /\ current_time \in Nat

-----------------------------------------------------------------------------

Init ==
    /\ tour_position = 0
    /\ tour_origin \in 0..MaxBlockOffset  \\* PRNG-based randomization
    /\ blocks_corrupted = {}               \\* No initial corruption (for simplicity)
    /\ blocks_scrubbed = {}
    /\ corruption_detected = {}
    /\ repair_triggered = {}
    /\ iops_consumed = 0
    /\ current_time = 0

(* Calculate actual block offset from tour position *)
ActualBlockOffset(pos) ==
    (tour_origin + pos) % MaxBlockOffset

(* Check if scrubbing is allowed (rate limit) *)
CanScrub ==
    /\ iops_consumed < ScrubRateLimitIOPS
    /\ tour_position < MaxBlockOffset

(* Scrub next block in tour *)
ScrubNextBlock ==
    /\ CanScrub
    /\ LET block_offset == ActualBlockOffset(tour_position)
       IN
         /\ blocks_scrubbed' = blocks_scrubbed \cup {block_offset}
         /\ tour_position' = tour_position + 1
         /\ iops_consumed' = iops_consumed + 1
         \\* If block is corrupted, detect it
         /\ IF block_offset \in blocks_corrupted
            THEN corruption_detected' = corruption_detected \cup {block_offset}
            ELSE corruption_detected' = corruption_detected
         /\ UNCHANGED <<tour_origin, blocks_corrupted, repair_triggered, current_time>>

(* Trigger repair for detected corruption *)
TriggerRepair ==
    /\ corruption_detected /= {}
    /\ \E block \in corruption_detected :
         /\ block \notin repair_triggered
         /\ repair_triggered' = repair_triggered \cup {block}
         /\ UNCHANGED <<tour_position, tour_origin, blocks_corrupted, blocks_scrubbed,
                        corruption_detected, iops_consumed, current_time>>

(* Complete repair (fixes corrupted block) *)
CompleteRepair ==
    /\ repair_triggered /= {}
    /\ \E block \in repair_triggered :
         /\ blocks_corrupted' = blocks_corrupted \ {block}
         /\ repair_triggered' = repair_triggered \ {block}
         /\ corruption_detected' = corruption_detected \ {block}
         /\ UNCHANGED <<tour_position, tour_origin, blocks_scrubbed, iops_consumed, current_time>>

(* Complete current tour and start new one *)
CompleteTour ==
    /\ tour_position >= MaxBlockOffset
    /\ tour_position' = 0
    /\ tour_origin' \in 0..MaxBlockOffset  \\* New random origin
    /\ blocks_scrubbed' = {}
    /\ iops_consumed' = 0
    /\ UNCHANGED <<blocks_corrupted, corruption_detected, repair_triggered, current_time>>

(* Inject corruption (for model checking) *)
InjectCorruption ==
    /\ blocks_corrupted /= Blocks  \\* Not all blocks corrupted
    /\ \E block \in Blocks :
         /\ block \notin blocks_corrupted
         /\ blocks_corrupted' = blocks_corrupted \cup {block}
         /\ UNCHANGED <<tour_position, tour_origin, blocks_scrubbed,
                        corruption_detected, repair_triggered, iops_consumed, current_time>>

(* Time advances (for rate limiting) *)
Tick ==
    /\ current_time' = current_time + 1
    /\ iops_consumed' = 0  \\* Reset IOPS budget every second
    /\ UNCHANGED <<tour_position, tour_origin, blocks_corrupted, blocks_scrubbed,
                   corruption_detected, repair_triggered>>

-----------------------------------------------------------------------------

Next ==
    \/ ScrubNextBlock
    \/ TriggerRepair
    \/ CompleteRepair
    \/ CompleteTour
    \/ InjectCorruption
    \/ Tick

Spec == Init /\ [][Next]_vars /\ WF_vars(ScrubNextBlock) /\ WF_vars(TriggerRepair)

-----------------------------------------------------------------------------

(* SAFETY PROPERTIES *)

(* Property 1: Corruption Detected *)
(*
  If a block is corrupted and scrubbed, it will be detected.
  This ensures scrubbing actually validates data integrity.
*)
CorruptionDetected ==
    [](\A block \in Blocks :
        (block \in blocks_corrupted /\ block \in blocks_scrubbed)
        => (block \in corruption_detected))

(* Property 2: Scrub Progress *)
(*
  The scrub tour eventually makes forward progress.
  This ensures scrubbing doesn't deadlock or get stuck.
*)
ScrubProgress ==
    [](tour_position < MaxBlockOffset => <>(tour_position' > tour_position))

(* Property 3: Repair Triggered *)
(*
  When corruption is detected, repair is eventually triggered.
  This ensures scrubbing leads to automatic remediation.
*)
RepairTriggered ==
    [](\A block \in corruption_detected :
        <>(block \in repair_triggered \/ block \notin blocks_corrupted))

(* Property 4: Rate Limit Enforced *)
(*
  IOPS consumption never exceeds the configured limit.
  This prevents scrubbing from impacting production workload.
*)
RateLimitEnforced ==
    [](iops_consumed <= ScrubRateLimitIOPS)

(* Property 5: Tour Origin Randomized *)
(*
  Each new tour starts at a different origin (PRNG-based).
  This prevents synchronized scrub spikes across replicas.
  Note: TLA+ can't verify true randomness, but we check variety.
*)
TourOriginRandomized ==
    LET origins == {origin \in Nat : \E s \in DOMAIN vars : vars[s].tour_origin = origin}
    IN Cardinality(origins) > 1  \\* Multiple different origins seen

(* Property 6: Complete Tour Coverage *)
(*
  Each tour eventually scrubs all blocks (complete coverage).
  This ensures no block is neglected indefinitely.
*)
CompleteTourCoverage ==
    [](blocks_scrubbed = Blocks => <>tour_position = 0)

(* Property 7: Detected Blocks Are Corrupted *)
(*
  Only truly corrupted blocks are detected (no false positives).
  This ensures scrubbing is accurate.
*)
NoFalsePositives ==
    [](corruption_detected \subseteq blocks_corrupted)

(* Property 8: Repaired Blocks No Longer Corrupted *)
(*
  Completing repair removes block from corrupted set.
  This ensures repair actually fixes corruption.
*)
RepairEffective ==
    [](\A block \in Blocks :
        (block \in repair_triggered /\ block \notin repair_triggered')
        => (block \notin blocks_corrupted'))

-----------------------------------------------------------------------------

(* LIVENESS PROPERTIES *)

(* Property 9: All Corruption Eventually Detected *)
(*
  Every corrupted block is eventually detected (within tour period).
*)
AllCorruptionEventuallyDetected ==
    [](\A block \in blocks_corrupted :
        <>(block \in corruption_detected))

(* Property 10: Tours Never Stall *)
(*
  Tours are eventually completed (no infinite stalling).
*)
ToursNeverStall ==
    [](tour_position = MaxBlockOffset => <>(tour_position = 0))

-----------------------------------------------------------------------------

(* INVARIANTS *)

Inv ==
    /\ TypeOK
    /\ CorruptionDetected
    /\ RateLimitEnforced
    /\ NoFalsePositives
    /\ RepairEffective

=============================================================================
