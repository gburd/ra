# Rule: Learned Cardinality Estimation

**Category:** experimental/ml-guided
**File:** `rules/experimental/ml-guided/learned-cardinality.rra`

## Metadata

- **ID:** `learned-cardinality`
- **Version:** "1.0.0"
- **Databases:** postgresql
- **Tags:** ml, cardinality, deep-learning, query-optimization
- **Authors:** "Kipf et al. 2019", "RA Contributors"


# Learned Cardinality Estimation

## Description

Replaces traditional cardinality estimation (histograms, sampling) with a
deep learning model that predicts join cardinalities directly from query
structure. Models like MSCN (Multi-Set Convolutional Networks) and NeuroCard
learn from query workload history to provide more accurate estimates,
especially for complex predicates and correlated columns.

**When to apply**: Queries with complex predicates, multi-column correlations,
or when traditional statistics (histograms, samples) provide poor estimates.

**Why it works**: Deep learning models can capture complex data distributions
and correlations that traditional statistics miss. By training on actual
query executions, learned models avoid independence assumptions and provide
estimates closer to true cardinalities.

## Relational Algebra

```algebra
CardEst_traditional(join[R.a = S.b](filter[R.x > 10](R), S))
  -> CardEst_learned(join[R.a = S.b](filter[R.x > 10](R), S))
  where model_trained_on_workload()

Model inputs:
- Query structure (join graph, predicates)
- Table sizes, column statistics
- Predicate values

Model output:
- Predicted cardinality with confidence interval
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Integration with optimizer
rw!("learned-cardinality";
    "(cardinality ?query)" =>
    "(learned_cardinality (featurize ?query) (load_model))"
    if model_available()
    if workload_coverage("?query") > 0.7
),

// Learned cardinality estimation pipeline
struct LearnedCardinalityEstimator {
    model: Box<dyn CardinalityModel>,
    feature_extractor: FeatureExtractor,
    fallback_estimator: Box<dyn CardinalityEstimator>,
}

impl LearnedCardinalityEstimator {
    fn estimate(&self, query: &RelExpr) -> CardinalityEstimate {
        // Extract features from query
        let features = self.feature_extractor.extract(query);

        // Check if query is in model's domain
        if !self.is_covered(&features) {
            return self.fallback_estimator.estimate(query);
        }

        // Run inference
        let prediction = self.model.predict(&features);

        CardinalityEstimate {
            estimate: prediction.mean,
            confidence: prediction.confidence,
            lower_bound: prediction.quantile_05,
            upper_bound: prediction.quantile_95,
        }
    }

    fn is_covered(&self, features: &QueryFeatures) -> bool {
        // Check if query is similar to training distribution
        self.model.coverage_score(features) > 0.7
    }
}

// Feature extraction for MSCN-style model
struct FeatureExtractor;

impl FeatureExtractor {
    fn extract(&self, query: &RelExpr) -> QueryFeatures {
        QueryFeatures {
            // Table features (one-hot or embedding)
            tables: self.extract_tables(query),

            // Join structure (join graph adjacency matrix)
            joins: self.extract_join_graph(query),

            // Predicate features (type, operator, literal values)
            predicates: self.extract_predicates(query),

            // Column statistics (min, max, distinct count)
            column_stats: self.extract_column_stats(query),
        }
    }

    fn extract_join_graph(&self, query: &RelExpr) -> JoinGraph {
        // Build join graph as adjacency matrix
        // Nodes: tables, Edges: join predicates
        let tables = collect_tables(query);
        let mut adj_matrix = vec![vec![0.0; tables.len()]; tables.len()];

        for join_pred in collect_join_predicates(query) {
            let (t1, t2) = join_pred.table_pair();
            let idx1 = tables.iter().position(|t| t == t1).unwrap();
            let idx2 = tables.iter().position(|t| t == t2).unwrap();
            adj_matrix[idx1][idx2] = 1.0;
            adj_matrix[idx2][idx1] = 1.0;
        }

        JoinGraph { adj_matrix }
    }

    fn extract_predicates(&self, query: &RelExpr) -> Vec<PredicateFeature> {
        collect_predicates(query)
            .iter()
            .map(|pred| PredicateFeature {
                operator: pred.operator_encoding(), // eq=0, lt=1, gt=2, ...
                literal_value: pred.normalized_value(), // Normalized [0, 1]
                column_id: pred.column_id(),
                selectivity_hint: pred.historical_selectivity(),
            })
            .collect()
    }
}

// Model architectures
trait CardinalityModel {
    fn predict(&self, features: &QueryFeatures) -> Prediction;
    fn coverage_score(&self, features: &QueryFeatures) -> f64;
}

// MSCN: Multi-Set Convolutional Network
struct MSCN {
    table_embeddings: Vec<Vec<f32>>,
    join_encoder: ConvolutionalEncoder,
    predicate_encoder: SetEncoder,
    fusion_network: FeedForwardNetwork,
}

impl CardinalityModel for MSCN {
    fn predict(&self, features: &QueryFeatures) -> Prediction {
        // Embed tables
        let table_embeds = features
            .tables
            .iter()
            .map(|t| &self.table_embeddings[*t])
            .collect::<Vec<_>>();

        // Encode join graph via graph convolution
        let join_encoding = self
            .join_encoder
            .encode(&features.joins, &table_embeds);

        // Encode predicates as set
        let pred_encoding =
            self.predicate_encoder.encode(&features.predicates);

        // Fuse and predict
        let combined = concat(&join_encoding, &pred_encoding);
        let logits = self.fusion_network.forward(&combined);

        // Output: log(cardinality)
        let log_card = logits[0];
        let confidence = sigmoid(logits[1]);

        Prediction {
            mean: log_card.exp(),
            confidence,
            quantile_05: (log_card - 1.65).exp(),
            quantile_95: (log_card + 1.65).exp(),
        }
    }

    fn coverage_score(&self, features: &QueryFeatures) -> f64 {
        // Estimate if query is within training distribution
        // Use distance to nearest training query in embedding space
        self.compute_similarity(features)
    }
}
```

**Restrictions:**
- Requires training on representative workload (100K+ queries)
- Model may not generalize to unseen query patterns
- Inference latency: 1-10ms (acceptable for optimization, not execution)
- Needs periodic retraining as data distribution changes

## Cost Model

```rust
fn estimated_benefit(
    query: &RelExpr,
    traditional_estimate: f64,
    true_cardinality: f64, // from execution or sample
) -> f64 {
    // Traditional estimator error (q-error)
    let trad_error = if traditional_estimate > true_cardinality {
        traditional_estimate / true_cardinality
    } else {
        true_cardinality / traditional_estimate
    };

    // Learned model typically achieves 2-5x lower q-error
    let learned_error = trad_error / 3.0; // Assume 3x improvement

    // Cost impact: cardinality errors compound exponentially
    let trad_plan_cost = estimate_plan_cost(query, traditional_estimate);
    let learned_plan_cost = estimate_plan_cost(query, learned_estimate);

    // Model inference overhead
    let inference_latency_ms = 5.0;

    if trad_plan_cost > learned_plan_cost + inference_latency_ms {
        (trad_plan_cost - learned_plan_cost) / trad_plan_cost
    } else {
        0.0
    }
}
```

**Assumptions:**
- Model trained on at least 50K executed queries
- Training includes joins, filters, aggregations
- Model captures correlations traditional stats miss
- Typical q-error improvement: 2-5x over PostgreSQL histograms

**Typical benefit**: 30-80% better plan quality for queries with:
- Multi-column correlations
- Complex predicates (UDFs, string matching)
- High-selectivity joins

## Test Cases

### Positive: Correlated columns

```sql
-- Traditional stats assume independence: price ⊥ category
-- Reality: electronics more expensive than books
SELECT COUNT(*)
FROM products
WHERE category = 'electronics' AND price > 500;

-- Traditional: |products| * sel(category) * sel(price) = 10M * 0.2 * 0.1 = 200K
-- Learned model: Recognizes correlation, estimates 50K (closer to true 48K)
```

### Positive: Complex join with string predicates

```sql
SELECT COUNT(*)
FROM reviews r
JOIN products p ON r.product_id = p.id
WHERE r.rating >= 4
  AND p.name LIKE '%laptop%'
  AND r.text LIKE '%recommend%';

-- Traditional: Independence assumption leads to 100x underestimate
-- Learned: Captures correlation between high ratings and "recommend" keyword
```

### Negative: Simple single-table query

```sql
SELECT COUNT(*) FROM users WHERE id = 12345;

-- Traditional statistics sufficient (index lookup)
-- ML model overhead not justified
```

## References

**Academic papers:**
- Kipf et al., "Learned Cardinalities: Estimating Correlated Joins with Deep Learning", CIDR 2019
- Yang et al., "NeuroCard: One Cardinality Estimator for All Tables", VLDB 2021
- Hilprecht et al., "DeepDB: Learn from Data, not from Queries!", SIGMOD 2020
- Marcus et al., "Bao: Making Learned Query Optimization Practical", SIGMOD 2021

**Implementation:**
- Kipf's MSCN: https://github.com/andreaskipf/learnedcardinalities
- NeuroCard: https://github.com/neurocard/neurocard
- PostgreSQL + ML: pg_learned (experimental extension)

**Key insights:**
- Set-based models (MSCN) handle varying query structure
- Autoregressive models (NeuroCard) provide probabilistic estimates
- Training: supervised learning on (query, true_cardinality) pairs
- Deployment: model in optimizer, fallback to traditional stats
- Retraining: weekly/monthly as data distribution evolves
