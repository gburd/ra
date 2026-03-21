# Rule: "ML-Based Cardinality Estimation"

**Category:** experimental/ml-guided
**File:** `rules/experimental/ml-guided/ml-cardinality-estimation.rra`

## Metadata

- **ID:** `ml-cardinality-estimation`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** ml, cardinality, neural-network, deep-learning, autoregressive
- **Authors:** "RA Contributors"


# ML-Based Cardinality Estimation

## Description

Uses machine learning models to predict query cardinalities, addressing the
systematic errors of traditional estimators (independence assumption,
uniformity assumption, containment assumption). Three major architectures
have emerged:

1. **Query-driven models** (MSCN): Train on (query, true_cardinality) pairs.
   Fast inference but needs query workload for training.
2. **Data-driven models** (NeuroCard, DeepDB): Learn the joint data
   distribution. No query workload needed but higher training cost.
3. **Hybrid models** (Bao, Balsa): Learn to steer the optimizer rather than
   predict cardinalities directly.

The key metric is q-error: max(estimate/true, true/estimate). Traditional
estimators achieve median q-error 2-10x on JOB benchmark; ML models achieve
1.3-3x.

**When to apply**: Multi-way joins with correlated predicates where
traditional statistics produce estimates off by 10x or more. Not justified
for simple point queries or when current estimates are adequate.

**Why it works**: Neural networks can learn complex joint distributions
and correlations that independence-based estimators miss. The JOB
(Join Order Benchmark) demonstrated that traditional estimators fail
catastrophically on real-world queries, while ML models provide
consistently better estimates.

**Research status**: Active area. Production adoption is limited: Bao
(learned optimizer steering) is closest to production. Pure cardinality
models face challenges with distribution shift, training data collection,
and cold-start.

## Relational Algebra

```algebra
ML cardinality estimation replaces:
  CardEst_traditional(sigma[p](R join S)) =
    |R| * |S| * sel_independent(p)

With:
  CardEst_ML(sigma[p](R join S)) =
    model.predict(featurize(R, S, p))

Model architectures:

MSCN (Multi-Set Convolutional Network):
  Input: (table_set, join_set, predicate_set) as multi-sets
  Output: log(cardinality)
  Training: supervised on executed query workload

NeuroCard (Autoregressive):
  Input: (table, column, value) tuples
  Output: P(column = value | preceding columns)
  Cardinality = product of conditional probabilities
  Training: unsupervised on data (no query workload needed)

DeepDB (Sum-Product Network):
  Input: data tuples
  Output: joint probability distribution
  Cardinality = integrate P(conditions) over data
  Training: structure learning + parameter fitting on data
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Integration rule: replace traditional estimate with ML estimate
rw!("ml-cardinality-estimation";
    "(cardinality ?subplan)" =>
    "(ml_cardinality ?subplan)"
    if ml_model_available()
    if query_in_training_distribution("?subplan")
),

// MSCN-style query-driven model
struct MSCNEstimator {
    model: TorchModel,
    table_embeddings: HashMap<String, Vec<f32>>,
    column_encodings: HashMap<String, usize>,
    training_queries: usize,
}

impl MSCNEstimator {
    fn featurize(&self, subplan: &RelExpr) -> Tensor {
        // Table features: bag-of-tables encoding
        let tables = collect_tables(subplan);
        let table_vec: Vec<f32> = tables
            .iter()
            .map(|t| self.table_embeddings.get(t).cloned()
                .unwrap_or_default())
            .flatten()
            .collect();

        // Join features: adjacency matrix
        let joins = collect_joins(subplan);
        let join_vec = encode_join_graph(&tables, &joins);

        // Predicate features: (column, operator, value) triples
        let predicates = collect_predicates(subplan);
        let pred_vec: Vec<f32> = predicates
            .iter()
            .map(|p| self.encode_predicate(p))
            .flatten()
            .collect();

        // Concatenate and pad to fixed length
        Tensor::from_slices(&[&table_vec, &join_vec, &pred_vec])
    }

    fn predict(&self, subplan: &RelExpr) -> CardinalityPrediction {
        let features = self.featurize(subplan);
        let output = self.model.forward(&features);

        // Model outputs log-scale cardinality + uncertainty
        let log_card = output[0];
        let log_uncertainty = output[1].abs();

        CardinalityPrediction {
            estimate: log_card.exp(),
            lower_bound: (log_card - 2.0 * log_uncertainty).exp(),
            upper_bound: (log_card + 2.0 * log_uncertainty).exp(),
            confidence: (-log_uncertainty).exp(),
        }
    }
}

// NeuroCard-style data-driven model
struct NeuroCardEstimator {
    autoregressive_model: AutoregressiveModel,
    column_order: Vec<String>,
    schema: Schema,
}

impl NeuroCardEstimator {
    fn estimate(&self, predicates: &[Predicate]) -> f64 {
        // Convert predicates to column constraints
        let mut constraints = HashMap::new();
        for pred in predicates {
            constraints.insert(
                pred.column().to_string(),
                pred.as_range(),
            );
        }

        // Autoregressive factorization:
        // P(x1, x2, ..., xn) = P(x1) * P(x2|x1) * P(x3|x1,x2) * ...
        let mut log_prob = 0.0;

        for (i, col) in self.column_order.iter().enumerate() {
            let context: Vec<(&str, &Range)> = self.column_order[..i]
                .iter()
                .filter_map(|c| {
                    constraints.get(c.as_str())
                        .map(|r| (c.as_str(), r))
                })
                .collect();

            let conditional_prob = if let Some(range) = constraints.get(col.as_str()) {
                self.autoregressive_model
                    .conditional_probability(col, range, &context)
            } else {
                1.0 // Unconstrained column: probability = 1
            };

            log_prob += conditional_prob.ln();
        }

        let total_rows = self.schema.total_rows as f64;
        total_rows * log_prob.exp()
    }
}

// Training pipeline
struct MLTrainingPipeline {
    query_log: Vec<ExecutedQuery>,
    model_type: ModelType,
}

impl MLTrainingPipeline {
    fn train_mscn(&self) -> MSCNEstimator {
        // Requires executed queries with true cardinalities
        let training_data: Vec<(Tensor, f64)> = self.query_log
            .iter()
            .map(|q| {
                let features = featurize_query(&q.plan);
                let label = q.true_cardinality.ln();
                (features, label)
            })
            .collect();

        // Train with MSE loss on log-scale
        let model = train_neural_net(
            &training_data,
            LossFunction::MSE,
            epochs: 100,
            batch_size: 256,
            learning_rate: 0.001,
        );

        MSCNEstimator { model, /* ... */ }
    }

    fn evaluate(&self, estimator: &dyn CardinalityEstimator) -> EvalMetrics {
        let mut q_errors = Vec::new();
        for query in &self.query_log {
            let estimate = estimator.estimate(&query.plan);
            let true_card = query.true_cardinality;
            let q_error = if estimate > true_card {
                estimate / true_card
            } else {
                true_card / estimate
            };
            q_errors.push(q_error);
        }

        q_errors.sort_by(|a, b| a.partial_cmp(b).unwrap());

        EvalMetrics {
            median_q_error: q_errors[q_errors.len() / 2],
            p90_q_error: q_errors[(q_errors.len() as f64 * 0.9) as usize],
            p99_q_error: q_errors[(q_errors.len() as f64 * 0.99) as usize],
            max_q_error: *q_errors.last().unwrap_or(&1.0),
        }
    }
}

struct CardinalityPrediction {
    estimate: f64,
    lower_bound: f64,
    upper_bound: f64,
    confidence: f64,
}

struct EvalMetrics {
    median_q_error: f64,
    p90_q_error: f64,
    p99_q_error: f64,
    max_q_error: f64,
}

enum ModelType {
    MSCN,
    NeuroCard,
    DeepDB,
}
```

**Restrictions:**
- MSCN: Requires 50K-100K executed queries for training
- NeuroCard: Training on full data is expensive (hours for large datasets)
- All models: may not generalize to unseen query templates
- Distribution shift: data changes require retraining
- Inference latency: 1-10ms per estimate (acceptable for optimization)
- Cold start: no useful predictions before training

## Cost Model

```rust
fn ml_estimation_benefit(
    ml_metrics: &EvalMetrics,
    traditional_metrics: &EvalMetrics,
    workload: &[Query],
) -> f64 {
    // Plan quality improvement from better cardinality estimates
    let ml_avg_error = ml_metrics.median_q_error;
    let trad_avg_error = traditional_metrics.median_q_error;

    // Cost improvement is roughly proportional to error reduction
    // for multi-way joins (errors compound)
    let avg_joins = workload.iter()
        .map(|q| q.num_joins())
        .sum::<usize>() as f64
        / workload.len() as f64;

    let ml_compounded = ml_avg_error.powf(avg_joins);
    let trad_compounded = trad_avg_error.powf(avg_joins);

    (trad_compounded - ml_compounded) / trad_compounded
}
```

**Typical benefit**: 30-90% plan quality improvement for complex analytical
queries (4+ joins) with correlated data. Marginal improvement for simple
queries where traditional statistics suffice.

## Test Cases

### Test 1: JOB benchmark correlated columns (MSCN)

```sql
SELECT COUNT(*) FROM title t
JOIN movie_info mi ON t.id = mi.movie_id
JOIN cast_info ci ON t.id = ci.movie_id
WHERE mi.info_type_id = 3
  AND ci.role_id = 1
  AND t.production_year BETWEEN 2000 AND 2010;

-- Traditional (PostgreSQL): estimate 50, true 12,847 (q-error: 257x)
-- MSCN: estimate 9,200 (q-error: 1.4x)
-- ML avoids catastrophic underestimate that would trigger nested-loop
```

### Test 2: Distribution shift (negative case)

```sql
-- Model trained on 2023 data, query on 2024 data with new products
SELECT COUNT(*) FROM orders o
JOIN products p ON o.product_id = p.id
WHERE p.category = 'AI_hardware';

-- Training data had 0 AI_hardware products
-- ML model: estimates ~0 (extrapolation failure)
-- Traditional: 1/NDV(category) * |orders| = reasonable estimate
-- Fallback to traditional needed for unseen data patterns
```

### Test 3: NeuroCard on multi-column correlation

```sql
SELECT COUNT(*) FROM census
WHERE age > 60 AND income > 100000 AND education = 'PhD';

-- Independent assumption: 0.15 * 0.08 * 0.02 = 0.00024 (240 rows/M)
-- NeuroCard captures: old PhDs have high income (positive correlation)
-- NeuroCard estimate: 0.0015 (1,500 rows/M)
-- True: 0.0018 (1,800 rows/M)
-- NeuroCard q-error: 1.2x vs traditional 7.5x
```

### Test 4: Training data requirements

```sql
-- MSCN accuracy vs training set size:
-- 1K queries:  median q-error = 8.2x (insufficient)
-- 10K queries: median q-error = 3.1x (improving)
-- 50K queries: median q-error = 1.8x (good)
-- 100K queries: median q-error = 1.5x (diminishing returns)
```

## References

**Key papers:**
- Kipf et al., "Learned Cardinalities: Estimating Correlated Joins with Deep Learning", CIDR 2019
- Yang et al., "NeuroCard: One Cardinality Estimator for All Tables", VLDB 2021
- Hilprecht et al., "DeepDB: Learn from Data, not from Queries!", SIGMOD 2020
- Marcus et al., "Bao: Making Learned Query Optimization Practical", SIGMOD 2021
- Zhu et al., "FLAT: Fast, Lightweight and Accurate Method for Cardinality Estimation", VLDB 2021

**Benchmarks:**
- Leis et al., "How Good Are Query Optimizers, Really?" (JOB benchmark), VLDB 2015
- Han et al., "Cardinality Estimation in DBMS: A Comprehensive Benchmark Evaluation", VLDB 2022

**Production systems:**
- Microsoft: CardLearner in mssql
- Google: learned index structures for Spanner
- Alibaba: ML cardinality in OceanBase
