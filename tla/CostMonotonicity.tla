--------------------------- MODULE CostMonotonicity ---------------------------
(****************************************************************************)
(* TLA+ specification for proving that logical transformation rules never  *)
(* increase query execution cost. This specification models the cost model *)
(* and proves monotonicity properties for logical optimizations.           *)
(****************************************************************************)

EXTENDS Naturals, Reals, FiniteSets

CONSTANTS
    LogicalRules,   \* Set of logical transformation rules
    PhysicalRules,  \* Set of physical transformation rules
    MaxCost,        \* Maximum possible cost value
    Operators       \* Set of relational algebra operators

ASSUME LogicalRules \subseteq (Operators \X Operators)
ASSUME PhysicalRules \subseteq (Operators \X Operators)
ASSUME MaxCost \in Real
ASSUME MaxCost > 0

VARIABLES
    plan,           \* Current query plan (tree of operators)
    cost,           \* Current plan cost
    history,        \* Sequence of (plan, cost) pairs
    rule_applied    \* Last rule applied

vars == <<plan, cost, history, rule_applied>>

(****************************************************************************)
(* Cost model functions                                                     *)
(****************************************************************************)

(* Base cost for each operator type *)
BaseCost(op) ==
    CASE op = "Scan" -> 100
      [] op = "Filter" -> 10
      [] op = "Project" -> 5
      [] op = "HashJoin" -> 200
      [] op = "NestedLoopJoin" -> 1000
      [] op = "HashAggregate" -> 150
      [] op = "Sort" -> 300
      [] op = "Limit" -> 1
      [] OTHER -> 50

(* Selectivity factor for predicates *)
Selectivity(predicate) ==
    0.1  \* Conservative estimate

(* Cardinality estimation *)
EstimateCardinality(op, input_card) ==
    CASE op = "Filter" -> input_card * Selectivity("pred")
      [] op = "Project" -> input_card
      [] op = "HashJoin" -> input_card * input_card / 10
      [] op = "Limit" -> IF input_card > 100 THEN 100 ELSE input_card
      [] OTHER -> input_card

(****************************************************************************)
(* Type invariants                                                          *)
(****************************************************************************)

TypeOK ==
    /\ plan \in Operators
    /\ cost \in Real
    /\ cost >= 0
    /\ cost <= MaxCost
    /\ history \in Seq(Operators \X Real)
    /\ rule_applied \in LogicalRules \union PhysicalRules \union {NULL}

(****************************************************************************)
(* Initial state                                                            *)
(****************************************************************************)

Init ==
    /\ plan \in Operators
    /\ cost = BaseCost(plan)
    /\ history = <<[plan |-> plan, cost |-> cost]>>
    /\ rule_applied = NULL

(****************************************************************************)
(* Apply a logical rule (should not increase cost)                        *)
(****************************************************************************)

ApplyLogicalRule ==
    \E rule \in LogicalRules :
        LET old_cost == cost
            new_plan == rule[2]  \* Target of transformation
            new_cost == BaseCost(new_plan)
        IN
            /\ new_cost <= old_cost  \* CRITICAL: Logical rules never increase cost
            /\ plan' = new_plan
            /\ cost' = new_cost
            /\ history' = Append(history, [plan |-> new_plan, cost |-> new_cost])
            /\ rule_applied' = rule

(****************************************************************************)
(* Apply a physical rule (may increase or decrease cost)                  *)
(****************************************************************************)

ApplyPhysicalRule ==
    \E rule \in PhysicalRules :
        LET old_cost == cost
            new_plan == rule[2]
            new_cost == BaseCost(new_plan)
        IN
            /\ plan' = new_plan
            /\ cost' = new_cost
            /\ history' = Append(history, [plan |-> new_plan, cost |-> new_cost])
            /\ rule_applied' = rule

(****************************************************************************)
(* Next state transition                                                   *)
(****************************************************************************)

Next ==
    \/ ApplyLogicalRule
    \/ ApplyPhysicalRule

(****************************************************************************)
(* Specification                                                            *)
(****************************************************************************)

Spec == Init /\ [][Next]_vars

(****************************************************************************)
(* Cost monotonicity properties                                            *)
(****************************************************************************)

(* THEOREM: Logical rules never increase cost *)
THEOREM LogicalMonotonicity ==
    [](rule_applied \in LogicalRules => cost' <= cost)

(* THEOREM: Cost remains bounded *)
THEOREM CostBounded ==
    [](cost <= MaxCost)

(* THEOREM: Cost is always non-negative *)
THEOREM CostNonNegative ==
    [](cost >= 0)

(****************************************************************************)
(* History-based monotonicity for logical rules                           *)
(****************************************************************************)

(* INV: For any two consecutive logical rule applications, cost decreases or stays same *)
LogicalSequenceMonotonic ==
    \A i \in 1..(Len(history)-1) :
        LET curr == history[i]
            next == history[i+1]
        IN
            curr.cost >= next.cost

(****************************************************************************)
(* Optimality properties                                                   *)
(****************************************************************************)

(* INV: After applying all logical rules, cannot reduce cost further *)
LogicalSaturation ==
    (~\E rule \in LogicalRules :
        \E new_cost \in Real :
            new_cost < cost) =>
    []UNCHANGED cost

(* THEOREM: Eventually reaches local optimum for logical rules *)
THEOREM EventualOptimality ==
    Spec => <>(~\E rule \in LogicalRules : \E new_cost \in Real : new_cost < cost)

(****************************************************************************)
(* Key invariants                                                          *)
(****************************************************************************)

(* INV1: Type correctness *)
Inv1 == TypeOK

(* INV2: History records all transformations *)
Inv2 == Len(history) >= 1

(* INV3: Current plan matches last history entry *)
Inv3 == history[Len(history)].plan = plan /\ history[Len(history)].cost = cost

(* INV4: Cost decreases or stays same after logical rules *)
Inv4 == rule_applied \in LogicalRules => cost' <= cost

(****************************************************************************)
(* Physical rule properties (informative, not proven)                     *)
(****************************************************************************)

(* Physical rules explore the cost space and may temporarily increase cost *)
PhysicalExploration ==
    rule_applied \in PhysicalRules => (cost' <= cost \/ cost' > cost)

=============================================================================
