# TLA+ Formal Verification

This directory contains TLA+ specifications for formally verifying critical properties of the relational algebra optimization engine.

## Overview

TLA+ (Temporal Logic of Actions) is a formal specification language for concurrent and distributed systems. We use it to mathematically prove that our optimizer behaves correctly under all possible scenarios.

## Specifications

### 1. RuleComposition.tla

**Purpose**: Proves that the e-graph rewriting process always terminates.

**Key Properties**:
- **Termination**: The optimizer will always finish, either by reaching saturation or hitting resource bounds
- **Monotonic Growth**: The e-graph can only grow (or stay the same size), never shrink
- **Bounded Resources**: Node count and e-class sizes remain within configured limits

**Why This Matters**: Without termination guarantees, an optimizer could run forever on certain queries. This proof ensures that will never happen.

### 2. CostMonotonicity.tla

**Purpose**: Proves that logical transformation rules never make query plans more expensive.

**Key Properties**:
- **Logical Monotonicity**: Applying a logical rule (filter pushdown, join reordering, etc.) can only reduce or maintain cost, never increase it
- **Eventual Optimality**: The optimizer eventually reaches a state where no logical rule can further reduce cost
- **Cost Non-Negativity**: Costs are always non-negative real numbers

**Why This Matters**: This proves our cost model is correct and that logical optimizations are always beneficial. If a rule could increase cost, it would indicate a bug in either the rule or the cost model.

### 3. Equivalence.tla

**Purpose**: Proves that all transformation rules preserve query semantics.

**Key Properties**:
- **Semantic Equivalence**: The optimized plan always produces the same results as the original plan
- **Rule Correctness**: Every individual transformation rule preserves semantics
- **Determinism**: Query evaluation is deterministic (same inputs → same outputs)
- **Specific Rule Properties**:
  - Filter pushdown through joins
  - Join commutativity and associativity
  - Project fusion
  - Filter merge

**Why This Matters**: The most critical property of an optimizer is correctness. This proves that optimization never changes what a query computes, only how efficiently it's computed.

## Installation

### TLA+ Toolbox (GUI)

Download from: https://lamport.azurewebsites.net/tla/toolbox.html

The Toolbox includes:
- TLC model checker
- TLAPS theorem prover
- Built-in specification editor

### Command-Line Tools

#### macOS (Homebrew)
```bash
brew install tla-plus-toolbox
```

#### Linux (manual installation)
```bash
# Download TLA+ tools
wget https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar

# Create wrapper script
cat > /usr/local/bin/tlc << 'EOF'
#!/bin/bash
java -XX:+UseParallelGC -cp /path/to/tla2tools.jar tlc2.TLC "$@"
EOF

chmod +x /usr/local/bin/tlc
```

#### Nix (via flake)
Already configured in `flake.nix`:
```bash
nix develop  # TLA+ tools available in dev shell
```

## Running Model Checking

### Automated (All Specifications)

```bash
./scripts/run-tla.sh
```

This runs TLC on all three specifications and reports results.

### Manual (Individual Specification)

```bash
cd tla
tlc -workers auto -config models/RuleComposition.cfg RuleComposition.tla
tlc -workers auto -config models/CostMonotonicity.cfg CostMonotonicity.tla
tlc -workers auto -config models/Equivalence.cfg Equivalence.tla
```

### TLA+ Toolbox (GUI)

1. Open TLA+ Toolbox
2. File → Open Spec → Add Existing Spec
3. Select a `.tla` file
4. TLC Model Checker → New Model
5. Load corresponding `.cfg` from `models/`
6. Run TLC

## Configuration Files

Configuration files in `models/` define:
- **Constants**: Fixed values for model checking (e.g., `MaxIterations = 100`)
- **Invariants**: Properties that must hold in every state
- **Properties**: Temporal logic formulas to verify (safety and liveness)
- **Constraints**: Bounds to make state space finite for model checking

### Configuration Parameters

#### RuleComposition.cfg
- `MaxIterations = 100`: Maximum rewrite iterations
- `MaxNodes = 1000`: Maximum e-graph nodes
- `MaxEClassSize = 50`: Maximum equivalence class size

#### CostMonotonicity.cfg
- `MaxCost = 10000`: Upper bound for query costs
- `InitialCost = 5000`: Starting cost for test queries
- `LogicalRules = {r1, r2, r3}`: Set of logical transformation rules
- `PhysicalRules = {p1, p2}`: Set of physical implementation rules

#### Equivalence.cfg
- `Relations = {orders, customers}`: Test database relations
- `Attributes = {id, customer_id, amount, name}`: Columns
- `MaxTuples = 10`: Maximum rows per table (keeps state space manageable)

## Interpreting Results

### Success Output

```
TLC2 Version 2.18 of Day Month Year
Running TLC on specification RuleComposition
Computing initial states...
Finished computing initial states: 3 distinct states generated.
Progress(10): 156 states generated, 89 distinct states found
Model checking completed. No error has been found.
  Estimates of the probability that TLC did not check all reachable states
  because two distinct states had the same fingerprint:
  calculated (optimistic):  val = 0.0
State space size: 285 distinct states
```

**Interpretation**: All properties verified, specification is correct.

### Failure Output

```
Error: Invariant TypeOK is violated.
The behavior up to this point is:
State 1: <Initial predicate>
  /\ iteration = 0
  /\ egraph = {}
State 2: <Action ApplyRule line 45>
  /\ iteration = 1
  /\ egraph = {[op: "Filter", ...]}
```

**Interpretation**: A counterexample was found. The trace shows how to reproduce the error.

## Performance Tuning

### Reduce State Space

If model checking is too slow:

1. **Reduce constants**: Smaller `MaxIterations`, `MaxNodes`, `MaxTuples`
2. **Add constraints**: Limit which states TLC explores
3. **Use symmetry**: Declare symmetry sets to avoid exploring equivalent states

### Increase Parallelism

```bash
tlc -workers 8 -config models/spec.cfg spec.tla  # Use 8 threads
```

### Memory Allocation

```bash
java -Xmx16G -XX:+UseParallelGC -cp tla2tools.jar tlc2.TLC spec.tla
```

## Limitations

### Model Checking vs Theorem Proving

**TLC Model Checker** (what we use):
- Explores a finite state space exhaustively
- Fast, automatic, finds counterexamples
- Limited to bounded models (finite constants)
- Cannot prove properties for unbounded systems

**TLAPS Theorem Prover** (future work):
- Proves properties for all possible values
- Interactive, requires proof guidance
- Can handle infinite state spaces
- More effort to use

### Current Scope

Our TLA+ specifications verify:
- ✓ Core optimization algorithms (termination, monotonicity, equivalence)
- ✓ Properties hold for small, bounded models
- ✓ Counterexample-driven debugging

They do NOT verify:
- ✗ Full implementation correctness (Rust code)
- ✗ Properties for unbounded systems (infinite graphs, queries)
- ✗ Liveness properties requiring fairness assumptions

### Bridging the Gap

We combine formal verification with other techniques:
- **Property-based testing**: proptest generates random test cases
- **Differential testing**: Compare against PostgreSQL, DuckDB
- **Mutation testing**: cargo-mutants verifies test quality
- **Static analysis**: Clippy, rust-analyzer catch bugs at compile time

## Further Reading

### TLA+ Resources

- [TLA+ Homepage](https://lamport.azurewebsites.net/tla/tla.html)
- [Learn TLA+](https://learntla.com/)
- [Practical TLA+](https://www.apress.com/gp/book/9781484238288) by Hillel Wayne
- [TLA+ Video Course](https://lamport.azurewebsites.net/video/videos.html) by Leslie Lamport

### TLA+ in Practice

- [Amazon: How AWS Uses TLA+](https://lamport.azurewebsites.net/tla/amazon.html)
- [Microsoft: TLA+ at Azure](https://www.microsoft.com/en-us/research/publication/tla-azure/)
- [MongoDB: Formal Verification](https://www.mongodb.com/blog/post/formal-methods-mongodb)

### Formal Verification in Databases

- PostgreSQL: [Serializable Snapshot Isolation proof](https://drkp.net/papers/ssi-vldb12.pdf)
- CockroachDB: [Transaction model verification](https://www.cockroachlabs.com/blog/serializable-lockless-distributed-isolation-cockroachdb/)
- FoundationDB: [Simulation testing](https://www.foundationdb.org/blog/simulation-and-testing/)

## Contributing

When adding new optimization rules:

1. **Update Equivalence.tla**: Add evaluation semantics for new operators
2. **Add specific properties**: Prove the new rule preserves semantics
3. **Extend test cases**: Add positive/negative examples in the .tla file
4. **Run verification**: Ensure TLC still passes with the new rule
5. **Document assumptions**: Note any preconditions or limitations

## Contact

For questions about TLA+ specifications:
- Open an issue on GitHub
- See `docs/formal-verification.md` for detailed explanations
- Consult TLA+ community: https://groups.google.com/g/tlaplus
