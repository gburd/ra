# Database Setup for Neural Cost Model Training

**Date**: May 5, 2026
**Status**: Phase 2 Complete ✅ - Two databases ready, model trained, 131.9% error achieved

---

## Overview

This document covers setting up Postgres databases with TPROC-H (TPC-H) schema and data for neural cost model training. The neural model learns from real query execution against databases of varying sizes.

---

## Current Setup ✅

### Database 1: `tpch` (Tiny)
- **Connection**: `postgres://localhost/tpch`
- **Schema**: TPROC-H (TPC-H compatible)
- **Scale**: ~0.01 (very small dataset)
- **Status**: ✅ Active - used for Phase 1 & 2

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

### Database 2: `tpch_small` (Phase 2) ✅
- **Connection**: `postgres://localhost/tpch_small`
- **Schema**: TPROC-H (TPC-H compatible)
- **Scale**: 0.1 (TPC-H standard)
- **Status**: ✅ Active - provides larger dataset diversity

| Table | Row Count | Purpose |
|-------|-----------|---------|
| `lineitem` | 600,572 | Order line items (main fact table) |
| `orders` | 150,000 | Customer orders |
| `partsupp` | 80,000 | Part-supplier relationships |
| `part` | 20,000 | Part catalog |
| `customer` | 15,000 | Customer information |
| `supplier` | 1,000 | Supplier details |
| `nation` | 25 | Nations (standard TPC-H) |
| `region` | 5 | Regions (standard TPC-H) |

### Verification

```bash
# Test tpch (tiny) database
psql postgres://localhost/tpch -c "SELECT COUNT(*) FROM lineitem;"
# Result: 34 rows

# Test tpch_small database
psql postgres://localhost/tpch_small -c "SELECT COUNT(*) FROM lineitem;"
# Result: 600,572 rows

# Test EXPLAIN ANALYZE (used by training data collection)
psql postgres://localhost/tpch_small -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT COUNT(*) FROM orders WHERE o_orderdate > '1998-01-01';"
```

---

## Usage with Neural Model Pipeline

### 1. Data Collection (Phase 2) ✅

```bash
# Collect from tiny database
cargo run --release -p ra-bench --features live-comparison -- \
  collect-training \
  --db postgres://localhost/tpch \
  --configs default \
  --sizes tiny \
  --mode corpus \
  --output training_data.json

# Collect from larger database
cargo run --release -p ra-bench --features live-comparison -- \
  collect-training \
  --db postgres://localhost/tpch_small \
  --configs default \
  --sizes tiny \
  --mode corpus \
  --output training_data_small.json

# Merge datasets
jq -s 'add' training_data.json training_data_small.json > training_data_combined.json
```

**Results**:
- **Samples**: 284 (142 per database)
- **Features**: Correctly extracted (aggregates 0-8, joins 0-7)
- **Execution metrics**: CPU time, memory, I/O, cache hit ratio
- **Collection time**: ~5 minutes total

### 2. Model Training (Phase 2) ✅

```bash
# Train neural model on combined data
cargo run --release --example train_model -p ra-bench -- \
  --input training_data_combined.json \
  --epochs 50 \
  --train-ratio 0.8
```

**Phase 2 Performance** ✅:
- **Training samples**: 284 (combined from both databases)
- **Train/Test split**: 227 train, 57 test
- **Epochs**: 50
- **Initial error**: 6264.9%
- **Final test error**: 131.9% ✅ (exceeds 200-400% target)
- **Train error**: 84.2%
- **Median (p50) error**: 83.9% (excellent!)
- **Feature diversity**: ✅ Working (aggregates: 0-8, joins: 0-7)
- **Improvement from Phase 1**: 85.9% reduction (934.6% → 131.9%)

---

## Scaling Up for Production

### Target: Multiple Database Sizes

For production-quality model (< 10% error), need diverse training data:

| Database | Scale Factor | Approx Size | Actual Row Count (lineitem) | Status |
|----------|--------------|-------------|----------------------------|---------|
| `tpch` (tiny) | 0.01 | ~10 MB | 34 | ✅ Active (Phase 1 & 2) |
| `tpch_small` | 0.1 | ~100 MB | 600,572 | ✅ Active (Phase 2) |
| `tpch_medium` | 1.0 | ~1 GB | ~6M | 🎯 Next (Phase 3) |
| `tpch_large` | 10.0 | ~10 GB | ~60M | 🎯 Future (Phase 4) |

### Why Multiple Sizes Matter

Different data sizes produce different execution patterns:
- **Tiny**: Cache-resident, minimal I/O, sub-millisecond queries
- **Small**: Mixed cache behavior, moderate I/O
- **Medium**: Realistic workload, significant I/O patterns
- **Large**: Memory pressure, complex optimization decisions

### Training Results by Phase

| Phase | Dataset | Samples | Actual CPU Error | Status |
|-------|---------|---------|------------------|--------|
| **Phase 1** | tpch (tiny only) | 142 | 934.6% | ✅ Complete |
| **Phase 2** | tpch + tpch_small | 284 | **131.9%** ✅ | ✅ Complete |
| **Phase 3** | + tpch_medium | 1,000+ | 50-100% (target) | 🎯 Next |
| **Phase 4** | + tpch_large | 10,000+ | <10% (target) | 🎯 Future |

---

## Creating Additional Databases

### Prerequisites: Install dbgen ✅

The TPC-H data generator is needed for larger datasets.

**Status**: ✅ Already compiled and available at `/tmp/tpch-kit/dbgen/dbgen`

```bash
# Verify installation
/tmp/tpch-kit/dbgen/dbgen -h

# To install system-wide (optional):
sudo cp /tmp/tpch-kit/dbgen/dbgen /usr/local/bin/
sudo cp /tmp/tpch-kit/dbgen/dists.dss /usr/local/bin/
```

If dbgen is not installed, follow these steps:
```bash
# Clone TPC-H toolkit
cd /tmp
git clone https://github.com/gregrahn/tpch-kit.git
cd tpch-kit/dbgen

# Compile for macOS + PostgreSQL
make MACHINE=MACOS DATABASE=POSTGRESQL
```

### Create tpch_small (Scale 0.1) ✅ DONE

**Status**: ✅ Already created and active (Phase 2)
**Connection**: `postgres://localhost/tpch_small`
**Lineitem rows**: 600,572

To recreate or create similar database:
```bash
# 1. Generate data
cd /tmp/tpch-kit/dbgen
./dbgen -s 0.1

# 2. Create database and load schema
createdb tpch_small
psql tpch_small < scripts/bench-schema.sql

# 3. Load data (must handle foreign key constraints)
# See Phase 2 Results doc for complete loading script

# 4. Update statistics
psql tpch_small -c "ANALYZE;"

# 5. Verify
psql tpch_small -c "SELECT COUNT(*) FROM lineitem;"
# Result: 600,572 rows ✅
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