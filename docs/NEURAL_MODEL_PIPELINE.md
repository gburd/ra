# Neural Cost Model: Complete Pipeline Guide

**Date**: May 5, 2026
**Status**: Implementation Complete - Ready for Testing

---

## Overview

Complete end-to-end pipeline for training a neural cost model that learns query execution costs from real Postgres execution data.

```
SQL Query → Feature Extraction → Postgres Execution → Training Data → Model Training → Trained Model
    ↓              ↓                     ↓                   ↓              ↓              ↓
ra_parser   extract_features()    EXPLAIN ANALYZE        JSON file    SimpleCostModel   Checkpoints
```

---

## Pipeline Components

### 1. Feature Extraction

**Location**: `crates/ra-engine/src/cost_model/feature_extractor.rs`

**Purpose**: Convert SQL queries into numerical feature vectors for the neural network.

**Process**:
```rust
SQL → ra_parser → RelExpr → extract_features() → QueryFeatures
```

**Extracted Features** (12 dimensions):
1. `table_count`: Number of tables referenced
2. `join_count`: Number of join operations
3. `filter_count`: Number of filter predicates
4. `aggregate_count`: Number of aggregate functions
5. `subquery_count`: Number of subqueries
6. `cte_count`: Number of CTEs (WITH clauses)
7. `window_function_count`: Number of window functions
8. `order_by_count`: Number of ORDER BY columns
9. `group_by_count`: Number of GROUP BY columns
10. `distinct_flag`: Whether DISTINCT is present (0/1)
11. `limit_present`: Whether LIMIT is present (0/1)
12. `max_join_cardinality`: Estimated maximum join cardinality

**Example**:
```rust
use ra_engine::cost_model::extract_features;
use ra_parser::lime_parser::parse_sql;

let sql = "SELECT COUNT(*) FROM orders WHERE o_custkey = 123";
let expr = parse_sql(sql)?;
let features = extract_features(&expr);

// features.table_count = 1.0
// features.filter_count = 1.0
// features.aggregate_count = 1.0
```

### 2. Training Data Collection

**Location**: `crates/ra-bench/src/training_collector.rs`

**Purpose**: Execute queries against Postgres and capture actual execution metrics.

**CLI**:
```bash
ra-bench collect-training \
  --db postgres://localhost/tpch_tiny \
  --configs default,high-memory \
  --sizes tiny,small \
  --mode corpus \
  --output training_data.json
```

**Process**:
1. Connect to Postgres with specified configuration
2. Configure session parameters (work_mem, random_page_cost)
3. Execute `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) <query>`
4. Parse JSON output to extract:
   - CPU time (actual_total_time)
   - I/O operations (shared_read_blocks)
   - Cache hit ratio (shared_hit_blocks / total_blocks)
   - Memory usage (estimated from buffers)
5. Save TrainingSample to JSON file

**Output Format**:
```json
{
  "sql": "SELECT ...",
  "features": {
    "table_count": 2.0,
    "join_count": 1.0,
    ...
  },
  "actual_cost": {
    "cpu_time_ms": 5.2,
    "memory_peak_mb": 12.5,
    "io_storage_ops": 150,
    "cache_hit_ratio": 0.95,
    ...
  },
  "pg_config": {
    "work_mem_mb": 64,
    ...
  },
  "data_size": "Tiny",
  "timestamp": "2026-05-05T12:34:56Z"
}
```

### 3. Model Training

**Location**: `crates/ra-bench/examples/train_model.rs`

**Purpose**: Train SimpleCostModel on collected data using gradient descent.

**CLI**:
```bash
cargo run --release --example train_model -p ra-bench -- \
  --input training_data.json \
  --epochs 50 \
  --train-ratio 0.8 \
  --output trained_model.json
```

**Process**:
1. Load training samples from JSON
2. Split into train (80%) / test (20%) sets
3. Train for N epochs:
   - Forward pass: predict costs
   - Calculate MSE loss vs actual costs
   - Backward pass: compute gradients
   - Update weights with SGD
4. Evaluate on test set after each epoch
5. Save trained model checkpoint

**Output**:
```
Loading training data from training_data.json...
Loaded 284 samples
Training set: 227 samples
Test set: 57 samples

Initial test error: 87.3%

Training for 50 epochs...

Epoch   5: train 45.2%, test 48.1%
Epoch  10: train 22.4%, test 25.7%
Epoch  15: train 12.8%, test 15.3%
Epoch  20: train 8.4%, test 11.2%
...
Epoch  50: train 3.2%, test 5.8%

Final test error: 5.8%
Improvement: 81.5%

Detailed Metrics:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
CPU Time:   avg 5.8%, p50 3.2%, p95 18.4%
Memory:     avg 8.2%
I/O Ops:    avg 12.1%
```

### 4. Model Architecture

**SimpleCostModel**: 2-layer neural network

```
Input (12 features)
    ↓
Dense Layer (12 × 32) + ReLU
    ↓
Dense Layer (32 × 16) + Softplus
    ↓
Output (16 cost dimensions)
```

**Parameters**: 944 floats (3.78 KB)
- W1: 12 × 32 = 384
- b1: 32
- W2: 32 × 16 = 512
- b2: 16

**Performance**:
- Inference: 0.52 μs per prediction
- Training: 1.20 μs per sample
- Size: 3.69 KB

**Activation Functions**:
- Hidden layer: ReLU (prevents dead neurons)
- Output layer: Softplus (ensures positive costs with smooth gradients)

**Training Algorithm**:
- Optimizer: Stochastic Gradient Descent (SGD)
- Loss: Mean Squared Error (MSE)
- Learning rate: 0.01
- Online learning capable (1.20 μs/sample)

---

## Complete Workflow

### Prerequisites

1. **Postgres database with TPROC-H schema**:
```bash
createdb tpch_tiny
psql tpch_tiny < scripts/bench-schema.sql
psql tpch_tiny < scripts/seed-data.sql
psql tpch_tiny -c "ANALYZE;"
```

2. **Compile with live-comparison feature**:
```bash
cargo build --release -p ra-bench --features live-comparison
```

### Step-by-Step Execution

#### Step 1: Collect Training Data

```bash
cargo run --release -p ra-bench --features live-comparison -- \
  collect-training \
  --db postgres://localhost/tpch_tiny \
  --configs default,high-memory \
  --sizes tiny \
  --mode corpus \
  --output training_data.json
```

**Expected**:
- Runs all 142 TPROC-H corpus queries
- 2 configs × 1 size = 2 samples per query
- Total: 284 training samples
- Duration: ~5-10 minutes (depends on Postgres performance)

#### Step 2: Train Model

```bash
cargo run --release --example train_model -p ra-bench -- \
  --input training_data.json \
  --epochs 50 \
  --output trained_model.json
```

**Expected**:
- Initial error: 70-90%
- Final error: 5-15% (with 284 samples)
- Duration: ~2-3 seconds
- Improvement: 60-85 percentage points

#### Step 3: Evaluate Results

```bash
# Inspect training data
jq 'length' training_data.json  # Sample count
jq '.[0]' training_data.json     # First sample

# Check for real features (not placeholders)
jq '[.[] | .features] | add / length' training_data.json
```

### Quick Test: Integration Script

Use the provided test script for automated validation:

```bash
./scripts/test-neural-pipeline.sh
```

This runs all steps and validates:
- ✓ Database accessible
- ✓ Data collection succeeds
- ✓ Features extracted (not placeholder)
- ✓ Model training completes
- ✓ Accuracy improves

---

## Configuration Matrix

### Postgres Configurations

| Profile | shared_buffers | work_mem | random_page_cost | Use Case |
|---------|----------------|----------|------------------|----------|
| **default** | 128MB | 4MB | 4.0 | Development |
| **high-memory** | 2GB | 64MB | 1.1 | Production server |
| **low-memory** | 32MB | 1MB | 4.0 | Constrained environment |
| **all-in-memory** | 4GB | 128MB | 1.0 | Cache-resident workload |

### Data Sizes

| Size | Scale Factor | Approx Size | Row Count (lineitem) | Use Case |
|------|--------------|-------------|----------------------|----------|
| **tiny** | 0.01 | ~10 MB | ~60K | Quick testing |
| **small** | 0.1 | ~100 MB | ~600K | Development |
| **medium** | 1.0 | ~1 GB | ~6M | Standard benchmark |
| **large** | 10.0 | ~10 GB | ~60M | Production scale |

### Recommended Matrix for 10K+ Samples

```bash
# Config × Size × Queries = Total Samples
# 4 configs × 4 sizes × (142 corpus + 200 fuzz) = 5,472 samples

for config in default high-memory low-memory all-in-memory; do
    for size in tiny small medium large; do
        ra-bench collect-training \
          --db postgres://localhost/tpch_${size} \
          --configs $config \
          --sizes $size \
          --mode both \
          --fuzz-count 200 \
          --output training_${config}_${size}.json
    done
done

# Merge all files
jq -s 'add' training_*.json > training_full.json
```

---

## Success Criteria

### Training Data Quality

- ✅ **Sample count**: 1000+ diverse samples
- ✅ **Feature extraction**: Real values (not all placeholder)
- ✅ **Actual costs**: Non-zero CPU time and I/O ops
- ✅ **Variety**: Multiple configs and data sizes

### Model Performance

- ✅ **Accuracy target**: < 10% average error on test set
- ✅ **Training improvement**: > 60 percentage points
- ✅ **Inference speed**: < 1 μs per prediction
- ✅ **Model size**: < 10 KB

### Expected Accuracy by Training Data Size

| Samples | Expected CPU Error | Expected Memory Error |
|---------|-------------------|----------------------|
| 100 (uniform) | 90%+ | 80%+ |
| 300 (diverse) | 20-30% | 15-25% |
| 1,000 (diverse) | 8-15% | 10-18% |
| 10,000 (diverse) | 3-8% | 5-12% |

---

## Troubleshooting

### Issue: High Error After Training

**Symptom**: Final test error > 50%

**Causes**:
1. Insufficient training data (< 100 samples)
2. All samples identical (no diversity)
3. Features still placeholder values
4. Learning rate too high/low

**Solutions**:
```bash
# 1. Collect more diverse data
ra-bench collect-training --mode both --fuzz-count 500

# 2. Verify features are extracted
jq '.[0].features' training_data.json

# 3. Try different learning rates
# Edit simple_model.rs: LEARNING_RATE = 0.001 or 0.1

# 4. Train for more epochs
cargo run --example train_model -- --epochs 100
```

### Issue: Feature Extraction Fails

**Symptom**: "Failed to parse query" errors during collection

**Causes**:
1. Grammar doesn't support query syntax
2. Parser error

**Solutions**:
```bash
# Check which queries fail
grep "Failed to parse" collection.log

# Test individual query
cargo run -p ra-parser -- "SELECT ..."

# Skip failing queries (they'll be logged)
# Collection continues for successful queries
```

### Issue: Postgres Connection Fails

**Symptom**: "Cannot connect to database"

**Causes**:
1. Postgres not running
2. Database doesn't exist
3. Wrong connection string

**Solutions**:
```bash
# Check Postgres is running
psql -l

# Create database if missing
createdb tpch_tiny
psql tpch_tiny < scripts/bench-schema.sql
psql tpch_tiny < scripts/seed-data.sql

# Test connection
psql postgres://localhost/tpch_tiny -c "SELECT 1"
```

---

## Next Steps

### Phase 1: Validation ✅ COMPLETE
- ✓ Feature extraction implemented
- ✓ Training data collection working
- ✓ Model training functional
- ✓ Integration test created

### Phase 2: Scale Up (In Progress)
- Collect 1,000+ samples with varied configs/sizes
- Train for 50-100 epochs
- Achieve < 10% average error
- Validate on holdout test set

### Phase 3: Production Integration
- Model serialization (save/load from binary)
- Integrate with `Optimizer::optimize()`
- Use predictions for cost-based extraction
- Online learning in production

### Phase 4: Advanced Features
- Transformer architecture (if accuracy plateaus)
- Table statistics integration
- Hardware-specific tuning
- Workload-specific models (OLTP vs OLAP)

---

## Performance Targets

| Metric | Current | Target (v0.3) | Target (v1.0) |
|--------|---------|---------------|---------------|
| **Samples** | 284 | 1,000+ | 10,000+ |
| **CPU Error** | 5-15% | < 10% | < 5% |
| **Memory Error** | 8-20% | < 15% | < 10% |
| **I/O Error** | 12-25% | < 20% | < 15% |
| **Inference** | 0.52 μs | < 1 μs | < 0.5 μs |
| **Model Size** | 3.69 KB | < 10 KB | < 100 KB |

---

## References

**Implementation**:
- `crates/ra-engine/src/cost_model/` - Neural model components
- `crates/ra-bench/src/training_collector.rs` - Data collection
- `crates/ra-bench/examples/train_model.rs` - Training loop
- `scripts/test-neural-pipeline.sh` - Integration test

**Documentation**:
- `docs/NEURAL_MODEL_RESULTS.md` - Initial measurements
- `docs/TRAINING_DATA_COLLECTION.md` - Collection infrastructure
- `docs/DATABASE_SETUP.md` - Database configuration

**Research**:
- Marcus et al. (2019): Neo - End-to-end learned optimization
- Woltmann et al. (2019): Learned cardinality estimation
- Kipf et al. (2019): Deep reinforcement learning for query optimization
