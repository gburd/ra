/**
 * Client-side query optimization simulation.
 *
 * Mirrors the cost model logic from ra-hardware and ra-stats crates
 * to produce realistic query plan decisions in the browser without
 * requiring WASM or a backend API.
 */

import type {
  AggregationStrategy,
  CostBreakdown,
  DevicePlacement,
  DistributedJoinStrategy,
  HardwareCategory,
  HardwareConfig,
  JoinAlgorithm,
  ScanMethod,
  SimPlanNode,
  StalenessLevel,
} from "src/components/demonstrations/types.ts";

// ---- Hardware profiles (mirrors ra-hardware/profiles.rs) ----

const HARDWARE_PROFILES: Record<HardwareCategory, HardwareConfig> = {
  desktop_budget: {
    name: "Desktop Budget (i7, 16GB, NVMe)",
    cpuCores: 12,
    memoryGb: 16,
    storageType: "nvme",
    storageBandwidthGbps: 3.5,
    cpuMemoryBandwidthGbps: 76.8,
    hasGpu: false,
    gpuMemoryGb: 0,
    gpuSmCount: 0,
    gpuMemoryBandwidthGbps: 0,
    pcieBandwidthGbps: 0,
    simdWidthBits: 256,
    numaNodes: 1,
    l3CacheMb: 25,
  },
  desktop_workstation: {
    name: "Desktop Workstation (i9, 64GB, NVMe, RTX 4070)",
    cpuCores: 24,
    memoryGb: 64,
    storageType: "nvme",
    storageBandwidthGbps: 7.0,
    cpuMemoryBandwidthGbps: 89.6,
    hasGpu: true,
    gpuMemoryGb: 12,
    gpuSmCount: 46,
    gpuMemoryBandwidthGbps: 504,
    pcieBandwidthGbps: 25,
    simdWidthBits: 256,
    numaNodes: 1,
    l3CacheMb: 36,
  },
  entry_server: {
    name: "Entry Server (Xeon, 128GB, SATA SSD)",
    cpuCores: 40,
    memoryGb: 128,
    storageType: "sata_ssd",
    storageBandwidthGbps: 0.55,
    cpuMemoryBandwidthGbps: 51.2,
    hasGpu: false,
    gpuMemoryGb: 0,
    gpuSmCount: 0,
    gpuMemoryBandwidthGbps: 0,
    pcieBandwidthGbps: 0,
    simdWidthBits: 512,
    numaNodes: 1,
    l3CacheMb: 60,
  },
  dual_socket_server: {
    name: "Dual-Socket Server (2x EPYC, 512GB, NVMe)",
    cpuCores: 128,
    memoryGb: 512,
    storageType: "nvme",
    storageBandwidthGbps: 7.0,
    cpuMemoryBandwidthGbps: 204.8,
    hasGpu: false,
    gpuMemoryGb: 0,
    gpuSmCount: 0,
    gpuMemoryBandwidthGbps: 0,
    pcieBandwidthGbps: 0,
    simdWidthBits: 256,
    numaNodes: 2,
    l3CacheMb: 512,
  },
  gpu_server_a100: {
    name: "GPU Server (A100 80GB)",
    cpuCores: 64,
    memoryGb: 512,
    storageType: "nvme",
    storageBandwidthGbps: 7.0,
    cpuMemoryBandwidthGbps: 50,
    hasGpu: true,
    gpuMemoryGb: 80,
    gpuSmCount: 108,
    gpuMemoryBandwidthGbps: 2039,
    pcieBandwidthGbps: 25,
    simdWidthBits: 512,
    numaNodes: 2,
    l3CacheMb: 64,
  },
  gpu_server_h100: {
    name: "GPU Server (H100 80GB)",
    cpuCores: 64,
    memoryGb: 1024,
    storageType: "nvme",
    storageBandwidthGbps: 7.0,
    cpuMemoryBandwidthGbps: 50,
    hasGpu: true,
    gpuMemoryGb: 80,
    gpuSmCount: 132,
    gpuMemoryBandwidthGbps: 3350,
    pcieBandwidthGbps: 64,
    simdWidthBits: 512,
    numaNodes: 2,
    l3CacheMb: 64,
  },
  data_warehouse: {
    name: "Data Warehouse (4x EPYC, 2TB, NVMe Array)",
    cpuCores: 256,
    memoryGb: 2048,
    storageType: "nvme",
    storageBandwidthGbps: 28.0,
    cpuMemoryBandwidthGbps: 409.6,
    hasGpu: false,
    gpuMemoryGb: 0,
    gpuSmCount: 0,
    gpuMemoryBandwidthGbps: 0,
    pcieBandwidthGbps: 0,
    simdWidthBits: 256,
    numaNodes: 4,
    l3CacheMb: 1024,
  },
  raspberry_pi: {
    name: "Raspberry Pi 4 (4GB)",
    cpuCores: 4,
    memoryGb: 4,
    storageType: "sata_ssd",
    storageBandwidthGbps: 0.04,
    cpuMemoryBandwidthGbps: 4.0,
    hasGpu: false,
    gpuMemoryGb: 0,
    gpuSmCount: 0,
    gpuMemoryBandwidthGbps: 0,
    pcieBandwidthGbps: 0,
    simdWidthBits: 128,
    numaNodes: 1,
    l3CacheMb: 1,
  },
  cloud_vm_small: {
    name: "Cloud VM Small (m5.2xlarge, 32GB)",
    cpuCores: 8,
    memoryGb: 32,
    storageType: "cloud",
    storageBandwidthGbps: 0.6,
    cpuMemoryBandwidthGbps: 25.6,
    hasGpu: false,
    gpuMemoryGb: 0,
    gpuSmCount: 0,
    gpuMemoryBandwidthGbps: 0,
    pcieBandwidthGbps: 0,
    simdWidthBits: 256,
    numaNodes: 1,
    l3CacheMb: 25,
  },
  cloud_vm_large: {
    name: "Cloud VM Large (m5.16xlarge, 256GB)",
    cpuCores: 64,
    memoryGb: 256,
    storageType: "cloud",
    storageBandwidthGbps: 2.4,
    cpuMemoryBandwidthGbps: 102.4,
    hasGpu: false,
    gpuMemoryGb: 0,
    gpuSmCount: 0,
    gpuMemoryBandwidthGbps: 0,
    pcieBandwidthGbps: 0,
    simdWidthBits: 256,
    numaNodes: 2,
    l3CacheMb: 50,
  },
  oltp_database: {
    name: "OLTP Database (Xeon, 512GB, Optane)",
    cpuCores: 40,
    memoryGb: 512,
    storageType: "nvme",
    storageBandwidthGbps: 4.8,
    cpuMemoryBandwidthGbps: 102.4,
    hasGpu: false,
    gpuMemoryGb: 0,
    gpuSmCount: 0,
    gpuMemoryBandwidthGbps: 0,
    pcieBandwidthGbps: 0,
    simdWidthBits: 512,
    numaNodes: 2,
    l3CacheMb: 60,
  },
  olap_database: {
    name: "OLAP Database (4x EPYC, 2TB, A100)",
    cpuCores: 256,
    memoryGb: 2048,
    storageType: "nvme",
    storageBandwidthGbps: 28.0,
    cpuMemoryBandwidthGbps: 409.6,
    hasGpu: true,
    gpuMemoryGb: 80,
    gpuSmCount: 108,
    gpuMemoryBandwidthGbps: 2039,
    pcieBandwidthGbps: 25,
    simdWidthBits: 256,
    numaNodes: 4,
    l3CacheMb: 1024,
  },
};

export function getHardwareProfile(
  category: HardwareCategory,
): HardwareConfig {
  return HARDWARE_PROFILES[category];
}

export function getAllHardwareCategories(): HardwareCategory[] {
  return Object.keys(HARDWARE_PROFILES) as HardwareCategory[];
}

// ---- Cost calculations (mirrors ra-hardware/cost.rs) ----

function makeCost(
  cpu: number,
  io: number,
  memory: number,
  network: number,
): CostBreakdown {
  return {
    cpu,
    io,
    memory,
    network,
    total: cpu + io + memory + network,
  };
}

/** Staleness multiplier: stale stats inflate cardinality estimates. */
export function stalenessMultiplier(staleness: StalenessLevel): number {
  switch (staleness) {
    case "fresh":
      return 1.0;
    case "slightly_stale":
      return 1.3;
    case "moderately_stale":
      return 2.5;
    case "very_stale":
      return 10.0;
  }
}

/** Confidence from staleness for display. */
export function stalenessConfidence(staleness: StalenessLevel): number {
  switch (staleness) {
    case "fresh":
      return 0.95;
    case "slightly_stale":
      return 0.8;
    case "moderately_stale":
      return 0.5;
    case "very_stale":
      return 0.2;
  }
}

// ---- Scan cost ----

export function scanCost(
  hw: HardwareConfig,
  rowCount: number,
  avgRowSize: number,
): CostBreakdown {
  const dataBytes = rowCount * avgRowSize;
  const bwGbps = hw.storageBandwidthGbps;
  const ioTime = dataBytes / (bwGbps * 1e9);
  const cpuTime = dataBytes / (hw.cpuMemoryBandwidthGbps * 1e9);
  return makeCost(cpuTime, ioTime, 0, 0);
}

// ---- Index scan cost ----

export function indexScanCost(
  hw: HardwareConfig,
  totalRows: number,
  selectedRows: number,
  avgRowSize: number,
): CostBreakdown {
  const indexLookups = selectedRows;
  const randomIoLatency =
    hw.storageType === "nvme"
      ? 0.00001
      : hw.storageType === "sata_ssd"
        ? 0.00005
        : hw.storageType === "hdd"
          ? 0.008
          : 0.001;
  const ioTime = indexLookups * randomIoLatency;
  const cpuTime =
    indexLookups * 100e-9 + Math.log2(totalRows + 1) * 50e-9;
  const memBytes = selectedRows * avgRowSize;
  return makeCost(cpuTime, ioTime, memBytes / 1e9, 0);
}

// ---- Join costs ----

export function nestedLoopJoinCost(
  hw: HardwareConfig,
  outerRows: number,
  innerRows: number,
  avgRowSize: number,
): CostBreakdown {
  const cpuTime = outerRows * innerRows * 50e-9;
  const dataBytes = (outerRows + innerRows) * avgRowSize;
  const ioTime = dataBytes / (hw.storageBandwidthGbps * 1e9);
  return makeCost(cpuTime, ioTime, 0, 0);
}

export function hashJoinCost(
  hw: HardwareConfig,
  buildRows: number,
  probeRows: number,
  avgRowSize: number,
): CostBreakdown {
  const buildCpu = buildRows * 100e-9;
  const probeCpu = probeRows * 50e-9;
  const htMemory = buildRows * avgRowSize * 2;
  const dataBytes = (buildRows + probeRows) * avgRowSize;
  const ioTime = dataBytes / (hw.storageBandwidthGbps * 1e9);
  return makeCost(buildCpu + probeCpu, ioTime, htMemory / 1e9, 0);
}

export function sortMergeJoinCost(
  hw: HardwareConfig,
  leftRows: number,
  rightRows: number,
  avgRowSize: number,
): CostBreakdown {
  const leftSort =
    leftRows > 1
      ? leftRows * Math.log2(leftRows) * 200e-9
      : leftRows * 200e-9;
  const rightSort =
    rightRows > 1
      ? rightRows * Math.log2(rightRows) * 200e-9
      : rightRows * 200e-9;
  const mergeCpu = (leftRows + rightRows) * 30e-9;
  const dataBytes = (leftRows + rightRows) * avgRowSize;
  const ioTime = dataBytes / (hw.storageBandwidthGbps * 1e9);
  const sortMem = (leftRows + rightRows) * avgRowSize;
  return makeCost(
    leftSort + rightSort + mergeCpu,
    ioTime,
    sortMem / 1e9,
    0,
  );
}

// ---- Choose best join ----

export function chooseBestJoin(
  hw: HardwareConfig,
  leftRows: number,
  rightRows: number,
  avgRowSize: number,
  availableMemoryBytes: number,
  hasIndex: boolean,
): JoinAlgorithm {
  const smaller = Math.min(leftRows, rightRows);
  const larger = Math.max(leftRows, rightRows);
  const htSize = smaller * avgRowSize * 2;

  if (hasIndex && smaller < 1000) {
    return "index_nested_loop";
  }

  if (smaller < 100 && larger < 10000) {
    return "nested_loop";
  }

  if (htSize <= availableMemoryBytes) {
    return "hash_join";
  }

  return "sort_merge";
}

// ---- Aggregation strategies ----

export function hashAggCost(
  hw: HardwareConfig,
  inputRows: number,
  groupCount: number,
  avgRowSize: number,
): CostBreakdown {
  const cpuTime = inputRows * 80e-9;
  const htMem = groupCount * 64;
  const dataBytes = inputRows * avgRowSize;
  const ioTime = dataBytes / (hw.storageBandwidthGbps * 1e9);
  return makeCost(cpuTime, ioTime, htMem / 1e9, 0);
}

export function sortAggCost(
  hw: HardwareConfig,
  inputRows: number,
  avgRowSize: number,
): CostBreakdown {
  const sortTime =
    inputRows > 1
      ? inputRows * Math.log2(inputRows) * 200e-9
      : inputRows * 200e-9;
  const scanTime = inputRows * 30e-9;
  const dataBytes = inputRows * avgRowSize;
  const ioTime = dataBytes / (hw.storageBandwidthGbps * 1e9);
  const sortMem = inputRows * avgRowSize;
  return makeCost(sortTime + scanTime, ioTime, sortMem / 1e9, 0);
}

export function chooseBestAggregation(
  hw: HardwareConfig,
  inputRows: number,
  groupCount: number,
  avgRowSize: number,
  availableMemoryBytes: number,
): AggregationStrategy {
  const htSize = groupCount * 64;

  if (groupCount <= 1) {
    return "streaming_agg";
  }

  if (
    groupCount > 1000000 &&
    hw.cpuCores >= 16 &&
    htSize > availableMemoryBytes
  ) {
    return "two_phase_agg";
  }

  if (htSize <= availableMemoryBytes && groupCount < inputRows * 0.5) {
    return "hash_agg";
  }

  return "sort_agg";
}

// ---- Scan method selection ----

export function chooseScanMethod(
  hw: HardwareConfig,
  totalRows: number,
  selectivity: number,
  hasIndex: boolean,
  hasMultipleIndexes: boolean,
): ScanMethod {
  const selectedRows = totalRows * selectivity;

  if (!hasIndex) {
    return "sequential_scan";
  }

  if (selectivity < 0.01 && hasIndex) {
    return "index_only_scan";
  }

  if (selectivity < 0.05 && hasIndex) {
    return "index_scan";
  }

  if (
    selectivity < 0.2 &&
    hasMultipleIndexes &&
    totalRows > 100000
  ) {
    return "bitmap_scan";
  }

  if (selectivity >= 0.2) {
    return "sequential_scan";
  }

  return "index_scan";
}

// ---- GPU offloading decision ----

export function gpuScanCost(
  hw: HardwareConfig,
  rowCount: number,
  avgRowSize: number,
): CostBreakdown {
  if (!hw.hasGpu) {
    return makeCost(Infinity, 0, 0, 0);
  }
  const dataBytes = rowCount * avgRowSize;
  const transferTime = dataBytes / (hw.pcieBandwidthGbps * 1e9);
  const gpuCompute =
    dataBytes / (hw.gpuMemoryBandwidthGbps * 1e9);
  return makeCost(gpuCompute, 0, 0, transferTime);
}

export function gpuHashJoinCost(
  hw: HardwareConfig,
  buildRows: number,
  probeRows: number,
  avgRowSize: number,
): CostBreakdown {
  if (!hw.hasGpu) {
    return makeCost(Infinity, 0, 0, 0);
  }
  const totalBytes = (buildRows + probeRows) * avgRowSize;
  const transferTime = totalBytes / (hw.pcieBandwidthGbps * 1e9);
  const sm = hw.gpuSmCount;
  const gpuBuild = (buildRows * 100e-9) / sm;
  const gpuProbe = (probeRows * 50e-9) / sm;
  return makeCost(gpuBuild + gpuProbe, 0, 0, transferTime);
}

export function gpuAggregationCost(
  hw: HardwareConfig,
  inputRows: number,
  groupCount: number,
  avgRowSize: number,
): CostBreakdown {
  if (!hw.hasGpu) {
    return makeCost(Infinity, 0, 0, 0);
  }
  const dataBytes = inputRows * avgRowSize;
  const transferTime = dataBytes / (hw.pcieBandwidthGbps * 1e9);
  const sm = hw.gpuSmCount;
  const gpuTime = (inputRows * 80e-9) / sm + groupCount * 100e-9;
  return makeCost(gpuTime, 0, 0, transferTime);
}

export function chooseDevicePlacement(
  hw: HardwareConfig,
  cpuCost: CostBreakdown,
  gpuCost: CostBreakdown,
): DevicePlacement {
  if (!hw.hasGpu) {
    return "cpu";
  }
  if (gpuCost.total < cpuCost.total * 0.7) {
    return "gpu";
  }
  if (
    gpuCost.total < cpuCost.total * 1.1 &&
    gpuCost.total > cpuCost.total * 0.7
  ) {
    return "hybrid";
  }
  return "cpu";
}

// ---- Distributed query planning ----

export function chooseDistributedJoinStrategy(
  leftRows: number,
  rightRows: number,
  clusterNodes: number,
  isColocated: boolean,
): DistributedJoinStrategy {
  if (isColocated) {
    return "colocated";
  }

  const smaller = Math.min(leftRows, rightRows);
  const broadcastThreshold = 100000 * clusterNodes;

  if (smaller < broadcastThreshold) {
    return "broadcast";
  }

  return "shuffle";
}

export function distributedJoinCost(
  hw: HardwareConfig,
  leftRows: number,
  rightRows: number,
  avgRowSize: number,
  clusterNodes: number,
  strategy: DistributedJoinStrategy,
): CostBreakdown {
  const networkBandwidthGbps = 1.25; // 10 Gbps link
  const localLeft = leftRows / clusterNodes;
  const localRight = rightRows / clusterNodes;

  switch (strategy) {
    case "colocated": {
      const localCost = hashJoinCost(
        hw,
        localLeft,
        localRight,
        avgRowSize,
      );
      return localCost;
    }
    case "broadcast": {
      const smaller = Math.min(leftRows, rightRows);
      const larger = Math.max(leftRows, rightRows);
      const transferBytes =
        smaller * avgRowSize * (clusterNodes - 1);
      const networkTime =
        transferBytes / (networkBandwidthGbps * 1e9);
      const localCost = hashJoinCost(
        hw,
        smaller,
        larger / clusterNodes,
        avgRowSize,
      );
      return makeCost(
        localCost.cpu,
        localCost.io,
        localCost.memory,
        networkTime,
      );
    }
    case "shuffle": {
      const transferBytes =
        (leftRows + rightRows) *
        avgRowSize *
        ((clusterNodes - 1) / clusterNodes);
      const networkTime =
        transferBytes / (networkBandwidthGbps * 1e9);
      const localCost = hashJoinCost(
        hw,
        localLeft,
        localRight,
        avgRowSize,
      );
      return makeCost(
        localCost.cpu,
        localCost.io,
        localCost.memory,
        networkTime,
      );
    }
  }
}

// ---- Parallel execution cost ----

export function parallelScanCost(
  hw: HardwareConfig,
  rowCount: number,
  avgRowSize: number,
  parallelism: number,
): CostBreakdown {
  const seq = scanCost(hw, rowCount, avgRowSize);
  const overhead = 1.0 + 0.05 * (parallelism - 1); // coordination
  return makeCost(
    (seq.cpu * overhead) / parallelism,
    seq.io / parallelism,
    0,
    0,
  );
}

export function parallelHashJoinCost(
  hw: HardwareConfig,
  buildRows: number,
  probeRows: number,
  avgRowSize: number,
  parallelism: number,
): CostBreakdown {
  const seq = hashJoinCost(hw, buildRows, probeRows, avgRowSize);
  const overhead = 1.0 + 0.08 * (parallelism - 1);
  return makeCost(
    (seq.cpu * overhead) / parallelism,
    seq.io / parallelism,
    seq.memory,
    0,
  );
}

// ---- Plan building helpers ----

export function buildStalenessComparisonPlan(
  hw: HardwareConfig,
  staleness: StalenessLevel,
  trueLeftRows: number,
  trueRightRows: number,
): SimPlanNode {
  const mult = stalenessMultiplier(staleness);
  const estimatedLeft = Math.round(trueLeftRows * mult);
  const estimatedRight = Math.round(trueRightRows * mult);
  const avgRowSize = 100;

  const joinAlg = chooseBestJoin(
    hw,
    estimatedLeft,
    estimatedRight,
    avgRowSize,
    hw.memoryGb * 1e9 * 0.5,
    false,
  );

  let joinCost: CostBreakdown;
  switch (joinAlg) {
    case "hash_join":
      joinCost = hashJoinCost(
        hw,
        Math.min(estimatedLeft, estimatedRight),
        Math.max(estimatedLeft, estimatedRight),
        avgRowSize,
      );
      break;
    case "nested_loop":
      joinCost = nestedLoopJoinCost(
        hw,
        estimatedLeft,
        estimatedRight,
        avgRowSize,
      );
      break;
    case "sort_merge":
      joinCost = sortMergeJoinCost(
        hw,
        estimatedLeft,
        estimatedRight,
        avgRowSize,
      );
      break;
    case "index_nested_loop":
      joinCost = nestedLoopJoinCost(
        hw,
        estimatedLeft,
        estimatedRight,
        avgRowSize,
      );
      break;
  }

  const leftScan = scanCost(hw, estimatedLeft, avgRowSize);
  const rightScan = scanCost(hw, estimatedRight, avgRowSize);

  return {
    operator: `${formatJoinAlg(joinAlg)}`,
    cost: joinCost,
    estimatedRows: Math.round(
      (estimatedLeft * estimatedRight) / Math.max(estimatedLeft, estimatedRight),
    ),
    properties: {
      algorithm: joinAlg,
      staleness,
      confidence: `${(stalenessConfidence(staleness) * 100).toFixed(0)}%`,
    },
    children: [
      {
        operator: "Seq Scan (orders)",
        cost: leftScan,
        estimatedRows: estimatedLeft,
        properties: { table: "orders" },
        children: [],
      },
      {
        operator: "Seq Scan (customers)",
        cost: rightScan,
        estimatedRows: estimatedRight,
        properties: { table: "customers" },
        children: [],
      },
    ],
  };
}

function formatJoinAlg(alg: JoinAlgorithm): string {
  switch (alg) {
    case "nested_loop":
      return "Nested Loop Join";
    case "hash_join":
      return "Hash Join";
    case "sort_merge":
      return "Sort-Merge Join";
    case "index_nested_loop":
      return "Index Nested Loop Join";
  }
}

// ---- Formatting helpers ----

export function formatCost(cost: number): string {
  if (cost === Infinity) return "N/A";
  if (cost >= 1) return `${cost.toFixed(2)}s`;
  if (cost >= 0.001) return `${(cost * 1000).toFixed(2)}ms`;
  if (cost >= 0.000001) return `${(cost * 1e6).toFixed(1)}us`;
  return `${(cost * 1e9).toFixed(0)}ns`;
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`;
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(1)} KB`;
  return `${bytes} B`;
}

export function formatRows(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`;
  return String(Math.round(n));
}
