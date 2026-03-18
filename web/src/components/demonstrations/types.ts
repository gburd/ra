/** Cost breakdown for a query plan operator. */
export interface CostBreakdown {
  readonly cpu: number;
  readonly io: number;
  readonly memory: number;
  readonly network: number;
  readonly total: number;
}

/** A simulated query plan node. */
export interface SimPlanNode {
  readonly operator: string;
  readonly cost: CostBreakdown;
  readonly estimatedRows: number;
  readonly properties: Record<string, string>;
  readonly children: readonly SimPlanNode[];
}

/** Staleness levels matching ra-stats. */
export type StalenessLevel =
  | "fresh"
  | "slightly_stale"
  | "moderately_stale"
  | "very_stale";

/** Statistics profile names matching ra-stats profiles. */
export type StatsProfileName =
  | "RealTime"
  | "Standard"
  | "Lazy"
  | "Stale"
  | "Analytical"
  | "Streaming";

/** Statistics configuration for demo. */
export interface StatsConfig {
  readonly profile: StatsProfileName;
  readonly staleness: StalenessLevel;
  readonly rowCount: number;
  readonly modifications: number;
  readonly confidence: number;
}

/** Hardware profile categories for demos. */
export type HardwareCategory =
  | "desktop_budget"
  | "desktop_workstation"
  | "entry_server"
  | "dual_socket_server"
  | "gpu_server_a100"
  | "gpu_server_h100"
  | "data_warehouse"
  | "raspberry_pi"
  | "cloud_vm_small"
  | "cloud_vm_large"
  | "oltp_database"
  | "olap_database";

/** Simplified hardware profile for TS demos. */
export interface HardwareConfig {
  readonly name: string;
  readonly cpuCores: number;
  readonly memoryGb: number;
  readonly storageType: "hdd" | "sata_ssd" | "nvme" | "cloud";
  readonly storageBandwidthGbps: number;
  readonly cpuMemoryBandwidthGbps: number;
  readonly hasGpu: boolean;
  readonly gpuMemoryGb: number;
  readonly gpuSmCount: number;
  readonly gpuMemoryBandwidthGbps: number;
  readonly pcieBandwidthGbps: number;
  readonly simdWidthBits: number;
  readonly numaNodes: number;
  readonly l3CacheMb: number;
}

/** Join algorithm choices. */
export type JoinAlgorithm =
  | "nested_loop"
  | "hash_join"
  | "sort_merge"
  | "index_nested_loop";

/** Aggregation strategy choices. */
export type AggregationStrategy =
  | "hash_agg"
  | "sort_agg"
  | "streaming_agg"
  | "two_phase_agg";

/** Scan method choices. */
export type ScanMethod =
  | "sequential_scan"
  | "index_scan"
  | "bitmap_scan"
  | "index_only_scan";

/** Device placement for GPU offloading. */
export type DevicePlacement = "cpu" | "gpu" | "hybrid";

/** Distributed join strategy. */
export type DistributedJoinStrategy =
  | "broadcast"
  | "shuffle"
  | "colocated";
