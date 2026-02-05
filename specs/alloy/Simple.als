-- Simple Alloy test spec
module simple

sig Node {
    edge: set Node
}

-- No cycles
assert Acyclic {
    no n: Node | n in n.^edge
}

check Acyclic for 5

run {} for 3
