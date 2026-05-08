# Neural Cost Model: Learned Query Cost Estimation

**Status**: Full Pipeline Implemented (compact linear models, not transformer)
**Target**: v0.3.0

---

## Overview

The Ra neural cost model uses a domain-specific transformer to predict multi-dimensional query costs (CPU, memory, I/O, network, locks) from SQL tokens and time budget context. The model learns continuously from query execution feedback through online learning.

This **hybrid approach** combines:
- **Human knowledge**: Encoded in rewrite rules (priors)
- **Learned patterns**: Extracted from execution feedback
- **Real-time adaptation**: Online learning updates the model as Ra executes queries
- **Latency-aware**: Model encodes time budget constraints (<1ms vs unlimited)

---

## Architecture (Implemented)

```
┌─────────────────────────────────────────────────────────────┐
│  FLOW 1: HOT PATH (per-query, latency-critical)            │
│                                                              │
│  SQL → Parse → RelExpr → extract_features() → QueryFeatures │
│                              (12 dimensions)                 │
│         ↓                                                    │
│  SystemFingerprint (14 dims, ~10ns atomic read)             │
│         ↓                                                    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │ NeuralRuleSelector (26→10 linear + sigmoid)         │    │
│  │ Selects which rule groups to enable for this query  │    │
│  │ Falls back to LazyRuleCompiler if untrained         │    │
│  └────────────────────────┬────────────────────────────┘    │
│         ↓                                                    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │ E-Graph Saturation (equality saturation with egg)   │    │
│  │ + NeuralConvergenceDetector (early termination)     │    │
│  │ + RuleStallingTracker (adaptive rule demotion)      │    │
│  └────────────────────────┬────────────────────────────┘    │
│         ↓                                                    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │ HybridCostFn (egg::CostFunction implementation)     │    │
│  │                                                      │    │
│  │ per_node_cost = α × neural + (1-α) × traditional   │    │
│  │                                                      │    │
│  │ α = blend_alpha (0.0 to 0.9, never fully neural)   │    │
│  │ traditional = IntegratedCostFn (hardware + stats)    │    │
│  │ neural = NodeCostWeights (8→1 linear, ~20ns/node)   │    │
│  └────────────────────────┬────────────────────────────┘    │
│         ↓                                                    │
│  Optimized RelExpr → Execute                                │
├─────────────────────────────────────────────────────────────┤
│  FLOW 2: LEARNER (background, under load)                   │
│                                                              │
│  ExecutionFeedback { predicted_cost, actual_time_ms, ... }  │
│         ↓                                                    │
│  FeedbackCollector → MapeTracker (rolling accuracy)         │
│         ↓                                                    │
│  OnlineLearner.record() → ProductionCostModel.train_batch() │
│         ↓ (every 3200 samples)                              │
│  FastCostModel = distill(ProductionCostModel)               │
│         ↓                                                    │
│  Arc::swap(live_model, new_model)  ← zero-downtime update  │
│         ↓                                                    │
│  NeuralRuleSelector.train_batch(rule_labels)                │
└─────────────────────────────────────────────────────────────┘
```

### Model Hierarchy

```
ProductionCostModel (12→64→16, momentum SGD, ~2μs)
    │
    │ distill weights every 3200 samples
    ▼
FastCostModel (12→32→16, Box arrays, ~80ns)
    │
    │ used for whole-plan scoring + per-node prediction
    ▼
NodeCostWeights (8→1, inline in HybridCostFn, ~20ns/node)
```

---

## Model Format

### Files

```
ra-engine/cost_model/
  model.safetensors       # Binary weights (GPU-optimized, ~2-10 MB)
  model.toml              # Metadata only (human-readable, ~1 KB)
  tokenizer.json          # Vocabulary mapping
  training_log.jsonl      # Append-only execution history
```

### Binary Model (`model.safetensors`)

Uses [safetensors](https://github.com/huggingface/safetensors) format (standard for Gemma/Qwen/Llama):
- Header: JSON metadata
- Token embeddings: `[vocab_size × embed_dim]` float16 matrix (512 × 128 = 128 KB)
- Transformer layers: 4 layers × (Q, K, V, O matrices + FFN weights)
- Cost predictor heads: 16 separate heads (one per cost dimension)

**Total size**: ~2-5 MB (much smaller than general LLMs due to domain-specific vocabulary)

### Metadata (`model.toml`)

Human-readable configuration with:
- Hyperparameters (embed_dim, num_layers, num_heads)
- Training statistics (queries seen, average error)
- Cost head definitions (loss function, weight per dimension)
- Latency budget tokens
- Rule integration strategy

See `crates/ra-engine/cost_model/model.toml` for full spec.

### Tokenizer (`tokenizer.json`)

Vocabulary mapping with:
- Special tokens (PAD, UNK, BUDGET_*)
- SQL keywords (SELECT, FROM, WHERE, JOIN, ...)
- Operators (EQ, LT, GT, PLUS, MINUS, ...)
- Table names (dynamically assigned to range 100-199)
- Literals (bucketed into ranges for generalization)

**Vocab size**: 512 tokens (compact, domain-specific)

---

## Cost Dimensions (16 total)

### Core Resources
1. **cpu_time_ms**: Total CPU time for query execution
2. **memory_peak_mb**: Peak memory usage
3. **memory_avg_mb**: Average memory usage

### I/O
4. **io_storage_ops**: Number of storage I/O operations
5. **io_storage_bytes**: Bytes read/written to storage
6. **io_network_ops**: Network round-trips (distributed queries)
7. **io_network_bytes**: Network bytes transferred

### Concurrency & Locking
8. **locks_acquired**: Number of locks acquired
9. **lock_hold_time_ms**: Average lock hold time
10. **lock_contention_score**: Contention probability [0,1]

### Postgres-Specific
11. **vacuum_overhead**: VACUUM cost estimate
12. **wal_generation_bytes**: WAL bytes generated
13. **replication_lag_ms**: Expected replication lag

### System
14. **cache_hit_ratio**: Estimated cache hit ratio [0,1]
15. **page_faults**: Expected page faults
16. **context_switches**: Expected context switches

---

## Implementation Status

> **Note**: The original design called for a full transformer architecture (4 layers,
> 8 attention heads, token embeddings). This was replaced with compact linear models
> that achieve sub-100ns inference — fast enough to run inside the egg cost function
> on every node without exceeding the 5ms OLTP optimization budget.

### ✅ Phase 1: Infrastructure & Feature Extraction (DONE)

- [x] `CostVector` struct (16 cost dimensions)
- [x] `QueryFeatures` (12-dim structural feature vector)
- [x] `extract_features()` / `extract_features_with_stats()` in `feature_extractor.rs`
- [x] `TimeBudget` enum and tokenizer vocabulary

### ✅ Phase 2: Neural Cost Models (DONE)

**Implemented** (no `burn` dependency — pure Rust, zero-alloc inference):

| Model | Architecture | Inference | Purpose |
|-------|-------------|-----------|---------|
| `SimpleCostModel` | 12→32→16 (Vec) | ~600ns | Baseline, training experiments |
| `FastCostModel` | 12→32→16 (Box arrays) | ~80ns | Production inline scoring |
| `ProductionCostModel` | 12→64→16 (momentum SGD) | ~2μs | Offline training, distillation source |

**Key files:**
- `crates/ra-engine/src/cost_model/fast_model.rs`
- `crates/ra-engine/src/cost_model/production_model.rs`
- `crates/ra-engine/src/cost_model/simple_model.rs`

### ✅ Phase 3: Online Learning (DONE)

- [x] `OnlineLearner` — accumulates execution feedback, auto-trains at batch boundaries (default: 64 samples)
- [x] Checkpointing to JSON (every 3200 samples)
- [x] `fast_model_snapshot()` — distills `ProductionCostModel` → `FastCostModel`
- [x] Offline training mode for batch experiments

**Key file:** `crates/ra-engine/src/cost_model/online_learner.rs`

### ✅ Phase 4: Full-Pipeline Neural Integration (DONE)

Neural guidance at every stage of the optimization pipeline:

| Stage | Component | Location |
|-------|-----------|----------|
| Pre-saturation | `NeuralRuleSelector` (26→10 linear, online logistic regression) | `neural/rule_selector.rs` |
| During saturation | `NeuralConvergenceDetector` (epsilon/patience early termination) | `neural/saturation.rs` |
| During saturation | `RuleStallingTracker` (adaptive rule group demotion) | `neural/saturation.rs` |
| Extraction | `HybridCostFn` (blends `IntegratedCostFn` + per-node neural prediction) | `extract/hybrid_cost.rs` |
| System state | `SystemFingerprint` (56-byte lock-free state vector, ~10ns reads) | `state/fingerprint.rs` |
| Feedback | `ExecutionFeedback` + `FeedbackCollector` + `MapeTracker` | `cost_model/feedback.rs` |

**Key design decisions:**
- Blend alpha never exceeds 0.9 — traditional cost always contributes ≥10%
- Cold start safe: alpha = 0.0 until 500+ training samples accumulated
- `NeuralRuleSelector` falls back to `LazyRuleCompiler` heuristics when untrained
- Per-node neural cost: 8-dim features → 1 scalar (~20ns per node)

### ⏸️  Phase 5: Background Monitor & Production Deployment

**Remaining work:**
1. Background monitor thread polling `pg_stat_*` catalogs to update `SystemFingerprint`
2. Model versioning with automatic rollback (MAPE regression trigger)
3. A/B testing infrastructure (10% of queries use previous model version)
4. PostgreSQL extension integration (wire `FeedbackCollector` into `planner_hook.rs`)

---

## Hyperparameter Summary (Actual Implementation)

| Parameter | FastCostModel | ProductionCostModel | NeuralRuleSelector |
|-----------|---------------|--------------------|--------------------|
| **Input dim** | 12 (QueryFeatures) | 12 (QueryFeatures) | 26 (12 features + 14 fingerprint) |
| **Hidden dim** | 32 | 64 | N/A (single linear layer) |
| **Output dim** | 16 (CostVector) | 16 (CostVector) | 10 (rule group scores) |
| **Activation** | ReLU + Softplus | ReLU + Softplus | Sigmoid |
| **Learning rate** | N/A (distilled) | 0.005 (adaptive) | 0.01 (fixed) |
| **Inference** | ~80ns | ~2μs | ~200ns |
| **Training** | N/A | Momentum SGD | Online logistic regression |

### Blend Alpha Computation

The neural model's influence is gated by three confidence factors:

```
alpha = clamp(data_conf × state_stability × stats_quality, 0.0, 0.9)

where:
  data_conf      = 1 - exp(-samples_trained / 2000)  [0→1 sigmoid]
  state_stability = 1 - max(io_saturation, memory_pressure)
  stats_quality   = 1 - avg_staleness
```

This ensures: untrained model has zero influence, stressed system reduces neural weight,
stale statistics reduce neural trust.

---

## Research Foundation

**Learned Query Optimizers:**
- Marcus et al. (2019): Neo - End-to-end learned optimization
- Woltmann et al. (2019): Learned cardinality estimation
- Leis et al. (2015): Multi-dimensional cost models

**Transformer Architecture:**
- Vaswani et al. (2017): "Attention Is All You Need"
- Pham et al. (2023): "How Much Does Attention Actually Attend?"

**Classical Foundation:**
- Graefe (1995): Cost as a vector, not scalar
- Selinger et al. (1979): Dynamic programming for join ordering

---

## Why This Approach Works

| Aspect | Classical | Neo/Bao | **Ra Hybrid** |
|--------|-----------|---------|----------------|
| **Adaptability** | Fixed | Adapts | **Continuous online learning** |
| **Interpretability** | Transparent | Black box | **Linear models + feature vectors** |
| **Cold start** | Works immediately | Needs training | **Falls back to traditional costing** |
| **Cost dimensions** | Single scalar | Single scalar | **16 dimensions** |
| **Inference latency** | ~50ns | ~1ms | **~80ns (FastCostModel)** |
| **Storage** | KB of code | 100+ MB | **~10 KB (Box arrays)** |
| **Hardware** | CPU-only | GPU required | **CPU-only, SIMD-friendly** |
| **Integration** | Deep in code | External service | **Embedded in egg CostFunction** |
| **Safety** | N/A | N/A | **Blend capped at 0.9, never fully neural** |

**Key Design Decisions:**
1. **Compact linear models** — not transformers. Sub-100ns inference enables per-node scoring inside the e-graph extraction loop.
2. **Multi-dimensional prediction** — 16 separate cost dimensions (CPU, I/O, memory, locks, etc.)
3. **Confidence-gated blending** — neural influence grows with training data volume but never fully replaces traditional costing
4. **System-aware** — `SystemFingerprint` captures hardware utilization, extension capabilities, statistics quality, and workload character
5. **Full-pipeline integration** — neural model guides rule selection, saturation convergence, AND extraction (not just post-hoc re-ranking)
6. **Online learning** — execution feedback drives continuous model improvement without redeployment

---

## Next Steps

Remaining work to complete the neural pipeline:

1. **Background monitor thread** — Poll `pg_stat_bgwriter`, `pg_extension`,
   `pg_class` catalogs to keep `SystemFingerprint` current (~1s hardware,
   ~30s capabilities)

2. **Wire feedback into PostgreSQL extension** — Connect `FeedbackCollector`
   to `planner_hook.rs` post-execution path; update `SystemFingerprint`
   with rolling MAPE

3. **Model safety** — Implement rollback trigger (if MAPE exceeds 2x
   previous version over 100 queries, revert to previous `FastCostModel`)

4. **Remove legacy variant re-ranking** — The `plan_variants.rs` +
   `extract_best_with_neural()` path is superseded by `HybridCostFn`;
   remove once integration tests confirm equivalent or better plan quality

---

## Current Status

**✅ Phase 1 Complete**: Infrastructure skeleton implemented
- Model metadata defined
- Tokenizer vocabulary created
- Rust module structure in place
- Design fully documented

**⏸️  Blocked**: Phases 2-5 require burn ML framework
- Dependency version resolution needed
- Transformer implementation pending
- Full integration deferred to v0.3.0

**Alternative**: Consider using simpler ML backends (linfa, smartcore) for initial prototype before committing to burn's transformer architecture.

---

## References

- Design document: `/Users/gregburd/src/ra/docs/NEURAL_COST_MODEL.md`
- Model metadata: `/Users/gregburd/src/ra/crates/ra-engine/cost_model/model.toml`
- Tokenizer vocab: `/Users/gregburd/src/ra/crates/ra-engine/cost_model/tokenizer.json`
- Implementation: `/Users/gregburd/src/ra/crates/ra-engine/src/cost_model/`
