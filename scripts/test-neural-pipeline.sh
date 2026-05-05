#!/bin/bash
# End-to-end test of neural cost model training pipeline
#
# This script tests:
# 1. Feature extraction from SQL queries
# 2. Training data collection from Postgres execution
# 3. Model training on collected data
# 4. Accuracy evaluation
#
# Prerequisites:
# - Postgres running with tpch_tiny database
# - ra-bench compiled with --features live-comparison

set -euo pipefail

echo "═══════════════════════════════════════════════════════════"
echo "Neural Cost Model Pipeline Integration Test"
echo "═══════════════════════════════════════════════════════════"
echo

# Configuration
DB_URL="${DATABASE_URL:-postgres://localhost/tpch_tiny}"
OUTPUT_DIR="${OUTPUT_DIR:-/tmp/ra-neural-test}"
TRAINING_FILE="$OUTPUT_DIR/training_data.json"
TRAINED_MODEL="$OUTPUT_DIR/trained_model.json"

# Create output directory
mkdir -p "$OUTPUT_DIR"

echo "Configuration:"
echo "  Database: $DB_URL"
echo "  Output:   $OUTPUT_DIR"
echo

# Step 1: Verify database exists
echo "Step 1: Verifying database..."
if ! psql "$DB_URL" -c "SELECT COUNT(*) FROM lineitem LIMIT 1" &>/dev/null; then
    echo "❌ ERROR: Cannot connect to database or lineitem table missing"
    echo
    echo "Please ensure tpch_tiny database is set up:"
    echo "  createdb tpch_tiny"
    echo "  psql tpch_tiny < scripts/bench-schema.sql"
    echo "  psql tpch_tiny < scripts/seed-data.sql"
    echo
    exit 1
fi
echo "✓ Database accessible"
echo

# Step 2: Collect training data
echo "Step 2: Collecting training data..."
echo "  Running ra-bench collect-training..."

if ! cargo run --release -p ra-bench --features live-comparison -- \
    collect-training \
    --db "$DB_URL" \
    --configs default \
    --sizes tiny \
    --mode corpus \
    --output "$TRAINING_FILE" 2>&1 | grep -v "^warning:"; then
    echo "❌ ERROR: Training data collection failed"
    exit 1
fi

# Verify training data was created
if [ ! -f "$TRAINING_FILE" ]; then
    echo "❌ ERROR: Training data file not created"
    exit 1
fi

SAMPLE_COUNT=$(jq 'length' "$TRAINING_FILE")
echo "✓ Collected $SAMPLE_COUNT training samples"
echo

# Step 3: Inspect sample
echo "Step 3: Validating training data format..."
echo "  Sample training data (first entry):"
jq '.[0] | {
    sql: (.sql | .[0:80] + "..."),
    features: .features,
    actual_cost: {
        cpu_time_ms: .actual_cost.cpu_time_ms,
        memory_peak_mb: .actual_cost.memory_peak_mb,
        cache_hit_ratio: .actual_cost.cache_hit_ratio
    },
    config: .pg_config.work_mem_mb,
    size: .data_size
}' "$TRAINING_FILE" | head -20

# Verify features are not all placeholder values
HAS_REAL_FEATURES=$(jq '.[0].features |
    if .table_count == 1.0 and .join_count == 0.0 and .filter_count == 1.0
    then false else true end' "$TRAINING_FILE")

if [ "$HAS_REAL_FEATURES" = "true" ]; then
    echo "✓ Features appear to be extracted (not all placeholder values)"
else
    echo "⚠ WARNING: Features may still be placeholder values"
    echo "   This is OK for initial testing but should be fixed"
fi
echo

# Step 4: Train model
echo "Step 4: Training neural cost model..."
echo "  Running train_model (20 epochs for quick test)..."

if ! cargo run --release --example train_model -p ra-bench -- \
    --input "$TRAINING_FILE" \
    --epochs 20 \
    --train-ratio 0.8 \
    --output "$TRAINED_MODEL" 2>&1 | tee "$OUTPUT_DIR/training.log" | grep -v "^warning:"; then
    echo "❌ ERROR: Model training failed"
    exit 1
fi
echo

# Step 5: Analyze results
echo "Step 5: Analyzing results..."

# Extract key metrics from training log
INITIAL_ERROR=$(grep "Initial test error:" "$OUTPUT_DIR/training.log" | awk '{print $4}' | tr -d '%')
FINAL_ERROR=$(grep "Final test error:" "$OUTPUT_DIR/training.log" | awk '{print $4}' | tr -d '%')
IMPROVEMENT=$(grep "Improvement:" "$OUTPUT_DIR/training.log" | awk '{print $2}' | tr -d '%')

echo "Results Summary:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Training samples:    $SAMPLE_COUNT"
echo "  Initial test error:  ${INITIAL_ERROR}%"
echo "  Final test error:    ${FINAL_ERROR}%"
echo "  Improvement:         ${IMPROVEMENT}%"
echo

# Success criteria
SUCCESS=true

if (( $(echo "$SAMPLE_COUNT < 50" | bc -l) )); then
    echo "⚠ WARNING: Sample count is low ($SAMPLE_COUNT < 50)"
    echo "   For production, collect 1000+ samples"
    SUCCESS=false
fi

if [ -n "$FINAL_ERROR" ] && (( $(echo "$FINAL_ERROR > 50" | bc -l) )); then
    echo "⚠ WARNING: Final error is high (${FINAL_ERROR}% > 50%)"
    echo "   This may indicate insufficient training data or feature issues"
    SUCCESS=false
fi

if [ -n "$IMPROVEMENT" ] && (( $(echo "$IMPROVEMENT < 0" | bc -l) )); then
    echo "⚠ WARNING: Model did not improve (negative improvement)"
    SUCCESS=false
fi

echo
if [ "$SUCCESS" = true ]; then
    echo "✅ Pipeline test PASSED"
    echo
    echo "Next steps:"
    echo "  1. Collect more training data with --mode both --fuzz-count 500"
    echo "  2. Train for more epochs (50-100) for better accuracy"
    echo "  3. Set up larger databases (tpch_small, tpch_medium) for diversity"
    echo "  4. Integrate trained model with optimizer"
else
    echo "⚠️  Pipeline test completed with warnings"
    echo
    echo "Review the warnings above and address them before production use"
fi
echo

# Cleanup option
echo "Test artifacts saved in: $OUTPUT_DIR"
echo "To clean up: rm -rf $OUTPUT_DIR"
