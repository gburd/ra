# ML-Based Rule Ordering and Pruning

## Overview

Ra's ML-based rule ordering system uses Bayesian belief networks to dynamically order and prune optimization rules based on their expected effectiveness. The system continuously learns from execution observations, adapting rule priorities to the specific workload patterns of each deployment.

## Architecture

### Components

1. **Belief Network** (`ra-ml/src/belief_network.rs`)
   - Maintains conditional probability tables (CPTs) for each rule
   - Tracks prior probability of improvement
   - Learns context-conditioned probabilities
   - Computes expected value of applying each rule

2. **Streaming Updates** (`ra-ml/src/streaming.rs`)
   - Continuous model updates via differential dataflow
   - Real-time execution feedback integration
   - Shared model state across multiple optimizer instances
   - Batch processing of observations

3. **Database Storage** (`ra-ml/src/storage.rs`)
   - PostgreSQL or SQLite backend for model persistence
   - Differential dataflow-driven updates
   - Multi-instance model sharing
   - Observation history storage

4. **Optimizer Integration** (`ra-engine/src/ml_integration.rs`)
   - Connects belief network to rule application logic
   - Feeds execution observations to training
   - Rule ordering and filtering based on context

## How It Works

### Learning from Observations

Each time the optimizer applies a rule, it records an execution observation:

```rust
ExecutionObservation {
    rule_id: "filter-pushdown-basic",
    estimated_time_before: 100.0,
    estimated_time_after: 50.0,
    actual_time: Some(45.0),
    improved: true,
    context: vec![table_size, predicate_selectivity, ...],
    timestamp: 1234567890,
}
```

The belief network updates conditional probability tables based on:
- **Success rate**: How often the rule improves plans
- **Improvement magnitude**: Average cost reduction when successful
- **Context correlation**: Which plan features predict success

### Rule Ordering

Given a query plan and context features, the belief network ranks rules by expected value:

```
expected_value(rule) = P(improvement | context) × mean_improvement
```

Rules with higher expected value are tried first, reducing wasted effort on unlikely-to-help transformations.

### Rule Pruning

The system can filter out rules with expected value below a threshold:

```rust
let promising_rules = belief_network.filter_rules(
    &all_rules,
    &context,
    threshold: 0.1  // 10% expected improvement
);
```

This reduces the search space by skipping rules that historically don't help similar queries.

## Model Scopes

Models can be trained and shared at three levels:

- **Account**: Separate models per customer account
- **Project**: Separate models per project/database
- **Overall**: Single shared model across all instances

This allows specialization to workload characteristics while falling back to global knowledge for cold starts.

## CLI Usage

### Training Models

Currently, model training happens through observation collection:

```bash
# Enable ML-enhanced optimization
ra-cli optimize 'SELECT ...' --ml-ordering --ml-filtering

# Observations are automatically collected and stored
```

### Managing Models

```bash
# View model statistics
ra-cli ml stats --name production_model

# Show statistics for specific rule
ra-cli ml stats --name production_model --rule filter-pushdown

# Load model from database
ra-cli ml load --name production_model --scope overall

# Save model to database
ra-cli ml save --input model.json --name production_model

# Export model for analysis
ra-cli ml export --name production_model --output export.json
ra-cli ml export --name production_model --format csv --output stats.csv
```

### Database Configuration

By default, models are stored in SQLite:

```bash
# Use PostgreSQL instead
ra-cli ml save \
    --input model.json \
    --name prod_model \
    --backend postgres \
    --database "postgresql://localhost/ra_ml"
```

## Differential Dataflow Integration

The streaming module uses timely dataflow for continuous learning:

```rust
let config = StreamingConfig {
    workers: 4,
    batch_size: 100,
    update_interval_secs: 60,
    shared_state: true,
    scope: ModelScope::Overall,
};

let estimator = StreamingMlEstimator::new(model, schema, config);

// Observations flow through differential dataflow
estimator.observe(observation);
```

Key features:
- **Batched updates**: Accumulate observations before retraining
- **Incremental computation**: Only recompute affected CPTs
- **Multi-instance coordination**: Share state across optimizer instances

## Configuration

### MlOptimizerConfig

```rust
MlOptimizerConfig {
    enable_rule_ordering: true,    // Use learned rule priorities
    enable_rule_filtering: false,  // Prune low-value rules
    filter_threshold: 0.1,         // Min expected improvement
    collect_observations: true,    // Record execution data
    model_scope: ModelScope::Overall,
}
```

### StreamingConfig

```rust
StreamingConfig {
    workers: 4,                // Dataflow worker threads
    batch_size: 100,           // Observations per batch
    update_interval_secs: 60,  // Max time between updates
    shared_state: true,        // Share across instances
    scope: ModelScope::Overall,
}
```

### StorageConfig

```rust
StorageConfig {
    backend: DatabaseBackend::Postgres,
    connection_string: "postgresql://localhost/ra_ml",
    max_connections: 10,
}
```

## Performance Impact

### Rule Ordering Benefits

- **Reduced iterations**: Apply successful rules first
- **Faster convergence**: Reach good plans earlier
- **Better intermediate plans**: Useful if budget expires

### Rule Filtering Benefits

- **Smaller search space**: Skip unlikely transformations
- **Lower memory usage**: Fewer e-graph nodes
- **Faster saturation**: Less work per iteration

### Overhead

- **Observation collection**: ~1% per optimization
- **Model inference**: ~5ms for ordering 100 rules
- **Batch training**: Background, amortized

## Future Enhancements

1. **Online training**: Update CPTs during optimization
2. **Neural network ordering**: Replace CPTs with learned model
3. **Cost model calibration**: Learn better cardinality estimates
4. **Plan fingerprinting**: Recognize similar queries
5. **Transfer learning**: Bootstrap new deployments from global knowledge

## Related RFCs

- RFC 0050: Cardinality Estimation
- RFC 0058: Rule Complexity Prioritization
- RFC 0070: Adaptive Resource Budgets

## References

- Marcus et al. "Neo: A Learned Query Optimizer" (VLDB 2019)
- Hilprecht et al. "DeepDB: Learn from Data, not from Queries!" (SIGMOD 2020)
- Yang et al. "Deep Unsupervised Cardinality Estimation" (VLDB 2019)
