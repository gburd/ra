# ML Cardinality Estimation

The `ra-ml` crate provides neural network models for cardinality prediction, trained on execution feedback. This enables Ra to learn from actual query executions and improve cost estimates over time.

## Overview

Traditional cardinality estimation uses histograms and independence assumptions that often produce inaccurate estimates for correlated columns or complex predicates. ML-based estimation learns from actual execution data, adapting to your specific workload and data distribution.

**Key Benefits:**
- **Learns from real data**: Captures actual selectivities, not theoretical assumptions
- **Handles correlations**: Learns relationships between columns that histograms miss
- **Adapts to workload**: Improves accuracy for frequently-run query patterns
- **Reduces plan errors**: More accurate cardinality → better join orders and algorithm choices

## How It Works: Integration with Ra Optimizer

```
┌─────────────────────────────────────────────────────────────┐
│                     SQL Query Input                          │
└────────────────────┬────────────────────────────────────────┘
                     ↓
┌─────────────────────────────────────────────────────────────┐
│  [ra-parser] Parse SQL → RelExpr Tree                       │
└────────────────────┬────────────────────────────────────────┘
                     ↓
┌─────────────────────────────────────────────────────────────┐
│  [ra-engine] Equality Saturation (egg E-graph)              │
│  • Applies 1,327+ transformation rules                       │
│  • Generates 10s-1000s of equivalent plans                   │
│  • Push filters, reorder joins, pick algorithms             │
└────────────────────┬────────────────────────────────────────┘
                     ↓
┌─────────────────────────────────────────────────────────────┐
│  [Cost Extraction] ◄── ra-ml integrates here!               │
│                                                               │
│  For each plan variant in E-graph:                          │
│  ┌────────────────────────────────────────────────┐         │
│  │  CardinalityAwareCostFn                        │         │
│  │  ├─ Cardinality Estimation                     │         │
│  │  │   ├─ HeuristicEstimator (default, fast)    │         │
│  │  │   └─ MLEstimator (learned, accurate) ◄──┐  │         │
│  │  └─ Hardware-adjusted costs                 │  │         │
│  └─────────────────────────────────────────────┼──┘         │
│                                                  │            │
│  Neural Network predicts cardinality ───────────┘            │
└────────────────────┬────────────────────────────────────────┘
                     ↓
┌─────────────────────────────────────────────────────────────┐
│  Extract Best Plan (lowest cost from E-graph)               │
└─────────────────────────────────────────────────────────────┘
```

### Integration Point: CardinalityAwareCostFn

Located in `crates/ra-engine/src/cardinality_cost.rs`, this is where ML predictions feed into cost calculation:

```rust
impl CostFunction<RelLang> for CardinalityAwareCostFn {
    fn cost<C>(&mut self, enode: &RelLang, mut costs: C) -> f64
    where
        C: FnMut(Id) -> f64,
    {
        match enode {
            RelLang::Scan(table) => {
                // Get cardinality estimate from ML or heuristic
                let rows = self.estimator.estimate_scan(table);
                rows * SCAN_COST_PER_ROW * hw_factor
            }
            RelLang::Join(join_type, left, right, condition) => {
                let left_rows = costs(*left);
                let right_rows = costs(*right);

                // ML estimates join selectivity based on:
                // - Column correlations
                // - Histogram statistics
                // - Learned patterns from past queries
                let selectivity = self.estimator.estimate_join_selectivity(
                    condition,
                    left_rows,
                    right_rows
                );

                let output_rows = left_rows * right_rows * selectivity;
                output_rows * JOIN_COST_PER_ROW
            }
            // ... other operators
        }
    }
}
```

## Components

### 1. Cardinality Estimators (`src/estimator.rs`)

Two implementations with a common interface:

```rust
pub trait CardinalityEstimator {
    fn estimate(&self, expr: &RelExpr, stats: &dyn StatisticsProvider)
        -> Cardinality;
}
```

#### HeuristicEstimator (Default)

**Characteristics:**
- Fast, no training required
- Fixed selectivity: 0.33 for filters and joins
- No statistics needed
- Good for rapid prototyping and testing

**Limitations:**
- Inaccurate for correlated columns (e.g., `city = 'NYC' AND state = 'NY'`)
- Misses data skew (e.g., 99% of orders from last month)
- Poor for complex predicates (date ranges, LIKE patterns)

#### MLEstimator (Learned)

**Characteristics:**
- Neural network trained on real executions
- Learns actual selectivities from data
- Uses table statistics & histograms
- Adapts to query workload over time

**Accuracy:**
- Measured using Q-Error metric (geometric mean of actual/predicted ratio)
- Target Q-Error < 2.0 (within 2x of actual)
- Typical improvement: 40-60% reduction in estimation error vs heuristic

### 2. Feature Extraction (`src/features.rs`)

Converts RelExpr nodes into ML-compatible feature vectors:

```rust
pub fn extract_features(
    expr: &RelExpr,
    stats: &dyn StatisticsProvider
) -> Vec<f64> {
    match expr {
        RelExpr::Scan(table) => vec![
            stats.row_count(table).log10(),      // Base cardinality
            stats.distinct_keys(table).log10(),  // Distinct values
            1.0, 0.0, 0.0,  // One-hot: is_scan
        ],

        RelExpr::Filter { predicate, input } => vec![
            // Input cardinality features
            input_features,
            // Predicate complexity
            predicate_depth(predicate),
            num_conjuncts(predicate),
            has_or_clauses(predicate) as f64,
            0.0, 1.0, 0.0,  // One-hot: is_filter
        ],

        RelExpr::Join { join_type, left, right, condition } => vec![
            // Left/right cardinalities
            left_features,
            right_features,
            // Join characteristics
            join_type_encoding(join_type),
            join_condition_selectivity_hint(condition),
            num_join_keys(condition),
            0.0, 0.0, 1.0,  // One-hot: is_join
        ],
    }
}
```

**Feature Categories:**
- **Table statistics**: row counts, distinct values, null fractions
- **Predicate features**: depth, conjunct count, operator types
- **Join features**: type, key count, foreign key hints
- **Histogram features**: bucket boundaries, frequencies

### 3. Neural Network (`src/nn.rs`)

Simple MLP (Multi-Layer Perceptron) architecture:

```rust
pub struct NeuralNetwork {
    layers: Vec<Layer>,
    activations: Vec<Activation>,
}

pub struct Layer {
    weights: Vec<Vec<f64>>,
    biases: Vec<f64>,
}

pub enum Activation {
    ReLU,
    LeakyReLU(f64),  // LeakyReLU(0.01)
    Sigmoid,
}
```

**Default Architecture:**
- Input layer: 20-50 dimensions (depends on feature extraction)
- Hidden layers: 2-3 layers, [64, 32] neurons
- Output: 1 dimension (log cardinality)
- Activation: LeakyReLU for hidden, linear for output

**Training:**
- Optimizer: Adam or SGD
- Loss: Mean Squared Error on log(cardinality)
- Regularization: L2 weight decay
- Batch size: 32-128 samples

### 4. Training Pipeline (`src/training.rs`)

```rust
pub struct TrainingSample {
    features: Vec<f64>,        // Input features
    actual_cardinality: f64,   // Ground truth from EXPLAIN ANALYZE
    predicted_cardinality: f64, // Model prediction
    q_error: f64,              // actual / predicted ratio
}

pub struct Dataset {
    samples: Vec<TrainingSample>,
    schema: SchemaInfo,
}
```

**Data Collection:**
1. Run query with `EXPLAIN ANALYZE` in PostgreSQL
2. Extract actual row counts from execution
3. Parse query to RelExpr
4. Extract features from RelExpr nodes
5. Record (features, actual_rows) pairs

**Training Process:**
1. Collect 1,000+ queries from workload
2. Extract features and actual cardinalities
3. Train neural network to minimize Q-Error
4. Validate on held-out test set
5. Deploy model to production

**Online Learning:**
- Continuously collect new queries
- Periodically retrain (e.g., nightly)
- A/B test new models before deployment
- Monitor Q-Error metrics over time

## Usage

### With Ra PostgreSQL Extension

When Ra is deployed as a PostgreSQL extension, it can replace PostgreSQL's native planner:

```sql
-- Enable Ra planner for this session
SET ra.enabled = true;

-- Enable ML cost model (requires trained model)
SET ra.cost_model = 'ml';
SET ra.ml_model_path = '/path/to/model.json';

-- Query is now planned by Ra with ML cardinality estimation
SELECT u.name, COUNT(*) as order_count
FROM users u
JOIN orders o ON u.id = o.user_id
WHERE u.age > 25 AND o.created_at > '2024-01-01'
GROUP BY u.id, u.name
HAVING COUNT(*) > 5;
```

**How it works:**
1. PostgreSQL planner hook captures query
2. Ra parses SQL to RelExpr
3. Ra runs equality saturation (applies rules)
4. ML estimator predicts cardinalities
5. Ra extracts best plan from E-graph
6. Ra injects plan back into PostgreSQL via custom plan nodes

### With Ra Proxy + pg_plan_advice (PostgreSQL 19+)

PostgreSQL 19 introduces `pg_plan_advice`, allowing external planners to inject entire execution plans:

```
┌─────────────────────────────────────────────────────────────┐
│  Client Application                                          │
└────────────────────┬────────────────────────────────────────┘
                     ↓ SQL Query
┌─────────────────────────────────────────────────────────────┐
│  Ra Proxy (intercepts all queries)                          │
│  ├─ Parse SQL                                                │
│  ├─ Generate Ra plan (with ML cost model)                   │
│  ├─ Generate PostgreSQL plan                                │
│  ├─ Compare costs/structures                                 │
│  └─ Decide: use Ra plan or PG plan                          │
└────────────────────┬────────────────────────────────────────┘
                     ↓
┌─────────────────────────────────────────────────────────────┐
│  PostgreSQL 19+ (with pg_plan_advice)                       │
│  ├─ Ra proxy injects plan via pg_plan_advice                │
│  ├─ PostgreSQL executes injected plan                        │
│  └─ Returns results + EXPLAIN ANALYZE data                  │
└────────────────────┬────────────────────────────────────────┘
                     ↓ Results + Execution Stats
┌─────────────────────────────────────────────────────────────┐
│  Ra Proxy (logs comparison data for ML training)            │
│  ├─ Actual cardinalities from EXPLAIN ANALYZE                │
│  ├─ Ra predictions vs PG predictions                         │
│  └─ Store as training data for model improvement            │
└─────────────────────────────────────────────────────────────┘
```

**Deployment:**
```bash
# Start PostgreSQL 19 with Ra proxy
docker-compose up postgres-ra-proxy

# Proxy listens on port 5433, forwards to PostgreSQL on 5432
psql -h localhost -p 5433 -U myuser mydb
```

**Configuration:**
```toml
# ra-proxy.toml
[proxy]
listen_addr = "0.0.0.0:5433"
postgres_addr = "localhost:5432"

[planner]
mode = "adaptive"  # always-ra | always-pg | adaptive
ml_model = "/models/trained-model.json"

[logging]
log_queries = true
log_plans = true
log_comparisons = true
output_dir = "/var/log/ra-proxy"

[training]
collect_training_data = true
min_execution_time_ms = 100  # Only log slow queries
training_data_file = "/data/training.jsonl"
```

### CLI Workflows

#### 1. Basic Optimization (Heuristic)

```bash
# Parse SQL to RelExpr
ra-cli explain "SELECT * FROM users WHERE age > 25"

# Optimize with default heuristic cost model
ra-cli optimize "SELECT u.name, o.total
  FROM users u
  JOIN orders o ON u.id = o.user_id
  WHERE u.age > 25 AND o.total > 100"

# Output shows:
# - Optimized plan tree
# - Estimated cost: 150,000
# - Number of E-graph nodes explored
```

#### 2. Train ML Model

```bash
# Collect training data from PostgreSQL
ra-cli ml collect-data \
  --database postgres://localhost/mydb \
  --workload queries.sql \
  --output training-data.jsonl

# Train neural network
ra-cli ml train \
  --data training-data.jsonl \
  --output model.json \
  --epochs 100 \
  --batch-size 64 \
  --learning-rate 0.001

# Outputs:
# - Trained model: model.json
# - Training metrics: loss curves, Q-Error
# - Validation accuracy
```

#### 3. Optimize with ML Model

```bash
# Use trained ML model for optimization
ra-cli optimize \
  --cost-model ml \
  --ml-model model.json \
  --stats-provider postgres://localhost/mydb \
  "SELECT * FROM users u
   JOIN orders o ON u.id = o.user_id
   WHERE u.age > 25"

# Output (with ML):
# - Estimated cost: 85,000 (ML learned age>25 is selective)
# - Plan: HashJoin(Scan(orders), Filter(age>25, Scan(users)))
# - Cardinality estimates for each operator
```

#### 4. Compare ML vs Heuristic

```bash
# Side-by-side comparison
ra-cli compare-costs \
  --heuristic \
  --ml model.json \
  --query "SELECT ..."

# Shows:
# Heuristic: 150k cost, 3-way nested loop
# ML:        85k cost, hash join with early filter
# Difference: 43% cost reduction
# Cardinality comparison at each operator
```

#### 5. Evaluate Model Accuracy

```bash
# Test model on held-out queries
ra-cli ml evaluate \
  --model model.json \
  --test-data test-queries.jsonl \
  --database postgres://localhost/mydb

# Outputs:
# - Q-Error distribution (P50, P90, P99)
# - Mean/median estimation error
# - Queries with worst estimates
# - Comparison: ML vs PostgreSQL estimates
```

## Example: Impact on Query Planning

Consider this query:

```sql
SELECT * FROM orders WHERE status = 'pending'
```

**Without ML (Heuristic):**
- Assumes 33% selectivity → 333,000 rows
- Cost model chooses: Sequential scan
- Actual: Only 1% are pending (10,000 rows)
- Result: Suboptimal plan (10x slower than necessary)

**With ML (Learned):**
- Learns from past executions: 0.01 selectivity
- Predicts: 10,000 rows (accurate!)
- Cost model chooses: Index scan on status
- Result: 10x faster execution

**Training data that enabled this:**
```json
{
  "query": "SELECT * FROM orders WHERE status = ?",
  "features": [8.0, 0.0, 1.0, ...],  // log(row_count), filter_depth, etc.
  "actual_cardinality": 10000,
  "estimated_cardinality_heuristic": 333000,
  "q_error_heuristic": 33.3,
  "q_error_ml": 1.05
}
```

## Architecture

```
ra-ml/
├── src/
│   ├── estimator.rs     # CardinalityEstimator trait
│   │   ├── HeuristicEstimator
│   │   └── MLEstimator
│   ├── features.rs      # Feature extraction from RelExpr
│   │   ├── extract_scan_features()
│   │   ├── extract_filter_features()
│   │   └── extract_join_features()
│   ├── nn.rs           # Neural network model
│   │   ├── NeuralNetwork
│   │   ├── Layer
│   │   └── Activation (ReLU, LeakyReLU, Sigmoid)
│   ├── training.rs     # Training pipeline
│   │   ├── TrainingSample
│   │   ├── Dataset
│   │   └── train_model()
│   └── lib.rs         # Public API
├── tests/
│   ├── estimator_test.rs
│   └── nn_test.rs
└── Cargo.toml
```

## Integration Points

### 1. Ra Engine Integration

`ra-engine` uses `ra-ml` via the `CardinalityAwareCostFn`:

```rust
// crates/ra-engine/src/cardinality_cost.rs
use ra_ml::{CardinalityEstimator, MLEstimator, HeuristicEstimator};

pub struct CardinalityAwareCostFn {
    estimator: Box<dyn CardinalityEstimator>,
    hardware_profile: HardwareProfile,
}

impl CardinalityAwareCostFn {
    pub fn new_heuristic() -> Self {
        Self {
            estimator: Box::new(HeuristicEstimator::new()),
            hardware_profile: HardwareProfile::default(),
        }
    }

    pub fn new_ml(model_path: &Path) -> Result<Self> {
        let estimator = MLEstimator::load(model_path)?;
        Ok(Self {
            estimator: Box::new(estimator),
            hardware_profile: HardwareProfile::default(),
        })
    }
}
```

### 2. PostgreSQL Extension Integration

The `ra-pg-extension` uses ML estimates when configured:

```c
// crates/ra-pg-extension/src/planner_hook.c
void ra_planner_hook(PlannerInfo *root, Oid relationObjectId, ...) {
    // Get configuration
    bool use_ml = GetConfigOption("ra.cost_model") == "ml";

    if (use_ml) {
        // Load ML model
        char *model_path = GetConfigOption("ra.ml_model_path");
        RaMLEstimator *estimator = ra_ml_load(model_path);

        // Run Ra optimization with ML
        RaPlan *plan = ra_optimize_with_ml(query_tree, estimator);

        // Convert to PostgreSQL plan
        return ra_plan_to_pg_plan(plan);
    } else {
        // Use heuristic estimator
        return ra_optimize_with_heuristic(query_tree);
    }
}
```

### 3. Proxy Integration

The `ra-proxy` logs all query executions for continuous learning:

```rust
// crates/ra-proxy/src/main.rs
async fn handle_query(query: &str, conn: &PgConnection) -> Result<Response> {
    // Parse query
    let relexpr = ra_parser::sql_to_relexpr(query)?;

    // Generate Ra plan with ML
    let ra_plan = ra_optimizer.optimize_with_ml(&relexpr, &ml_model)?;

    // Execute with EXPLAIN ANALYZE
    let result = conn.execute_with_explain(query).await?;

    // Extract actual cardinalities
    let actual_cards = extract_cardinalities(&result.explain_output);
    let predicted_cards = ra_plan.cardinalities();

    // Log for training
    training_logger.log_sample(TrainingSample {
        features: extract_features(&relexpr),
        actual_cardinality: actual_cards,
        predicted_cardinality: predicted_cards,
        q_error: compute_q_error(actual_cards, predicted_cards),
    });

    Ok(result.response)
}
```

## Performance Considerations

**Inference Latency:**
- Heuristic estimator: <1ms per query
- ML estimator: 1-5ms per query (depends on model size)
- Acceptable for most workloads (< 1% of total query time)

**Training Time:**
- Initial training: 10-30 minutes (1,000-10,000 queries)
- Incremental updates: 1-5 minutes (100-1,000 new queries)
- Schedule: nightly or weekly, depending on workload changes

**Memory Usage:**
- Model size: 1-10 MB (typical)
- Feature cache: 10-100 MB
- Training data: 100 MB - 1 GB

**Accuracy vs Speed Tradeoff:**
- Small model (32-16 neurons): Fast inference, moderate accuracy
- Large model (128-64-32 neurons): Slower inference, high accuracy
- Recommendation: Start small, scale up if needed

## Monitoring & Debugging

**Metrics to Track:**
- Q-Error distribution (P50, P90, P99)
- Queries with Q-Error > 10 (severe estimation errors)
- Training loss over time
- Model prediction latency

**Debugging Tools:**
```bash
# Inspect model predictions
ra-cli ml explain \
  --model model.json \
  --query "SELECT ..." \
  --verbose

# Shows:
# - Feature values for each operator
# - Model predictions at each node
# - Comparison with heuristic estimates

# Find queries with high Q-Error
ra-cli ml analyze-errors \
  --training-data training.jsonl \
  --threshold 10

# Outputs:
# - Queries with worst estimates
# - Feature patterns correlated with errors
# - Recommendations for feature engineering
```

## Limitations & Future Work

**Current Limitations:**
- Single-table statistics only (no cross-table correlations yet)
- No subquery cardinality modeling
- Limited support for complex predicates (LIKE, IN, EXISTS)
- Model doesn't adapt to schema changes automatically

**Future Enhancements:**
- **Query-structure-aware models**: Separate models for OLTP vs OLAP
- **Uncertainty quantification**: Confidence intervals for predictions
- **Active learning**: Prioritize collecting data for uncertain queries
- **Transfer learning**: Pre-trained models for common database schemas
- **Automated feature engineering**: Learn optimal features from data

## Continuous Learning and Database Storage

Ra now supports continuous model updates and database-backed persistence:

### Streaming Updates

The `StreamingMlEstimator` provides continuous learning via differential dataflow:

```rust
use ra_ml::streaming::{StreamingMlEstimator, StreamingConfig, ModelScope};

let config = StreamingConfig {
    workers: 4,
    batch_size: 100,
    update_interval_secs: 60,
    shared_state: true,
    scope: ModelScope::Overall,
};

let estimator = StreamingMlEstimator::new(model, schema, config);

// Observations flow through differential dataflow
estimator.observe(ExecutionObservation {
    rule_id: "filter-pushdown",
    estimated_time_before: 100.0,
    estimated_time_after: 50.0,
    actual_time: Some(45.0),
    improved: true,
    context: vec![...],
    timestamp: timestamp(),
});
```

### Database Storage

Models and observations are stored in PostgreSQL:

```rust
use ra_ml::storage::{ModelStorage, StorageConfig, DatabaseBackend};

let config = StorageConfig {
    backend: DatabaseBackend::Postgres,
    connection_string: "postgresql://localhost/ra_ml",
    max_connections: 10,
};

let storage = ModelStorage::new(config).await?;

// Save model
storage.save_model("production", &model, &schema_json, "overall", None, None).await?;

// Load model
let (model, schema_data) = storage.load_model("production").await?;
```

### Model Scopes

Models can be scoped to different levels:
- **Account**: Per-customer models
- **Project**: Per-database models
- **Overall**: Global shared model

This enables workload-specific learning while sharing knowledge across deployments.

## Rule Ordering with Belief Networks

In addition to cardinality estimation, Ra uses ML for dynamic rule ordering. See [ml-rule-ordering.md](ml-rule-ordering.md) for details on:

- Bayesian belief networks for rule effectiveness prediction
- Context-aware rule prioritization
- Continuous learning from execution observations
- Rule filtering to reduce search space

## Further Reading

- [Cost Models](../guides/cost-models.md) -- Traditional cost estimation
- [Adaptive Execution](adaptive-execution.md) -- Runtime reoptimization
- [PostgreSQL Extension](../integrations/postgresql.md) -- Ra as PostgreSQL planner
- [Proxy Architecture](../guides/proxy-architecture.md) -- Ra proxy with pg_plan_advice
- [Research Papers](../research.md) -- Academic foundations of learned cost models
- [ML Rule Ordering](ml-rule-ordering.md) -- Bayesian belief networks for optimization
