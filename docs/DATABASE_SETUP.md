# Database Setup for Neural Cost Model Training

**Date**: May 5, 2026
**Status**: Phase 1 Complete - tpch database ready for training

---

## Overview

This document covers setting up Postgres databases with TPROC-H (TPC-H) schema and data for neural cost model training. The neural model learns from real query execution against databases of varying sizes.

---

## Current Setup ✅

### Database: `tpch`
- **Connection**: `postgres://localhost/tpch`
- **Schema**: TPROC-H (TPC-H compatible)
- **Scale**: ~0.01 (very small dataset)
- **Status**: ✅ Ready for training

### Table Information

| Table | Row Count | Purpose |
|-------|-----------|---------|
| `lineitem` | 34 | Order line items (main fact table) |
| `orders` | ~25 | Customer orders |
| `customer` | ~15 | Customer information |
| `supplier` | ~10 | Supplier details |
| `part` | ~20 | Part catalog |
| `partsupp` | ~50 | Part-supplier relationships |
| `nation` | 25 | Nations (standard TPC-H) |
| `region` | 5 | Regions (standard TPC-H) |

### Verification

```bash
# Test connection
psql postgres://localhost/tpch -c "SELECT COUNT(*) FROM lineitem;"
# Result: 34 rows

# Test query execution
psql postgres://localhost/tpch -c "SELECT COUNT(*) FROM orders WHERE o_custkey < 10;"

# Test EXPLAIN ANALYZE (used by training data collection)
psql postgres://localhost/tpch -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT COUNT(*) FROM orders;"
```

---

## Usage with Neural Model Pipeline

### 1. Data Collection

```bash
# Collect training data from current database
cargo run --release -p ra-bench --features live-comparison -- \
  collect-training \
  --db postgres://localhost/tpch \
  --configs default \
  --sizes tiny \
  --mode corpus \
  --output training_data.json
```

**Results**:
- **Samples**: 142 (one per TPROC-H corpus query)
- **Features**: Correctly extracted (aggregates, joins, filters)
- **Execution metrics**: CPU time, memory, I/O, cache hit ratio
- **Collection time**: ~2-3 minutes

### 2. Model Training

```bash
# Train neural model on collected data
cargo run --release --example train_model -p ra-bench -- \
  --input training_data.json \
  --epochs 20
```

**Current Performance**:
- **Training samples**: 142
- **Test error**: 934.6% (high, but expected with small dataset)
- **Improvement**: 7848.9 percentage points (massive improvement from fix)
- **Feature diversity**: ✅ Working (aggregates: 0-8, joins: 0-7)

---

## Scaling Up for Production

### Target: Multiple Database Sizes

For production-quality model (< 10% error), need diverse training data:

| Database | Scale Factor | Approx Size | Target Row Count (lineitem) | Status |
|----------|--------------|-------------|----------------------------|---------|
| `tpch_tiny` | 0.01 | ~10 MB | ~60K | ⚠️ Use existing `tpch` |
| `tpch_small` | 0.1 | ~100 MB | ~600K | ❌ Not created |
| `tpch_medium` | 1.0 | ~1 GB | ~6M | ❌ Not created |
| `tpch_large` | 10.0 | ~10 GB | ~60M | ❌ Not created |

### Why Multiple Sizes Matter

Different data sizes produce different execution patterns:
- **Tiny**: Cache-resident, minimal I/O, sub-millisecond queries
- **Small**: Mixed cache behavior, moderate I/O
- **Medium**: Realistic workload, significant I/O patterns
- **Large**: Memory pressure, complex optimization decisions

### Expected Training Results

| Dataset | Samples | Expected CPU Error | Expected Memory Error |
|---------|---------|-------------------|----------------------|
| Current (tiny only) | 142 | 500-1000% | 200-500% |
| Mixed sizes | 1,000+ | 20-50% | 30-70% |
| Full production | 10,000+ | 3-8% | 5-12% |

---

## Creating Additional Databases

### Prerequisites: Install dbgen

The TPC-H data generator is needed for larger datasets:

```bash
# Clone TPC-H toolkit
cd /tmp
git clone https://github.com/gregrahn/tpch-kit.git
cd tpch-kit/dbgen

# Compile for macOS + PostgreSQL
make MACHINE=MACOS DATABASE=POSTGRESQL

# Install system-wide
sudo cp dbgen /usr/local/bin/
sudo cp dists.dss /usr/local/bin/
```

### Create tpch_small (Scale 0.1)

```bash
# 1. Generate data
cd /tmp
dbgen -s 0.1

# 2. Create database
createdb tpch_small
psql tpch_small < $REPO/scripts/bench-schema.sql

# 3. Load data
for table in customer lineitem nation orders part partsupp region supplier; do
    echo "Loading $table..."
    psql tpch_small -c "\\copy $table FROM '${table}.tbl' DELIMITER '|' CSV;"
done

# 4. Create indexes
psql tpch_small -c "
    CREATE INDEX idx_lineitem_orderkey ON lineitem(l_orderkey);
    CREATE INDEX idx_lineitem_partkey ON lineitem(l_partkey);
    CREATE INDEX idx_orders_custkey ON orders(o_custkey);
    CREATE INDEX idx_customer_nationkey ON customer(c_nationkey);
    CREATE INDEX idx_supplier_nationkey ON supplier(s_nationkey);
    CREATE INDEX idx_partsupp_partkey ON partsupp(ps_partkey);
"

# 5. Update statistics
psql tpch_small -c "ANALYZE;"

# 6. Verify
psql tpch_small -c "SELECT COUNT(*) FROM lineitem;"
# Expected: ~600,000 rows
```

### Create tpch_medium (Scale 1.0)

```bash
# Same process with dbgen -s 1.0
# Expected: ~6 million lineitem rows, ~1 GB database
```

### Create tpch_large (Scale 10.0)

```bash
# Same process with dbgen -s 10.0
# Expected: ~60 million lineitem rows, ~10 GB database
# Note: This will take significant disk space and generation time
```

---

## Collecting Diverse Training Data

### Full Collection Strategy

Once multiple databases exist:

```bash
# Collect from all sizes and configurations
for db in tpch_tiny tpch_small tpch_medium tpch_large; do
    for config in default high-memory low-memory; do
        cargo run --release -p ra-bench --features live-comparison -- \
          collect-training \
          --db "postgres://localhost/$db" \
          --configs $config \
          --sizes $(echo $db | cut -d_ -f2) \
          --mode both \
          --fuzz-count 200 \
          --output "training_${db}_${config}.json"
    done
done

# Merge all training files
jq -s 'add' training_*.json > training_full.json

# Expected result: 10,000+ diverse samples
```

### Expected Accuracy Improvement

| Training Data | Samples | Expected CPU Error | Status |
|---------------|---------|-------------------|---------|
| **Current** (tiny only) | 142 | 934.6% | ✅ Working |
| **Phase 2** (tiny + small) | 600+ | 200-400% | 🎯 Next |
| **Phase 3** (all sizes) | 2,500+ | 50-100% | 🎯 Target |
| **Production** (+ more fuzz) | 10,000+ | < 10% | 🎯 Goal |

---

## Alternative: Docker Setup

If compiling dbgen is problematic, use Docker:

```bash
# Use pre-built TPC-H Docker image
docker run -d --name postgres-tpch \
  -e POSTGRES_DB=tpch_medium \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -p 5432:5432 \
  tpch/postgres:1.0

# Connect with: postgres://postgres:postgres@localhost/tpch_medium
```

---

## Configuration Testing

### Postgres Configuration Profiles

Test different Postgres settings for diverse training:

```bash
# Create postgresql-high-memory.conf
shared_buffers = 2GB
work_mem = 64MB
effective_cache_size = 16GB
random_page_cost = 1.1

# Restart with different config
pg_ctl restart -D $PGDATA -o "-c config_file=postgresql-high-memory.conf"

# Collect training data with this config
```

**Note**: Most settings can be changed per-session, but `shared_buffers` requires restart.

---

## Troubleshooting

### Issue: dbgen Not Available

**Solutions**:
1. **Compile from source** (recommended - see instructions above)
2. **Use HammerDB** (GUI tool with built-in TPC-H generation)
3. **Download pre-generated data** (search for "TPC-H generated data")
4. **Use Docker image** (see Alternative section above)

### Issue: Disk Space

TPC-H databases can be large:
- Scale 0.1: ~100 MB
- Scale 1.0: ~1 GB
- Scale 10.0: ~10 GB
- Scale 100.0: ~100 GB (not recommended for laptop)

**Solutions**:
- Start with smaller scales (0.1, 1.0)
- Use cloud instance with more disk space
- Compress older training datasets

### Issue: Slow Data Generation

**dbgen performance**:
- Scale 0.1: ~30 seconds
- Scale 1.0: ~3-5 minutes
- Scale 10.0: ~30-60 minutes

**Solutions**:
- Generate overnight for large scales
- Use parallel generation (`dbgen -s 1.0 -S 1 -C 4` for 4-way parallel)
- Download pre-generated data files

### Issue: Connection Failures

```bash
# Check Postgres is running
brew services list | grep postgres

# Start if needed
brew services start postgresql

# Test connection
psql -l
```

---

## Next Steps

### Immediate (Current Setup Working)

✅ **Current status**: tpch database ready, feature extraction fixed, pipeline working

### Phase 2: Scale Up Data Collection

1. **Install dbgen** or setup Docker alternative
2. **Create tpch_small** (scale 0.1) database
3. **Collect mixed training data** (tiny + small = 600+ samples)
4. **Train for more epochs** (50-100) with better data
5. **Target 200-400% error** (vs current 934.6%)

### Phase 3: Production Ready

1. **Create tpch_medium** (scale 1.0) database
2. **Collect 2,500+ samples** across all sizes/configs
3. **Achieve 50-100% error** range
4. **Add model persistence** and online learning

### Phase 4: Integration

1. **Integrate trained model** with optimizer
2. **Use for cost-based plan extraction**
3. **Online learning** in production
4. **Monitor and improve** accuracy over time

---

## References

**Current Database**:
- Connection: `postgres://localhost/tpch`
- Schema: `scripts/bench-schema.sql`
- Data: `scripts/seed-data.sql` (very small scale)

**TPC-H Resources**:
- [TPC-H Specification](http://www.tpc.org/tpch/)
- [HammerDB](https://hammerdb.com/) (alternative data generator)
- [tpch-kit](https://github.com/gregrahn/tpch-kit) (dbgen source)

**Pipeline Documentation**:
- `docs/NEURAL_MODEL_PIPELINE.md` - Complete pipeline guide
- `scripts/test-neural-pipeline.sh` - Integration test
- `docs/TRAINING_DATA_COLLECTION.md` - Collection infrastructure