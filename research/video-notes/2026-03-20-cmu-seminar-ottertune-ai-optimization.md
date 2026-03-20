# CMU Seminar: OtterTune - AI-Powered Database Optimization

**Source:** https://db.cs.cmu.edu/seminar2023/ (ML-DB series)
**Date:** 2023-09-18
**Speaker:** Dana Van Aken

## Key Points
- Machine learning for automatic database configuration tuning
- Learned cost models from execution history
- Transfer learning across workloads and hardware
- Addresses the "knob tuning problem" in database systems

## Optimization Techniques

### Automatic Knob Tuning
- Identify most impactful configuration parameters
- Gaussian Process regression for cost prediction
- Bayesian optimization for exploring configuration space
- Start from default, iteratively improve

### Learned Cost Models
- Train ML models on (query, plan, runtime) triples
- More accurate than hand-tuned cost formulas
- Adapt to specific hardware and workload
- Challenge: training data collection overhead

### Transfer Learning
- Pre-train on one workload/hardware
- Fine-tune for new deployment
- Reduces cold-start problem
- Cross-database knowledge transfer

### Index Recommendation
- ML-based index selection
- Consider workload mix, not individual queries
- Account for index maintenance overhead
- Continuous adaptation to workload changes

## Applicable to RA
- RA has experimental/ml-guided/ (9 rules) but limited
- Gap: No automatic cost model calibration from workload history
- Gap: No ML-based configuration tuning
- Gap: No workload-aware index recommendation
- Gap: No transfer learning for cost models across deployments
- Gap: No learned cardinality estimation integration

## References
- Van Aken et al. "Automatic Database Management System Tuning Through Large-scale Machine Learning" (2017)
- Marcus & Papaemmanouil. "Neo: A Learned Query Optimizer" (2019)
