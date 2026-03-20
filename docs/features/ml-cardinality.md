# ML Cardinality Estimation

The `ra-ml` crate provides neural network models for cardinality
prediction, trained on execution feedback.

## Overview

Traditional cardinality estimation uses histograms and independence
assumptions that often produce inaccurate estimates for correlated
columns or complex predicates. ML-based estimation learns from actual
execution data.

## Components

- **Feature extraction** -- Converts query plans into numeric feature
  vectors
- **Neural network model** -- Predicts cardinality from features
- **ML-enhanced cost estimator** -- Integrates ML predictions into the
  cost model
- **Online training** -- Updates the model from execution feedback

## Architecture

```
ra-ml/
  features.rs    Feature extraction from query plans
  nn.rs          Neural network model
  estimator.rs   ML-enhanced cost estimator
  training.rs    Online model training
```

## Further Reading

- [Cost Models](../guides/cost-models.md) -- Traditional cost
  estimation
- [Adaptive Execution](adaptive-execution.md) -- Runtime
  reoptimization using actual statistics
