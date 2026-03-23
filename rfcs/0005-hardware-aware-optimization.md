# RFC 0005: Hardware-Aware Optimization

- **Status:** Implemented
- **Type:** Retroactive
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

A hardware cost model and optimization rule set that accounts for
CPU architecture, memory hierarchy, storage characteristics, GPU
capabilities, and NUMA topology when selecting query execution
strategies.

## Motivation

Traditional cost models assume uniform hardware. In practice, the
optimal plan depends heavily on hardware characteristics: a hash
join that fits in L3 cache is fast on modern CPUs but catastrophic
on memory-constrained embedded systems. GPU offloading helps large
aggregations but hurts small queries due to data transfer overhead.

## What Was Built

### Hardware Profiles

The `HardwareProfile` type describes a complete system:

- CPU: architecture (x86_64, ARM64, RISC-V), cache hierarchy,
  SIMD width, core count
- Memory: total capacity, bandwidth, NUMA configuration
- Storage: type (NVMe, SSD, HDD, cloud), IOPS, throughput
- GPU: vendor, compute units, memory, transfer bandwidth

20+ predefined profiles cover common deployment targets from
Raspberry Pi to data warehouse servers.

### Hardware Cost Model

`HardwareCostModel` estimates execution cost across CPU, GPU, and
FPGA, including data transfer overhead. For each operator it
computes:

- CPU cost: instruction count scaled by cache miss rates
- I/O cost: page reads scaled by storage characteristics
- Transfer cost: data movement between CPU and accelerators
- Memory cost: peak allocation vs available memory

### Optimization Rules

21 rules in `rules/hardware/` target:

- GPU offloading for large aggregations and hash joins
- FPGA acceleration for pattern matching
- SIMD-aware scan operators
- NUMA-local data placement
- Cache-conscious join algorithms

### Crate

The `ra-hardware` crate (Phase 13) provides all hardware models,
cost estimation, and operator placement logic.

## Key Design Decisions

- Hardware profiles are static configuration, not auto-detected,
  to support planning for remote target systems
- The cost model produces a multi-dimensional cost vector (CPU, I/O,
  memory, transfer) rather than a single scalar, enabling Pareto
  analysis
- GPU offloading rules use a minimum-data-size threshold to avoid
  transfer overhead on small inputs

## Prior Art

- MonetDB's hardware-conscious query processing
- HyPer's morsel-driven parallelism (NUMA-aware)
- Crystal (MIT) for GPU query processing

## References

- `docs/hardware-acceleration.md` -- full documentation
- `crates/ra-hardware/` -- hardware models and cost estimation
- `rules/hardware/` -- 21 hardware-specific rules
