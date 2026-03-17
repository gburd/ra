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

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

pub mod cost;
pub mod cpu;
pub mod device;
pub mod gpu;
pub mod memory;
pub mod profile;
pub mod profiles;
pub mod storage;

pub use cost::HardwareCostModel;
pub use cpu::{CacheHierarchy, CpuArchitecture, CpuModel, SimdCapability};
pub use device::Device;
pub use gpu::{GpuArchitecture, GpuMemoryType, GpuModel, GpuVendor, TransferCharacteristics};
pub use memory::{MemoryConfig, MemoryType, NumaTopology};
pub use profile::HardwareProfile;
pub use profiles::CompleteHardwareProfile;
pub use storage::{
    CloudStorageTier, PcieGen, SpindleSpeed, StorageDevice, StorageTechnology,
};
