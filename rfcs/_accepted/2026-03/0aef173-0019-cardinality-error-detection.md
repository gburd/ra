# RFC 0019: Cardinality Estimation Error Detection

**Status:** Accepted
**Implemented:** 2026-03-20
**Commit:** 0aef173

## Summary

Implemented q-error tracking and classification to detect when optimizer cardinality estimates diverge from actual row counts during execution. The system identifies stale statistics, missing correlations, and inadequate estimation techniques, providing actionable recommendations for improvement.

## Motivation

Cardinality estimation errors are the root cause of most bad query plans. Even sophisticated optimizers struggle when estimates are orders of magnitude off. By detecting and classifying these errors, we can:

- Identify tables needing statistics refresh
- Detect missing multi-column statistics
- Recognize estimation technique limitations
- Provide targeted recommendations
- Track estimation quality over time

## Technical Design

### Q-Error Metric

Q-error quantifies estimation accuracy:
```
q_error = max(estimated/actual, actual/estimated)
```

Properties:
- Always ≥ 1.0 (perfect estimate = 1.0)
- Symmetric for over/under estimation
- Geometrically meaningful (log scale)

### Error Classification

**Severity Levels:**
- **Low**: q-error < 2.0 (acceptable deviation)
- **Medium**: 2.0 ≤ q-error < 10.0 (needs attention)
- **High**: q-error ≥ 10.0 (critical issue)

**Error Categories:**
- **Stale Statistics**: Table modified since last ANALYZE
- **Missing Correlation**: Multi-column selectivity issues
- **Outlier Values**: Skewed distributions
- **Join Estimation**: Correlation between join keys

### Architecture

**`ra-stats/feedback` module:**
- `QError` struct for metric calculation
- `CardinalityFeedback` for tracking per-operator errors
- `ErrorClassification` for severity and category
- `RecommendationEngine` for actionable advice

**`ra-pg-monitor/error_detection` module:**
- Bridges feedback into monitoring dashboard
- Aggregates errors by table and operator
- Generates advisory messages
- Tracks trends over time

### Recommendation Engine

Automated recommendations based on error patterns:

1. **ANALYZE Command**: For stale statistics
   ```sql
   ANALYZE table_name;
   ```

2. **Extended Statistics**: For column correlation
   ```sql
   CREATE STATISTICS stat_name (dependencies)
   ON col1, col2 FROM table_name;
   ```

3. **Histogram Increase**: For outlier handling
   ```sql
   ALTER TABLE table_name
   ALTER COLUMN col_name
   SET STATISTICS 1000;
   ```

4. **Index Creation**: For missing access paths
   ```sql
   CREATE INDEX ON table_name(col_name);
   ```

## Implementation

### Key Files

- `crates/ra-stats/src/feedback.rs` (1056 lines)
  - `QError` calculation and tracking
  - `CardinalityFeedback` collection
  - `ErrorClassification` logic
  - `RecommendationEngine` implementation

- `crates/ra-pg-monitor/src/error_detection.rs` (284 lines)
  - `ErrorDetector` for monitoring integration
  - `TableErrorSummary` aggregation
  - Dashboard widget rendering

### Data Flow

1. Executor reports actual row counts
2. Feedback module calculates q-error
3. Classifier determines severity/category
4. Recommendations generated
5. Monitor displays alerts and advice

## Testing

Comprehensive test coverage:
- Q-error calculation accuracy
- Classification thresholds
- Recommendation generation
- Edge cases (zero rows, NULL values)
- Integration with monitoring

## Monitoring Integration

Dashboard displays:
- Real-time q-error heatmap
- Top estimation errors by table
- Recommendation queue
- Historical trends
- Alert thresholds

## Use Cases

- **Development**: Identify problematic queries early
- **Production**: Monitor estimation drift
- **Tuning**: Prioritize statistics maintenance
- **Debugging**: Root cause analysis for bad plans

## Performance Impact

Minimal overhead:
- Lightweight q-error calculation
- Asynchronous feedback processing
- Configurable sampling rate
- Optional detailed tracking

## References

- Moerkotte et al. "Preventing Bad Plans by Bounding the Impact of Cardinality Estimation Errors" (2009)
- Leis et al. "How Good Are Query Optimizers, Really?" (2015)
- PostgreSQL pg_stat_statements q-error tracking

## Future Work

- Machine learning for estimation improvement
- Automatic statistics refresh triggers
- Join correlation detection
- Workload-aware sampling strategies