/**
 * Kimberlite Hash Chain Integrity Model (CI-quick variant)
 *
 * Same properties as HashChain.als but with scope 5 instead of 10
 * for faster model checking in CI (scope 10 can take 15+ minutes).
 * The full HashChain.als is the authoritative specification.
 */

module kimberlite/HashChain_Quick

sig Hash {}

sig Data {}

sig LogEntry {
    position: Int,
    previous: lone LogEntry,
    hash: Hash,
    data: Data,
    checksum: Int
} {
    position >= 0
    position = 0 <=> no previous
    position > 0 => one previous
}

fact PositionOrder {
    all e: LogEntry | some e.previous => e.previous.position = e.position.minus[1]
}

pred hashFunction {
    all e: LogEntry | some e.previous => {
        no e2: LogEntry | e != e2 and e.previous = e2.previous and e.data = e2.data
            and e.hash != e2.hash
    }
}

pred wellFormedChain {
    one e: LogEntry | e.position = 0
    all n: Int | n >= 0 and n < #LogEntry => one e: LogEntry | e.position = n
    hashFunction
}

assert NoCycles {
    wellFormedChain =>
        no e: LogEntry | e in e.^previous
}

assert UniqueChain {
    wellFormedChain =>
        all e1, e2: LogEntry | e1 != e2 => e1.previous != e2.previous or no e1.previous
}

assert NoOrphans {
    wellFormedChain =>
        all e: LogEntry | e.position > 0 => one e.previous
}

assert FullyConnected {
    wellFormedChain => {
        let genesis = {e: LogEntry | e.position = 0} |
            all e: LogEntry | e in genesis.*~previous
    }
}

assert PositionMonotonic {
    wellFormedChain =>
        all e: LogEntry | some e.previous => e.position > e.previous.position
}

assert UniquePositions {
    wellFormedChain =>
        all e1, e2: LogEntry | e1.position = e2.position => e1 = e2
}

-- Use scope 5 for CI speed (vs scope 10 in HashChain.als)
check NoCycles for 5
check UniqueChain for 5
check NoOrphans for 5
check FullyConnected for 5
check PositionMonotonic for 5
check UniquePositions for 5
