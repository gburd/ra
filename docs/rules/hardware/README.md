# hardware Rules

Total rules in this category:       21

## Overview

Hardware-specific optimization rules leverage specialized hardware capabilities including GPU acceleration, FPGA processing, and NUMA-aware execution.

## Subcategories

- [accelerator](./accelerator/) -        5 rules
- [data-placement](./data-placement/) -        4 rules
- [fpga](./fpga/) -        4 rules
- [gpu](./gpu/) -        8 rules

## Rules

- [Cache-Conscious Radix Partitioning](./cache-conscious-partitioning.md) - `cache-conscious-partitioning`
- [Heterogeneous Operator Placement](./heterogeneous-operator-placement.md) - `heterogeneous-operator-placement`
- [NUMA-Aware Data Partitioning](./numa-aware-partitioning.md) - `numa-aware-partitioning`
- [Software Prefetch-Aware Hash Join](./prefetch-aware-join.md) - `prefetch-aware-join`
- [SIMD-Vectorized Scan and Filter](./simd-vectorized-scan.md) - `simd-vectorized-scan`
- [Row-to-Columnar Conversion for Device Processing](./columnar-conversion.md) - `columnar-conversion`
- [Device Memory Caching and Reuse](./device-memory-caching.md) - `device-memory-caching`
- [Minimize Host-to-Device Data Transfer](./host-to-device-transfer.md) - `host-to-device-transfer`
- [Unified Memory Management for CPU-GPU](./unified-memory-management.md) - `unified-memory-management`
- [FPGA Near-Storage Decompression Scan](./fpga-compression-scan.md) - `fpga-compression-scan`
- [FPGA Pipelined Hash Join](./fpga-hash-join.md) - `fpga-hash-join`
- [FPGA Hardware Regex Filter](./fpga-regex-filter.md) - `fpga-regex-filter`
- [FPGA Streaming Filter](./fpga-stream-filter.md) - `fpga-stream-filter`
- [GPU Parallel Aggregation](./gpu-aggregation.md) - `gpu-aggregation`
- [GPU Two-Phase Distinct Aggregation](./gpu-distinct-aggregation.md) - `gpu-distinct-aggregation`
- [GPU Hash Join](./gpu-hash-join.md) - `gpu-hash-join`
- [GPU Parallel Table Scan](./gpu-parallel-scan.md) - `gpu-parallel-scan`
- [GPU SIMT Predicate Evaluation](./gpu-predicate-evaluation.md) - `gpu-predicate-evaluation`
- [GPU Parallel Sort](./gpu-sort.md) - `gpu-sort`
- [GPU Accelerated String Operations](./gpu-string-operations.md) - `gpu-string-operations`
- [GPU Parallel Window Function](./gpu-window-function.md) - `gpu-window-function`
