Help me to incorporate testing of the leader3_2 prism model with the leader.prop file

You also need to include syntactic sugar for F<=t expr which desugars into (true U<=t expr)

Fix any bugs you may find and ensure that all the tests still continue to pass.

below is the output from the official prism tool

prism/bin/prism prism-examples/dtmcs/leader_sync/leader3_2.pm   -mtbdd prism-examples/dtmcs/leader_sync/leader.pctl -const L=3

PRISM-games
===========

Version: 3.2.1 (based on PRISM 4.8.1.dev)
Date: Wed Apr 15 12:10:04 BST 2026
*** CUSTOM BUILD - CHANGES ACTIVE ***
Hostname: nixos
Memory limits: cudd=1g, java(heap)=1g
Command line: prism-games prism-examples/dtmcs/leader_sync/leader3_2.pm -mtbdd prism-examples/dtmcs/leader_sync/leader.pctl -const L=3

Parsing PRISM model file "prism-examples/dtmcs/leader_sync/leader3_2.pm"...

Type:        DTMC
Modules:     counter process1 process2 process3
Variables:   c s1 u1 v1 p1 s2 u2 v2 p2 s3 u3 v3 p3
Labels:      "elected"
Rewards:     "num_rounds"

Parsing properties file "prism-examples/dtmcs/leader_sync/leader.pctl"...

3 properties:
(1) P=? [ F "elected" ]
(2) P=? [ F<=(L*(N+1)) "elected" ]
(3) R{"num_rounds"}=? [ F "elected" ]

---------------------------------------------------------------------

Model checking: P=? [ F "elected" ]

Building model (engine:symbolic)...

Translating modules to MTBDD...

Computing reachable states...

Reachability (BFS): 5 iterations in 0.00 seconds (average 0.000000, setup 0.00)

Time for model construction: 0.011 seconds.

Type:        DTMC
States:      26 (1 initial)
Transitions: 33

Transition matrix: 408 nodes (3 terminal), 33 minterms, vars: 16r/16c

Prob0: 8 iterations in 0.01 seconds (average 0.001250, setup 0.00)

Prob1: 1 iterations in 0.00 seconds (average 0.000000, setup 0.00)

yes = 26, no = 0, maybe = 0

Value in the initial state: 1.0

Time for model checking: 0.003 seconds.

Result: 1.0 (exact floating point)

---------------------------------------------------------------------

Model checking: P=? [ F<=(L*(N+1)) "elected" ]
Property constants: L=3

Prob0: 8 iterations in 0.00 seconds (average 0.000000, setup 0.00)

yes = 1, no = 0, maybe = 25

Computing probabilities...
Engine: MTBDD

Building iteration matrix MTBDD... [nodes=388] [7.6 Kb]

Starting iterations...

Iterative method: 12 iterations in 0.00 seconds (average 0.000000, setup 0.00)

Value in the initial state: 0.984375

Time for model checking: 0.01 seconds.

Result: 0.984375 (exact floating point)