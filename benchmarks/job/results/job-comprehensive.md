# JOB Comprehensive Benchmark Report

All 5 dimensions tracked for each of the 113 JOB queries.

Generated: (run `./run_job_comparison.sh imdb` to populate)

## Overview

- **Queries measured**: 0/113
- **Query failures**: 0
- **Correct results**: 0/0
- **PG total exec time**: 0ms
- **PG total plan time**: 0ms

## 1. Planning Efficiency

| Query | PG Plan (ms) | Ra Plan (ms) | Speedup | Rules | E-graph Nodes | Cache |
|-------|-------------|-------------|---------|-------|---------------|-------|

**PG Planning**: Total=0ms, Median=0ms, P95=0ms

## 2. Planning Accuracy (Q-Error)

| Query | Est. Cost | Actual Cost | Q-Error | Est. Rows | Actual Rows |
|-------|-----------|-------------|---------|-----------|-------------|

**Summary**: Median Q-Error=0, P95=0, Max=0, Geometric Mean=0

## 3. Execution Performance

| Query | PG Exec (ms) | Ra Exec (ms) | Speedup | Rows |
|-------|-------------|-------------|---------|------|

**Totals**: PG=0ms, Ra=0ms

## 4. Resource Consumption

| Query | Peak Mem (MB) | CPU Time (ms) | I/O Read (KB) | I/O Write (KB) |
|-------|-------------|-------------|-------------|---------------|

**I/O Totals**: Read=0MB, Write=0MB

## 5. Correctness Verification

| Query | PG Hash | Ra Hash | Match |
|-------|---------|---------|-------|

**Correctness**: 0% (0/0 queries match)

## Dimension Scorecard

| Dimension | Metric | Value | Target | Status |
|-----------|--------|-------|--------|--------|
| Planning Efficiency | Median plan time | N/A | <100ms | BASELINE |
| Planning Accuracy | Median Q-error | N/A | <2.0 | BASELINE |
| Execution Time | Median speedup | N/A | >1.0x | BASELINE |
| Resource Consumption | Total I/O read | N/A | tracked | BASELINE |
| Correctness | Match rate | N/A | 100% | BASELINE |
