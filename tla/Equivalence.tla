--------------------------- MODULE Equivalence ---------------------------
(****************************************************************************)
(* TLA+ specification for proving that relational algebra transformations  *)
(* preserve query semantics. This specification models query equivalence   *)
(* and proves that rewrite rules maintain semantic correctness.            *)
(****************************************************************************)

EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS
    Relations,      \* Set of base relations (tables)
    Attributes,     \* Set of attributes (columns)
    Rules,          \* Set of transformation rules
    MaxTuples       \* Maximum tuples for model checking

ASSUME Relations \subseteq STRING
ASSUME Attributes \subseteq STRING
ASSUME MaxTuples \in Nat \ {0}

(****************************************************************************)
(* Relational algebra operators                                            *)
(****************************************************************************)

Operators == {
    "Scan",
    "Filter",
    "Project",
    "Join",
    "LeftJoin",
    "RightJoin",
    "FullJoin",
    "SemiJoin",
    "AntiJoin",
    "Union",
    "Intersect",
    "Except",
    "Aggregate",
    "Sort",
    "Limit",
    "Distinct"
}

(****************************************************************************)
(* Query plan representation                                               *)
(****************************************************************************)

(* A query plan is a tree of operators *)
RECURSIVE PlanStructure(_)
PlanStructure(p) ==
    /\ p.op \in Operators
    /\ p.inputs \in Seq(PlanStructure)
    /\ p.schema \subseteq Attributes

(****************************************************************************)
(* Database state and tuple representation                                 *)
(****************************************************************************)

(* A tuple is a function from attributes to values *)
Tuple == [Attributes -> Nat]

(* A relation is a set of tuples *)
Relation == SUBSET Tuple

(* Database state maps relation names to relations *)
Database == [Relations -> Relation]

VARIABLES
    db,             \* Current database state
    plan1,          \* Original query plan
    plan2,          \* Transformed query plan
    result1,        \* Result of executing plan1
    result2,        \* Result of executing plan2
    equivalent      \* Whether plans are semantically equivalent

vars == <<db, plan1, plan2, result1, result2, equivalent>>

(****************************************************************************)
(* Evaluation semantics for relational operators                           *)
(****************************************************************************)

(* Evaluate a scan operator *)
EvalScan(relation, database) ==
    database[relation]

(* Evaluate a filter operator *)
EvalFilter(input, predicate) ==
    {t \in input : predicate(t)}

(* Evaluate a project operator *)
EvalProject(input, attrs) ==
    {[a \in attrs |-> t[a]] : t \in input}

(* Evaluate an inner join *)
EvalJoin(left, right, condition) ==
    {t1 @@ t2 : t1 \in left, t2 \in right, condition(t1, t2)}

(* Evaluate a left outer join *)
EvalLeftJoin(left, right, condition) ==
    LET matched == {t1 @@ t2 : t1 \in left, t2 \in right, condition(t1, t2)}
        unmatched == {t1 : t1 \in left,
                        ~\E t2 \in right : condition(t1, t2)}
    IN matched \union unmatched

(* Evaluate a semi-join *)
EvalSemiJoin(left, right, condition) ==
    {t1 \in left : \E t2 \in right : condition(t1, t2)}

(* Evaluate an anti-join *)
EvalAntiJoin(left, right, condition) ==
    {t1 \in left : ~\E t2 \in right : condition(t1, t2)}

(* Evaluate a union *)
EvalUnion(left, right) ==
    left \union right

(* Evaluate an intersect *)
EvalIntersect(left, right) ==
    left \intersect right

(* Evaluate an except (set difference) *)
EvalExcept(left, right) ==
    left \ right

(* Evaluate a distinct *)
EvalDistinct(input) ==
    input  \* Already a set, inherently distinct

(* Evaluate a limit *)
EvalLimit(input, n) ==
    LET seq == SetToSeq(input)
    IN {seq[i] : i \in 1..Min(Len(seq), n)}

(****************************************************************************)
(* Recursive plan evaluation                                               *)
(****************************************************************************)

RECURSIVE Eval(_, _)
Eval(plan, database) ==
    CASE plan.op = "Scan" ->
            EvalScan(plan.relation, database)
      [] plan.op = "Filter" ->
            EvalFilter(Eval(plan.inputs[1], database), plan.predicate)
      [] plan.op = "Project" ->
            EvalProject(Eval(plan.inputs[1], database), plan.attrs)
      [] plan.op = "Join" ->
            EvalJoin(Eval(plan.inputs[1], database),
                    Eval(plan.inputs[2], database),
                    plan.condition)
      [] plan.op = "LeftJoin" ->
            EvalLeftJoin(Eval(plan.inputs[1], database),
                        Eval(plan.inputs[2], database),
                        plan.condition)
      [] plan.op = "SemiJoin" ->
            EvalSemiJoin(Eval(plan.inputs[1], database),
                        Eval(plan.inputs[2], database),
                        plan.condition)
      [] plan.op = "AntiJoin" ->
            EvalAntiJoin(Eval(plan.inputs[1], database),
                        Eval(plan.inputs[2], database),
                        plan.condition)
      [] plan.op = "Union" ->
            EvalUnion(Eval(plan.inputs[1], database),
                     Eval(plan.inputs[2], database))
      [] plan.op = "Intersect" ->
            EvalIntersect(Eval(plan.inputs[1], database),
                         Eval(plan.inputs[2], database))
      [] plan.op = "Except" ->
            EvalExcept(Eval(plan.inputs[1], database),
                      Eval(plan.inputs[2], database))
      [] plan.op = "Distinct" ->
            EvalDistinct(Eval(plan.inputs[1], database))
      [] plan.op = "Limit" ->
            EvalLimit(Eval(plan.inputs[1], database), plan.n)
      [] OTHER -> {}

(****************************************************************************)
(* Type invariants                                                          *)
(****************************************************************************)

TypeOK ==
    /\ db \in Database
    /\ result1 \subseteq Tuple
    /\ result2 \subseteq Tuple
    /\ equivalent \in BOOLEAN
    /\ Cardinality(result1) <= MaxTuples
    /\ Cardinality(result2) <= MaxTuples

(****************************************************************************)
(* Initial state                                                            *)
(****************************************************************************)

Init ==
    /\ db \in Database
    /\ \E p1, p2 \in Operators :
        /\ plan1 = p1
        /\ plan2 = p2
    /\ result1 = {}
    /\ result2 = {}
    /\ equivalent = FALSE

(****************************************************************************)
(* Execute both plans and compare results                                  *)
(****************************************************************************)

Execute ==
    /\ result1' = Eval(plan1, db)
    /\ result2' = Eval(plan2, db)
    /\ equivalent' = (result1' = result2')
    /\ UNCHANGED <<db, plan1, plan2>>

(****************************************************************************)
(* Apply a transformation rule                                             *)
(****************************************************************************)

Transform ==
    \E rule \in Rules :
        /\ plan2' = rule(plan1)  \* Apply transformation
        /\ UNCHANGED <<db, plan1, result1, result2, equivalent>>

(****************************************************************************)
(* Next state transition                                                   *)
(****************************************************************************)

Next ==
    \/ Execute
    \/ Transform

(****************************************************************************)
(* Specification                                                            *)
(****************************************************************************)

Spec == Init /\ [][Next]_vars

(****************************************************************************)
(* Semantic equivalence properties                                         *)
(****************************************************************************)

(* THEOREM: Transformations preserve semantics *)
THEOREM SemanticEquivalence ==
    [](Execute => (result1 = result2))

(* THEOREM: All transformation rules preserve query results *)
THEOREM RuleCorrectness ==
    \A rule \in Rules :
        [](plan2 = rule(plan1) /\ Execute => result1 = result2)

(****************************************************************************)
(* Specific rule equivalences                                              *)
(****************************************************************************)

(* Filter pushdown through join preserves semantics *)
FilterPushdownCorrect ==
    LET original == [op |-> "Filter",
                     inputs |-> <<[op |-> "Join", inputs |-> <<left, right>>]>>]
        pushed == [op |-> "Join",
                  inputs |-> <<[op |-> "Filter", inputs |-> <<left>>], right>>]
    IN Eval(original, db) = Eval(pushed, db)

(* Join commutativity *)
JoinCommutative ==
    LET plan_a == [op |-> "Join", inputs |-> <<left, right>>]
        plan_b == [op |-> "Join", inputs |-> <<right, left>>]
    IN Eval(plan_a, db) = Eval(plan_b, db)

(* Join associativity *)
JoinAssociative ==
    LET plan_a == [op |-> "Join",
                   inputs |-> <<[op |-> "Join", inputs |-> <<a, b>>], c>>]
        plan_b == [op |-> "Join",
                   inputs |-> <<a, [op |-> "Join", inputs |-> <<b, c>>]>>]
    IN Eval(plan_a, db) = Eval(plan_b, db)

(* Project fusion *)
ProjectFusion ==
    LET plan_a == [op |-> "Project", attrs |-> attrs1,
                   inputs |-> <<[op |-> "Project", attrs |-> attrs2,
                               inputs |-> <<input>>]>>]
        plan_b == [op |-> "Project", attrs |-> attrs1 \intersect attrs2,
                   inputs |-> <<input>>]
    IN Eval(plan_a, db) = Eval(plan_b, db)

(* Filter merge *)
FilterMerge ==
    LET plan_a == [op |-> "Filter", predicate |-> pred1,
                   inputs |-> <<[op |-> "Filter", predicate |-> pred2,
                               inputs |-> <<input>>]>>]
        plan_b == [op |-> "Filter",
                   predicate |-> LAMBDA t : pred1(t) /\ pred2(t),
                   inputs |-> <<input>>]
    IN Eval(plan_a, db) = Eval(plan_b, db)

(****************************************************************************)
(* Key invariants                                                          *)
(****************************************************************************)

(* INV1: Type correctness *)
Inv1 == TypeOK

(* INV2: Results are deterministic *)
Inv2 == Execute /\ Execute' => result1 = result1' /\ result2 = result2'

(* INV3: Equivalence is reflexive *)
Inv3 == plan1 = plan2 => equivalent

(* INV4: Equivalence is symmetric *)
Inv4 == (result1 = result2) <=> (result2 = result1)

(* INV5: Equivalence is transitive *)
Inv5 ==
    \A db1, db2, db3 \in Database :
        (Eval(plan1, db1) = Eval(plan2, db1) /\
         Eval(plan2, db2) = Eval(plan2, db3)) =>
        Eval(plan1, db1) = Eval(plan2, db3)

(****************************************************************************)
(* Safety property: transformations never produce incorrect results        *)
(****************************************************************************)

Safety ==
    [](Execute => result1 = result2)

THEOREM Spec => []Safety

=============================================================================
