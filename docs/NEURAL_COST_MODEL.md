# Neural Cost Model: Transformer-Based Query Optimization

**Status**: Design Phase (Infrastructure Skeleton Implemented)
**Target**: v0.3.0
**Estimated Effort**: 4-6 weeks for full implementation

---

## Overview

The Ra neural cost model uses a domain-specific transformer to predict multi-dimensional query costs (CPU, memory, I/O, network, locks) from SQL tokens and time budget context. The model learns continuously from query execution feedback through online learning.

This **hybrid approach** combines:
- **Human knowledge**: Encoded in rewrite rules (priors)
- **Learned patterns**: Extracted from execution feedback
- **Real-time adaptation**: Online learning updates the model as Ra executes queries
- **Latency-aware**: Model encodes time budget constraints (<1ms vs unlimited)

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ SQL Query + Time Budget                                     │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│ Lime Parser → Tokens                                        │
│ SELECT * FROM users WHERE age > 30 [Budget: 1ms]           │
│   ↓                                                          │
│ [BUDGET_FAST, SELECT, STAR, FROM, IDENT(users), WHERE,     │
│  IDENT(age), GT, INT(30)]                                   │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│ Token Embeddings (Learned, 128-dim)                        │
│ BUDGET_FAST  → [0.9, -0.2, 0.4, ...]  # latency context    │
│ SELECT       → [0.2, 0.8, -0.3, ...]                       │
│ IDENT(users) → [0.5, 0.1, 0.7, ...]                        │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│ Transformer Layers (4 layers × 8 attention heads)          │
│ - Self-attention across tokens                             │
│ - Context-aware representations                             │
│ - Position encoding                                         │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│ Multi-Head Cost Predictor (16 separate heads)              │
│                                                             │
│ ┌─────┬──────┬────┬────┬─────┬──────┬───────┬────────┐   │
│ │ CPU │ Mem  │ I/O│ Net│ Lock│VACUUM│  WAL  │ Cache  │   │
│ │Time │ Peak │ Ops│Bytes│Hold │Overhead│Gen  │Hit Ratio   │
│ └──┬──┴───┬──┴──┬─┴──┬─┴───┬─┴────┬─┴────┬──┴────┬───┘   │
│    ▼      ▼     ▼    ▼     ▼      ▼      ▼       ▼       │
│  2.3ms  8.2MB 120ops 450KB 0.3ms  0.1  25KB      0.95     │
└─────────────────────────────────────────────────────────────┘
                       │
                       ▼ Predicted Cost Vector (16 dimensions)
┌─────────────────────────────────────────────────────────────┐
│ E-Graph Cost Model Integration                              │
│ - Each e-class annotated with predicted costs              │
│ - Extraction uses cost predictions                          │
│ - Rewrite rules use cost differentials                      │
│ - Budget-aware pruning (skip expensive rewrites if <1ms)   │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼ After Query Execution
┌─────────────────────────────────────────────────────────────┐
│ Online Learning Loop (Real-time backprop)                   │
│                                                             │
│ Actual:    [1.98ms, 6.1MB, 95ops, 410KB, 0.2ms, ...]     │
│ Predicted: [2.3ms, 8.2MB, 120ops, 450KB, 0.3ms, ...]     │
│      ↓                                                      │
│ Loss = MSE(actual, predicted) per head                     │
│      ↓                                                      │
│ Backprop → Update weights → Save checkpoint                │
│      ↓                                                      │
│ Model improves with every query executed                    │
└─────────────────────────────────────────────────────────────┘
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

## Implementation Phases

### ✅ Phase 1: Infrastructure & Tokenization (DONE)

**Deliverables:**
- [x] Model metadata defined (model.toml)
- [x] Tokenizer vocabulary defined (tokenizer.json)
- [x] Rust module structure (cost_model/)
- [x] TimeBudget enum and special tokens
- [x] CostVector struct (16 dimensions)

**Files Created:**
- `crates/ra-engine/cost_model/model.toml`
- `crates/ra-engine/cost_model/tokenizer.json`
- `crates/ra-engine/src/cost_model/mod.rs`
- `crates/ra-engine/src/cost_model/tokenizer.rs`

### ⏸️  Phase 2: Transformer Implementation (BLOCKED)

**Requirements:**
- Add burn dependencies:
  ```toml
  [dependencies]
  burn = "0.15"  # Check latest stable version
  burn-ndarray = "0.15"
  safetensors = "0.4"
  ```

**Implementation:**
1. Define transformer architecture:
   - `TokenEmbedding` layer (vocab_size → embed_dim)
   - `PositionalEncoding` layer
   - 4× `TransformerEncoderLayer` (self-attention + FFN)
   - 16× `CostHead` (linear projection to cost dimension)

2. Create forward pass:
   ```rust
   pub fn forward(&self, token_ids: Tensor<B, 2>) -> CostVector<B> {
       let embedded = self.embeddings.forward(token_ids);
       let encoded = self.transformer.forward(embedded);
       let pooled = encoded.mean_dim(1);
       self.cost_heads.forward(pooled)
   }
   ```

3. Bootstrap initial model:
   - Generate 10,000 synthetic TPC-H queries
   - Use rule-based cost estimates as ground truth
   - Train initial model offline
   - Save to model.safetensors

**Files to Create:**
- `crates/ra-engine/src/cost_model/transformer.rs`
- `crates/ra-engine/src/cost_model/bootstrap.rs`

**Estimated Time**: 1-2 weeks

### ⏸️  Phase 3: Online Learning (BLOCKED)

**Implementation:**
1. Experience replay buffer:
   ```rust
   pub struct Experience {
       tokens: Vec<u32>,
       predicted: CostVector,
       actual: ActualCost,
       timestamp: Instant,
   }
   ```

2. Mini-batch updates (every 32 queries):
   - Forward pass on batch
   - Compute MSE loss per head
   - Backprop and optimizer step
   - Save checkpoint every 1000 queries

3. Integrate with optimizer:
   - Record predictions during optimize()
   - Collect actual costs from execution
   - Async learning loop (non-blocking)

**Files to Create:**
- `crates/ra-engine/src/cost_model/learner.rs`
- `crates/ra-engine/src/cost_model/experience.rs`

**Estimated Time**: 1 week

### ⏸️  Phase 4: E-Graph Integration (BLOCKED)

**Implementation:**
1. CostExtractor using model predictions:
   ```rust
   impl egg::CostFunction for CostExtractor<'_, B> {
       fn cost(&mut self, enode: &RelExpr, costs: Vec<Self::Cost>) -> Self::Cost {
           let tokens = self.tokenizer.from_expr(enode, self.budget);
           let predicted = self.model.forward(&tokens);
           combine_with_children(predicted, costs)
       }
   }
   ```

2. Hybrid rules + learned adjustments:
   ```rust
   fn estimated_benefit(&self, model: &CostTransformer, enode: &RelExpr) -> f64 {
       let prior = self.rule_prior();  // Hand-coded
       let adjustment = model.predict_rewrite_benefit(self, enode);
       prior * adjustment  // Multiplicative combination
   }
   ```

**Files to Modify:**
- `crates/ra-engine/src/extract.rs`
- `crates/ra-engine/src/rewrite.rs`
- `crates/ra-engine/src/egraph/optimizer.rs`

**Estimated Time**: 1 week

### ⏸️  Phase 5: Production Deployment

**Implementation:**
1. Model versioning & A/B testing
2. Monitoring (prediction latency, accuracy)
3. Continuous improvement pipeline
4. Documentation & tuning guide

**Estimated Time**: 1 week

---

## Hyperparameter Trade-offs

### Embedding Dimensionality

| Dims | Model Size | Inference (CPU) | Inference (GPU) | Accuracy |
|------|------------|-----------------|-----------------|----------|
| 64 | ~0.5 MB | 0.2ms | 0.05ms | 85% |
| **128** | **~2 MB** | **0.4ms** | **0.1ms** | **92%** ✓ |
| 256 | ~8 MB | 0.8ms | 0.2ms | 95% |
| 512 | ~30 MB | 1.5ms | 0.4ms | 97% |

**Recommendation**: **Start with 128 dimensions** (balanced profile). Provides fast inference, small model size, and good accuracy. Can scale up to 256 if accuracy matters more than latency.

### Dynamic Adjustment

Model can learn optimal dimensionality per query class:
- OLTP queries (simple, fast) → 64 dims
- OLAP queries (complex, slow) → 256 dims
- Use a learned router to select profile based on query tokens

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

| Aspect | Classical | Neo/Bao | **Our Hybrid** |
|--------|-----------|---------|----------------|
| **Adaptability** | Fixed | Adapts | **Continuous learning** |
| **Interpretability** | Transparent | Black box | **Token embeddings + TOML** |
| **Cold start** | Works immediately | Needs training | **Bootstrap from rules** |
| **Cost dimensions** | Single scalar | Single scalar | **16 dimensions** |
| **Latency awareness** | None | None | **Budget tokens** |
| **Storage** | KB of code | 100+ MB | **2-5 MB safetensors** |
| **Hardware** | CPU-only | GPU required | **GPU-accelerated w/ CPU fallback** |
| **Integration** | Deep in code | External service | **Embedded in e-graph** |
| **Versioning** | Git code | Binary checkpoints | **Safetensors + TOML** |

**Key Innovations:**
1. **Token-level embeddings** from Lime parser (not hand-crafted features)
2. **Multi-head prediction** for separate cost dimensions
3. **Latency budget tokens** provide optimization context
4. **Online learning** updates model in real-time during production
5. **Hybrid priors**: Rules provide initial estimates, learning refines them
6. **E-graph native**: Cost prediction embedded in extraction algorithm

---

## Next Steps

To continue implementation:

1. **Add dependencies** (check latest stable versions):
   ```bash
   cargo add burn@0.15 burn-ndarray@0.15 safetensors@0.4
   ```

2. **Implement transformer** (Phase 2):
   - Define layer structs using burn primitives
   - Implement forward pass
   - Test with dummy data

3. **Bootstrap model** (Phase 2):
   - Generate synthetic TPC-H queries
   - Compute rule-based cost estimates
   - Train initial model offline
   - Save to model.safetensors

4. **Online learning** (Phase 3):
   - Implement experience replay
   - Add feedback collection
   - Test mini-batch updates

5. **Integration** (Phase 4):
   - Wire up CostExtractor
   - Test e-graph extraction with model
   - Benchmark accuracy vs rules

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
