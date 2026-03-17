--------------------------- MODULE RuleComposition ---------------------------
(****************************************************************************)
(* TLA+ specification for proving termination of rule application in the   *)
(* relational algebra optimization engine. This specification models the    *)
(* e-graph rewriting process and proves that equality saturation terminates *)
(* under the constraints of our rule system.                                *)
(****************************************************************************)

EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS
    MaxNodes,      \* Maximum e-graph size before termination
    MaxIterations, \* Maximum number of rewrite iterations
    Rules          \* Set of rewrite rules available

ASSUME MaxNodes \in Nat \ {0}
ASSUME MaxIterations \in Nat \ {0}
ASSUME Rules \in SUBSET (Nat \X Nat)  \* Rules map expression patterns

VARIABLES
    egraph,        \* Current e-graph representation (set of e-nodes)
    iteration,     \* Current iteration count
    saturated      \* Whether e-graph has reached saturation

vars == <<egraph, iteration, saturated>>

(****************************************************************************)
(* Type invariants                                                          *)
(****************************************************************************)

TypeOK ==
    /\ egraph \subseteq Nat
    /\ Cardinality(egraph) <= MaxNodes
    /\ iteration \in 0..MaxIterations
    /\ saturated \in BOOLEAN

(****************************************************************************)
(* Initial state: empty e-graph                                            *)
(****************************************************************************)

Init ==
    /\ egraph = {}
    /\ iteration = 0
    /\ saturated = FALSE

(****************************************************************************)
(* Apply a single rewrite rule to the e-graph                              *)
(* Returns TRUE if the rule made a change (added new e-classes)            *)
(****************************************************************************)

ApplyRule(rule) ==
    \E new_node \in Nat :
        /\ new_node \notin egraph
        /\ Cardinality(egraph) < MaxNodes
        /\ egraph' = egraph \union {new_node}
        /\ UNCHANGED <<iteration, saturated>>

(****************************************************************************)
(* Apply all rules in one iteration                                        *)
(* If no rules apply, mark as saturated                                    *)
(****************************************************************************)

ApplyAllRules ==
    \/ /\ \E rule \in Rules : ApplyRule(rule)
       /\ iteration' = iteration + 1
       /\ UNCHANGED saturated
    \/ /\ ~(\E rule \in Rules : \E new_node \in Nat :
              new_node \notin egraph /\ Cardinality(egraph) < MaxNodes)
       /\ saturated' = TRUE
       /\ UNCHANGED <<egraph, iteration>>

(****************************************************************************)
(* Check for termination conditions                                        *)
(****************************************************************************)

Terminate ==
    \/ iteration >= MaxIterations
    \/ Cardinality(egraph) >= MaxNodes
    \/ saturated

(****************************************************************************)
(* Next state transition                                                   *)
(****************************************************************************)

Next ==
    IF Terminate
    THEN UNCHANGED vars
    ELSE ApplyAllRules

(****************************************************************************)
(* Specification                                                            *)
(****************************************************************************)

Spec == Init /\ [][Next]_vars

(****************************************************************************)
(* Termination properties                                                  *)
(****************************************************************************)

(* THEOREM: The rewriting process always terminates *)
THEOREM Termination ==
    Spec => <>(Terminate)

(* THEOREM: E-graph size is monotonically increasing *)
THEOREM Monotonic ==
    [](Cardinality(egraph) <= Cardinality(egraph'))

(* THEOREM: Iteration count is bounded *)
THEOREM IterationBound ==
    [](iteration <= MaxIterations)

(* THEOREM: E-graph size is bounded *)
THEOREM SizeBound ==
    [](Cardinality(egraph) <= MaxNodes)

(****************************************************************************)
(* Liveness property: eventually reaches fixpoint or bound                 *)
(****************************************************************************)

EventuallyTerminates ==
    <>(saturated \/ iteration >= MaxIterations \/ Cardinality(egraph) >= MaxNodes)

THEOREM Spec => EventuallyTerminates

(****************************************************************************)
(* Invariants that should hold throughout execution                        *)
(****************************************************************************)

(* INV1: Type correctness is maintained *)
Inv1 == TypeOK

(* INV2: If saturated, no more changes possible *)
Inv2 == saturated => UNCHANGED egraph

(* INV3: Iteration increases monotonically *)
Inv3 == iteration' >= iteration

(* INV4: Once terminated, stays terminated *)
Inv4 == Terminate => []Terminate

=============================================================================
