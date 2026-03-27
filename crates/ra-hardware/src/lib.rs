//! Hardware-aware cost models and operator placement.
//!
//! This crate extends the core cost model with hardware profiles for
//! GPU, FPGA, and SIMD-aware query optimization. It provides:
//!
//! - [`HardwareProfile`] describing available accelerators and their
//!   performance characteristics.
//! - [`Device`] enumeration for operator placement decisions.
//! - [`HardwareCostModel`] that compares CPU vs device execution cost
//!   including data transfer overhead.
//! - Cost estimation functions for GPU scans, joins, aggregations,
//!   and FPGA streaming operators.
//! - Detailed hardware models for CPU, memory, storage, and GPU components.
//! - 20+ predefined hardware profiles for various workloads.
//! - [`benchmark`] module for hardware microbenchmarks (RFC 0068).
//! - [`calibration`] module converting measurements into cost coefficients.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

pub mod benchmark;
pub mod calibration;
pub mod cost;
pub mod cpu;
pub mod detection;
pub mod device;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod network_profiles;
pub mod profile;
pub mod profiles;
pub mod storage;
pub mod system_metrics;

pub use benchmark::{BenchmarkConfig, HardwareMeasurements};
pub use calibration::CalibratedCostModel;
pub use cost::HardwareCostModel;
pub use cpu::{CacheHierarchy, CpuArchitecture, CpuModel, SimdCapability};
pub use detection::detect_hardware;
pub use device::Device;
pub use gpu::{GpuArchitecture, GpuMemoryType, GpuModel, GpuVendor, TransferCharacteristics};
pub use memory::{MemoryConfig, MemoryType, NumaTopology};
pub use network::{
    BroadcastCost, LinkType, Location, NetworkLink, NetworkTopology, NodeId, TopologyError,
};
pub use profile::HardwareProfile;
pub use profiles::CompleteHardwareProfile;
pub use storage::{
    CloudStorageTier, LtoGeneration, NasProtocol, PcieGen, SpindleSpeed, StorageDevice,
    StorageTechnology,
};
pub use system_metrics::{DiskStats, NetworkStats, SystemMetrics};
