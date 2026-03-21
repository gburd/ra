# Rule: ML-Based Cardinality Estimation

**Category:** execution-models/experimental
**File:** `rules/execution-models/experimental/ml-cardinality-estimation.rra`

## Metadata

- **ID:** `ml-cardinality-estimation`
- **Version:** "1.0.0"
- **Databases:** postgresql, spark, trino, noisepage
- **Tags:** execution, experimental, research, machine-learning, cardinality, estimation, neural-network
- **Authors:** Andreas Kipf, Zongheng Yang, Ryan Marcus


# ML-Based Cardinality Estimation

## Description

Machine learning-based cardinality estimation replaces traditional histogram and
sampling-based techniques with trained models that predict the number of rows a
query predicate or join will produce. Traditional estimators assume attribute
independence and uniform distribution, causing errors that compound
exponentially across multi-table joins (often 10-1000x off). ML models can
capture correlations between attributes, learn from query feedback, and provide
substantially more accurate estimates.

**When to apply**: Any query optimizer that uses cardinality estimates to choose
join orders, access paths, or parallelism levels. The largest gains come from
complex multi-table joins where traditional independence assumptions fail badly.

**Why it works**: The optimizer's plan quality is extremely sensitive to
cardinality estimates. A 10x estimation error on a join can cause the optimizer
to choose a hash join instead of an index lookup, or to place a large table on
the build side instead of the probe side. ML models trained on actual data
distributions can capture multi-column correlations (e.g., city and zip code
are correlated), data skew, and functional dependencies that histogram-based
methods miss entirely.

**Key approaches:**
- **Query-driven models**: Train on (query, cardinality) pairs from query logs.
  Input: SQL predicate features. Output: estimated cardinality.
- **Data-driven models**: Learn the joint data distribution directly using
  density estimation (autoregressive models, normalizing flows, sum-product
  networks). Estimate cardinality by integrating over predicate ranges.
- **Hybrid models**: Combine traditional statistics for simple predicates with
  ML models for complex multi-column or join predicates.
- **Workload-aware models**: Focus accuracy on predicates seen in the workload
  rather than arbitrary predicates.

**Model architectures:**
1. **MSCN (Multi-Set Convolutional Network)**: Encodes tables, joins, and
   predicates as sets; processes with set convolutions.
2. **Naru/NeuroCard (Autoregressive)**: Models P(col1, col2, ..., colN) as a
   product of conditionals. Cardinality = N * P(predicate).
3. **DeepDB (Sum-Product Network)**: Tractable probabilistic model that
   supports exact marginalization and conditioning.
4. **FLAT (Factorized Learned Approach)**: Factorizes the joint distribution
   along conditional independence structure.

## Relational Algebra

```algebra
-- Traditional estimation (independence assumption):
|sigma_{A>10 AND B<5}(R)| = |R| * sel(A>10) * sel(B<5)
  -- Assumes A and B are independent
  -- Error: if A and B are correlated, can be 100x off

-- ML estimation (learned joint distribution):
|sigma_{A>10 AND B<5}(R)| = model.predict(
  table=R, predicates=[(A,>,10), (B,<,5)]
)
  -- Model has learned P(A,B) from actual data
  -- Captures correlation: error typically < 3x

-- Join cardinality:
|R join S on R.a = S.b| = join_model.predict(
  tables=[R, S], join=[R.a = S.b],
  filters=[R.x > 10]
)
  -- Traditional: |R| * |S| / max(NDV(R.a), NDV(S.b))
  -- ML: captures skew, correlations, FK relationships
```

## Implementation

```rust
/// ML cardinality estimator interface
pub trait MLCardinalityEstimator {
    /// Estimate cardinality for a subplan
    fn estimate(
        &self,
        tables: &[TableRef],
        predicates: &[Predicate],
        joins: &[JoinCondition],
    ) -> CardinalityEstimate;

    /// Update model with observed cardinality
    fn feedback(
        &mut self,
        query: &QueryFeatures,
        actual_cardinality: u64,
    );
}

/// Query-driven model (MSCN-style)
pub struct QueryDrivenEstimator {
    /// Neural network model
    model: NeuralNetwork,
    /// Feature encoder for predicates
    encoder: PredicateEncoder,
    /// Training buffer for online learning
    feedback_buffer: Vec<(QueryFeatures, u64)>,
    /// Retrain after this many feedback samples
    retrain_threshold: usize,
}

impl QueryDrivenEstimator {
    /// Encode query as feature vector
    fn encode_query(
        &self,
        tables: &[TableRef],
        predicates: &[Predicate],
        joins: &[JoinCondition],
    ) -> Vec<f32> {
        let mut features = Vec::new();

        // Table bitmap: which tables are referenced
        for table in tables {
            features.extend(
                self.encoder.encode_table(table),
            );
        }

        // Predicate encoding: column, operator, value
        for pred in predicates {
            let col_embed =
                self.encoder.encode_column(&pred.column);
            let op_embed =
                self.encoder.encode_op(&pred.op);
            let val_embed =
                self.encoder.encode_value(&pred.value);
            features.extend(col_embed);
            features.extend(op_embed);
            features.extend(val_embed);
        }

        // Join encoding
        for join in joins {
            features.extend(
                self.encoder.encode_join(join),
            );
        }

        features
    }
}

impl MLCardinalityEstimator for QueryDrivenEstimator {
    fn estimate(
        &self,
        tables: &[TableRef],
        predicates: &[Predicate],
        joins: &[JoinCondition],
    ) -> CardinalityEstimate {
        let features = self.encode_query(
            tables, predicates, joins,
        );

        // Forward pass through neural network
        let log_card = self.model.forward(&features);
        let cardinality = (10.0_f64)
            .powf(log_card as f64) as u64;

        CardinalityEstimate {
            estimate: cardinality,
            confidence: self.model.confidence(&features),
            method: EstimationMethod::MLQueryDriven,
        }
    }

    fn feedback(
        &mut self,
        query: &QueryFeatures,
        actual_cardinality: u64,
    ) {
        self.feedback_buffer.push(
            (query.clone(), actual_cardinality),
        );

        if self.feedback_buffer.len()
            >= self.retrain_threshold
        {
            self.retrain();
        }
    }
}

/// Data-driven model (autoregressive, NeuroCard-style)
pub struct DataDrivenEstimator {
    /// Autoregressive model: P(col1|) * P(col2|col1) * ...
    model: AutoregressiveModel,
    /// Column ordering for factorization
    column_order: Vec<ColumnId>,
    /// Number of samples for cardinality estimation
    num_samples: usize,
}

impl DataDrivenEstimator {
    /// Estimate cardinality using progressive sampling
    fn estimate_by_sampling(
        &self,
        predicates: &[Predicate],
    ) -> u64 {
        let table_size = self.model.table_size();
        let mut passing = 0u64;

        for _ in 0..self.num_samples {
            // Sample a row from the learned distribution
            let sample = self.model.sample();

            // Check if sample satisfies all predicates
            let passes = predicates.iter().all(|p| {
                evaluate_predicate(
                    &sample, p,
                )
            });

            if passes {
                passing += 1;
            }
        }

        // Scale by table size
        let selectivity =
            passing as f64 / self.num_samples as f64;
        (table_size as f64 * selectivity) as u64
    }

    /// Exact estimation using model integration
    fn estimate_exact(
        &self,
        predicates: &[Predicate],
    ) -> u64 {
        let table_size = self.model.table_size();

        // For each column in order, compute conditional
        // probability given predicates on prior columns
        let mut probability = 1.0_f64;

        for &col in &self.column_order {
            let relevant: Vec<_> = predicates.iter()
                .filter(|p| p.column == col)
                .collect();

            if relevant.is_empty() {
                continue; // marginalize this column
            }

            // P(col satisfies predicates | prior columns)
            let cond_prob = self.model
                .conditional_probability(
                    col, &relevant,
                );
            probability *= cond_prob;
        }

        (table_size as f64 * probability).max(1.0) as u64
    }
}

/// Hybrid estimator: traditional + ML fallback
pub struct HybridEstimator {
    traditional: HistogramEstimator,
    ml_model: Box<dyn MLCardinalityEstimator>,
    /// Use ML when traditional confidence is low
    ml_threshold: f64,
}

impl HybridEstimator {
    pub fn estimate(
        &self,
        tables: &[TableRef],
        predicates: &[Predicate],
        joins: &[JoinCondition],
    ) -> CardinalityEstimate {
        // Try traditional first
        let trad = self.traditional.estimate(
            tables, predicates, joins,
        );

        // Use ML for multi-column or join predicates
        let use_ml = joins.len() > 1
            || predicates.len() > 2
            || trad.confidence < self.ml_threshold;

        if use_ml {
            let ml = self.ml_model.estimate(
                tables, predicates, joins,
            );
            // Geometric mean of both estimates
            let combined = ((trad.estimate as f64)
                * (ml.estimate as f64)).sqrt() as u64;
            CardinalityEstimate {
                estimate: combined,
                confidence: ml.confidence,
                method: EstimationMethod::Hybrid,
            }
        } else {
            trad
        }
    }
}
```

**Restrictions:**
- Training requires representative workload or data scan
- Model inference adds 0.1-10ms to optimization time
- Models must be retrained when data distribution shifts
- Out-of-distribution predicates may produce worse estimates than histograms
- Storage overhead for model parameters (typically 1-100 MB)
- Explainability: hard to debug why a model produced a specific estimate

## Cost Model

```rust
fn ml_estimation_cost(
    num_tables: usize,
    num_predicates: usize,
    model_size_params: usize,
    training_queries: usize,
) -> MLCostAnalysis {
    // Training cost
    let training_ms = training_queries as f64
        * model_size_params as f64 * 0.001;

    // Inference cost per query
    let inference_ms = model_size_params as f64
        * 0.00001 + num_predicates as f64 * 0.1;

    // Traditional estimation cost
    let traditional_ms = num_tables as f64
        * num_predicates as f64 * 0.01;

    // Accuracy comparison (typical q-error)
    // Traditional: median 3-10x, tail 100-1000x
    // ML: median 1.5-3x, tail 5-20x
    let traditional_median_qerror = 5.0;
    let ml_median_qerror = 2.0;

    MLCostAnalysis {
        training_time_ms: training_ms,
        inference_time_ms: inference_ms,
        traditional_time_ms: traditional_ms,
        accuracy_improvement: traditional_median_qerror
            / ml_median_qerror,
    }
}
```

**Typical performance:**
- Training: minutes to hours (one-time, can be incremental)
- Inference: 0.1-5ms per subplan estimate
- Accuracy: 2-10x better than histograms for multi-column predicates
- Join estimation: 10-100x better for complex joins
- Tail accuracy: ML prevents extreme 1000x errors common with histograms

## Test Cases

### Positive: Correlated multi-column predicate

```sql
SELECT COUNT(*) FROM orders
WHERE city = 'San Francisco' AND zip_code = '94105';
-- Traditional: sel(city) * sel(zip) = 0.01 * 0.001 = 0.00001
-- Actual: 0.0008 (city and zip correlated)
-- Traditional error: 80x underestimate
-- ML model captures correlation: error < 2x
```

### Positive: Multi-table join with skew

```sql
SELECT COUNT(*)
FROM orders o
JOIN customers c ON o.customer_id = c.id
JOIN products p ON o.product_id = p.id
WHERE c.tier = 'premium' AND p.category = 'electronics';
-- Traditional: independence across all joins
-- Error: 100-1000x (tier correlates with category)
-- ML model trained on join execution: error < 5x
-- Better join order selection prevents 10x slowdown
```

### Positive: Feedback-driven improvement

```sql
-- Query executed, actual cardinality observed
-- Model receives feedback: predicted 1000, actual 50000
-- After 100 feedback samples, model retrains
-- Next similar query: predicted 48000 (within 5%)
-- Traditional histograms cannot learn from execution feedback
```

### Negative: Cold start (no training data)

```sql
-- New table loaded, no queries yet
-- ML model has no training data for this table
-- Must fall back to traditional histograms
-- Cold start requires either data scan for data-driven model
-- or workload execution for query-driven model
```

### Negative: Distribution shift

```sql
-- Model trained on January data
-- February data has different distribution (seasonal)
-- ML estimates based on stale distribution
-- Can be worse than fresh histograms
-- Solution: periodic retraining or drift detection
```

### Negative: Overhead for simple queries

```sql
SELECT * FROM users WHERE id = 42;
-- Simple point lookup: histogram is perfectly accurate
-- ML inference adds 1ms for no benefit
-- Solution: hybrid model uses ML only for complex predicates
```

## References

**Academic papers:**
- Kipf et al., "Learned Cardinalities: Estimating Correlated Joins with Deep Learning", CIDR 2019
- Yang et al., "Deep Unsupervised Cardinality Estimation (Naru)", VLDB 2019
- Yang et al., "NeuroCard: One Cardinality Estimator for All Tables", VLDB 2021
- Hilprecht et al., "DeepDB: Learn from Data, not from Queries!", VLDB 2020
- Zhu et al., "FLAT: Fast, Lightweight and Accurate Method for Cardinality Estimation", VLDB 2021
- Han et al., "Cardinality Estimation in DBMS: A Comprehensive Benchmark Evaluation", VLDB 2022

**Implementation:**
- NoisePage (CMU): Learned query optimizer integration
- PostgreSQL: pg_cardlearner extension (research prototype)
- Spark: Adaptive query execution with feedback
- Bao (Marcus et al.): Learned query optimization system
