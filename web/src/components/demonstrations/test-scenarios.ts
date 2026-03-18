/**
 * Test scenarios for the interactive demonstrations.
 *
 * Each scenario defines inputs, expected outputs, and validation
 * criteria for verifying that the demo components produce correct
 * results under various configurations.
 */

import type {
  AggregationStrategy,
  DevicePlacement,
  DistributedJoinStrategy,
  HardwareCategory,
  JoinAlgorithm,
  ScanMethod,
  StalenessLevel,
} from "src/components/demonstrations/types.ts";

/** A single test scenario. */
export interface TestScenario {
  readonly id: string;
  readonly demo: string;
  readonly name: string;
  readonly description: string;
  readonly inputs: Record<string, unknown>;
  readonly expected: TestExpectation;
}

/** Expected outcome of a test scenario. */
export interface TestExpectation {
  readonly primaryResult: string;
  readonly costImprovement?: { min: number; max: number };
  readonly selectedAlgorithm?: string;
  readonly planContains?: readonly string[];
  readonly planDoesNotContain?: readonly string[];
}

/** Result of running a test scenario. */
export interface TestResult {
  readonly scenario: TestScenario;
  readonly passed: boolean;
  readonly actual: string;
  readonly details: string;
  readonly durationMs: number;
}

// ---- Demo 1: Statistics Staleness ----

const STALENESS_SCENARIOS: readonly TestScenario[] = [
  {
    id: "stale-01",
    demo: "staleness",
    name: "Fresh stats choose hash join",
    description:
      "With fresh statistics on a 1M-row table joined to a 1K-row table, " +
      "the optimizer should choose hash join.",
    inputs: {
      staleness: "fresh" as StalenessLevel,
      leftRows: 1_000_000,
      rightRows: 1_000,
    },
    expected: {
      primaryResult: "hash_join",
      selectedAlgorithm: "hash_join",
    },
  },
  {
    id: "stale-02",
    demo: "staleness",
    name: "Very stale stats may choose nested loop",
    description:
      "With very stale statistics, cardinality estimates are unreliable " +
      "and the optimizer may choose a suboptimal plan.",
    inputs: {
      staleness: "very_stale" as StalenessLevel,
      leftRows: 1_000_000,
      rightRows: 1_000,
    },
    expected: {
      primaryResult: "suboptimal_plan_possible",
    },
  },
  {
    id: "stale-03",
    demo: "staleness",
    name: "Moderately stale with small tables",
    description:
      "Small tables are less sensitive to stale statistics.",
    inputs: {
      staleness: "moderately_stale" as StalenessLevel,
      leftRows: 100,
      rightRows: 50,
    },
    expected: {
      primaryResult: "nested_loop",
      selectedAlgorithm: "nested_loop",
    },
  },
];

// ---- Demo 2: Hardware-Specific Plans ----

const HARDWARE_SCENARIOS: readonly TestScenario[] = [
  {
    id: "hw-01",
    demo: "hardware",
    name: "Desktop budget prefers sort-merge for medium tables",
    description:
      "Limited memory on budget desktop favors sort-merge over hash join " +
      "for medium-sized tables.",
    inputs: {
      hardware: "desktop_budget" as HardwareCategory,
      tableRows: 500_000,
    },
    expected: {
      primaryResult: "plan_adapts_to_hardware",
    },
  },
  {
    id: "hw-02",
    demo: "hardware",
    name: "Data warehouse uses parallel hash join",
    description:
      "Data warehouse with many cores parallelizes hash join.",
    inputs: {
      hardware: "data_warehouse" as HardwareCategory,
      tableRows: 10_000_000,
    },
    expected: {
      primaryResult: "parallel_hash_join",
      planContains: ["hash_join"],
    },
  },
  {
    id: "hw-03",
    demo: "hardware",
    name: "Raspberry Pi avoids hash join on large tables",
    description:
      "Limited RAM on Raspberry Pi prevents building large hash tables.",
    inputs: {
      hardware: "raspberry_pi" as HardwareCategory,
      tableRows: 1_000_000,
    },
    expected: {
      primaryResult: "sort_merge_or_nested_loop",
      planDoesNotContain: ["hash_join"],
    },
  },
];

// ---- Demo 3: Join Algorithm Selection ----

const JOIN_SCENARIOS: readonly TestScenario[] = [
  {
    id: "join-01",
    demo: "join",
    name: "Small left, large right -> hash join",
    description:
      "When the build side fits in memory, hash join is optimal.",
    inputs: {
      leftRows: 1_000,
      rightRows: 1_000_000,
      joinSelectivity: 0.01,
    },
    expected: {
      primaryResult: "hash_join",
      selectedAlgorithm: "hash_join",
    },
  },
  {
    id: "join-02",
    demo: "join",
    name: "Both sorted -> sort-merge join",
    description:
      "Pre-sorted inputs make sort-merge join optimal (no sort needed).",
    inputs: {
      leftRows: 500_000,
      rightRows: 500_000,
      preSorted: true,
    },
    expected: {
      primaryResult: "sort_merge",
      selectedAlgorithm: "sort_merge",
    },
  },
  {
    id: "join-03",
    demo: "join",
    name: "Tiny tables -> nested loop",
    description:
      "For very small tables, nested loop has lowest overhead.",
    inputs: {
      leftRows: 10,
      rightRows: 20,
      joinSelectivity: 0.5,
    },
    expected: {
      primaryResult: "nested_loop",
      selectedAlgorithm: "nested_loop",
    },
  },
  {
    id: "join-04",
    demo: "join",
    name: "Indexed inner -> index nested loop",
    description:
      "With an index on the inner table's join key, INLJ is optimal.",
    inputs: {
      leftRows: 100_000,
      rightRows: 1_000_000,
      innerIndexed: true,
    },
    expected: {
      primaryResult: "index_nested_loop",
      selectedAlgorithm: "index_nested_loop",
    },
  },
];

// ---- Demo 4: Aggregation Strategy ----

const AGGREGATION_SCENARIOS: readonly TestScenario[] = [
  {
    id: "agg-01",
    demo: "aggregation",
    name: "Low cardinality -> hash aggregation",
    description:
      "Few distinct groups fit in a hash table; hash agg is fastest.",
    inputs: {
      inputRows: 1_000_000,
      distinctGroups: 100,
    },
    expected: {
      primaryResult: "hash_agg",
      selectedAlgorithm: "hash_agg",
    },
  },
  {
    id: "agg-02",
    demo: "aggregation",
    name: "Pre-sorted input -> streaming aggregation",
    description:
      "If data arrives sorted by group key, streaming agg avoids " +
      "materializing all groups.",
    inputs: {
      inputRows: 1_000_000,
      distinctGroups: 10_000,
      preSorted: true,
    },
    expected: {
      primaryResult: "streaming_agg",
      selectedAlgorithm: "streaming_agg",
    },
  },
  {
    id: "agg-03",
    demo: "aggregation",
    name: "High cardinality -> sort aggregation",
    description:
      "Many distinct groups exceed hash table capacity; sort first.",
    inputs: {
      inputRows: 10_000_000,
      distinctGroups: 5_000_000,
    },
    expected: {
      primaryResult: "sort_agg",
      selectedAlgorithm: "sort_agg",
    },
  },
];

// ---- Demo 5: Index Selection ----

const INDEX_SCENARIOS: readonly TestScenario[] = [
  {
    id: "idx-01",
    demo: "index",
    name: "High selectivity -> index scan",
    description:
      "Selecting <1% of rows makes index scan clearly better.",
    inputs: {
      tableRows: 1_000_000,
      selectivity: 0.001,
      hasIndex: true,
    },
    expected: {
      primaryResult: "index_scan",
      selectedAlgorithm: "index_scan",
    },
  },
  {
    id: "idx-02",
    demo: "index",
    name: "Low selectivity -> sequential scan",
    description:
      "Selecting >30% of rows makes sequential scan cheaper.",
    inputs: {
      tableRows: 1_000_000,
      selectivity: 0.4,
      hasIndex: true,
    },
    expected: {
      primaryResult: "sequential_scan",
      selectedAlgorithm: "sequential_scan",
    },
  },
  {
    id: "idx-03",
    demo: "index",
    name: "Medium selectivity -> bitmap scan",
    description:
      "5-20% selectivity favors bitmap scan over both index and seq scan.",
    inputs: {
      tableRows: 1_000_000,
      selectivity: 0.1,
      hasIndex: true,
    },
    expected: {
      primaryResult: "bitmap_scan",
      selectedAlgorithm: "bitmap_scan",
    },
  },
  {
    id: "idx-04",
    demo: "index",
    name: "Covering index -> index-only scan",
    description:
      "When the index contains all needed columns, skip the heap.",
    inputs: {
      tableRows: 1_000_000,
      selectivity: 0.01,
      coveringIndex: true,
    },
    expected: {
      primaryResult: "index_only_scan",
      selectedAlgorithm: "index_only_scan",
    },
  },
];

// ---- Demo 6: Subquery Unnesting ----

const SUBQUERY_SCENARIOS: readonly TestScenario[] = [
  {
    id: "sub-01",
    demo: "subquery",
    name: "EXISTS -> semi join",
    description:
      "EXISTS subquery is converted to a semi join.",
    inputs: {
      subqueryType: "exists",
      outerRows: 100_000,
      innerRows: 10_000,
    },
    expected: {
      primaryResult: "semi_join",
      planContains: ["semi"],
    },
  },
  {
    id: "sub-02",
    demo: "subquery",
    name: "NOT EXISTS -> anti join",
    description:
      "NOT EXISTS subquery is converted to an anti join.",
    inputs: {
      subqueryType: "not_exists",
      outerRows: 100_000,
      innerRows: 10_000,
    },
    expected: {
      primaryResult: "anti_join",
      planContains: ["anti"],
    },
  },
  {
    id: "sub-03",
    demo: "subquery",
    name: "Correlated scalar -> lateral join",
    description:
      "Correlated scalar subquery can be decorrelated or use lateral join.",
    inputs: {
      subqueryType: "scalar_correlated",
      outerRows: 1_000,
      innerRows: 100_000,
    },
    expected: {
      primaryResult: "decorrelated_or_lateral",
    },
  },
];

// ---- Demo 7: Parallel Execution ----

const PARALLEL_SCENARIOS: readonly TestScenario[] = [
  {
    id: "par-01",
    demo: "parallel",
    name: "Linear speedup for scan",
    description:
      "Parallel scan on large table should show near-linear speedup.",
    inputs: {
      tableRows: 10_000_000,
      parallelWorkers: 8,
      operation: "scan",
    },
    expected: {
      primaryResult: "speedup_near_linear",
      costImprovement: { min: 0.5, max: 0.9 },
    },
  },
  {
    id: "par-02",
    demo: "parallel",
    name: "Diminishing returns at high parallelism",
    description:
      "32 workers show less than 32x speedup due to coordination overhead.",
    inputs: {
      tableRows: 10_000_000,
      parallelWorkers: 32,
      operation: "hash_join",
    },
    expected: {
      primaryResult: "diminishing_returns",
    },
  },
  {
    id: "par-03",
    demo: "parallel",
    name: "Small table not worth parallelizing",
    description:
      "Parallel overhead exceeds benefit for tiny tables.",
    inputs: {
      tableRows: 100,
      parallelWorkers: 4,
      operation: "scan",
    },
    expected: {
      primaryResult: "serial_faster",
    },
  },
];

// ---- Demo 8: GPU Offloading ----

const GPU_SCENARIOS: readonly TestScenario[] = [
  {
    id: "gpu-01",
    demo: "gpu",
    name: "Large scan -> GPU beneficial",
    description:
      "GPU memory bandwidth advantage makes large scans faster on GPU.",
    inputs: {
      tableRows: 50_000_000,
      operation: "scan",
      gpuMemoryGb: 80,
    },
    expected: {
      primaryResult: "gpu",
      selectedAlgorithm: "gpu",
    },
  },
  {
    id: "gpu-02",
    demo: "gpu",
    name: "Small data -> CPU faster (PCIe overhead)",
    description:
      "PCIe transfer overhead dominates for small data sets.",
    inputs: {
      tableRows: 1_000,
      operation: "scan",
      gpuMemoryGb: 80,
    },
    expected: {
      primaryResult: "cpu",
      selectedAlgorithm: "cpu",
    },
  },
  {
    id: "gpu-03",
    demo: "gpu",
    name: "Data exceeds GPU memory -> hybrid",
    description:
      "When data does not fit in GPU memory, use chunked/hybrid approach.",
    inputs: {
      tableRows: 500_000_000,
      operation: "hash_join",
      gpuMemoryGb: 12,
    },
    expected: {
      primaryResult: "hybrid",
    },
  },
];

// ---- Demo 9: Distributed Joins ----

const DISTRIBUTED_SCENARIOS: readonly TestScenario[] = [
  {
    id: "dist-01",
    demo: "distributed",
    name: "Small dimension -> broadcast",
    description:
      "Small dimension table is broadcast to all nodes.",
    inputs: {
      leftRows: 10_000_000,
      rightRows: 1_000,
      nodes: 4,
    },
    expected: {
      primaryResult: "broadcast",
      selectedAlgorithm: "broadcast",
    },
  },
  {
    id: "dist-02",
    demo: "distributed",
    name: "Large-large -> shuffle",
    description:
      "Two large tables require shuffle (hash partitioned) join.",
    inputs: {
      leftRows: 10_000_000,
      rightRows: 10_000_000,
      nodes: 4,
    },
    expected: {
      primaryResult: "shuffle",
      selectedAlgorithm: "shuffle",
    },
  },
  {
    id: "dist-03",
    demo: "distributed",
    name: "Co-partitioned -> colocated",
    description:
      "Tables already partitioned on join key use colocated join.",
    inputs: {
      leftRows: 10_000_000,
      rightRows: 10_000_000,
      nodes: 4,
      coPartitioned: true,
    },
    expected: {
      primaryResult: "colocated",
      selectedAlgorithm: "colocated",
    },
  },
];

// ---- Demo 10: Cost Model Calibration ----

const COST_MODEL_SCENARIOS: readonly TestScenario[] = [
  {
    id: "cost-01",
    demo: "costmodel",
    name: "Default weights produce balanced costs",
    description:
      "With default CPU/IO/memory weights, costs are balanced.",
    inputs: {
      cpuWeight: 1.0,
      ioWeight: 1.0,
      memoryWeight: 1.0,
    },
    expected: {
      primaryResult: "balanced_cost",
    },
  },
  {
    id: "cost-02",
    demo: "costmodel",
    name: "High IO weight favors index scan",
    description:
      "Increasing IO cost weight makes sequential scans more expensive, " +
      "favoring index access.",
    inputs: {
      cpuWeight: 1.0,
      ioWeight: 5.0,
      memoryWeight: 1.0,
    },
    expected: {
      primaryResult: "io_sensitive_plan",
      planContains: ["index"],
    },
  },
  {
    id: "cost-03",
    demo: "costmodel",
    name: "High memory weight favors streaming",
    description:
      "Increasing memory cost weight favors streaming over hash-based plans.",
    inputs: {
      cpuWeight: 1.0,
      ioWeight: 1.0,
      memoryWeight: 5.0,
    },
    expected: {
      primaryResult: "memory_sensitive_plan",
    },
  },
];

// ---- Combined scenario list ----

export const ALL_SCENARIOS: readonly TestScenario[] = [
  ...STALENESS_SCENARIOS,
  ...HARDWARE_SCENARIOS,
  ...JOIN_SCENARIOS,
  ...AGGREGATION_SCENARIOS,
  ...INDEX_SCENARIOS,
  ...SUBQUERY_SCENARIOS,
  ...PARALLEL_SCENARIOS,
  ...GPU_SCENARIOS,
  ...DISTRIBUTED_SCENARIOS,
  ...COST_MODEL_SCENARIOS,
];

/** Get scenarios for a specific demo. */
export function getScenariosForDemo(
  demoId: string,
): readonly TestScenario[] {
  return ALL_SCENARIOS.filter((s) => s.demo === demoId);
}

/** Get scenario by ID. */
export function getScenarioById(
  id: string,
): TestScenario | undefined {
  return ALL_SCENARIOS.find((s) => s.id === id);
}

/** Summary statistics. */
export function getScenarioSummary(): {
  total: number;
  byDemo: Record<string, number>;
} {
  const byDemo: Record<string, number> = {};
  for (const s of ALL_SCENARIOS) {
    byDemo[s.demo] = (byDemo[s.demo] ?? 0) + 1;
  }
  return { total: ALL_SCENARIOS.length, byDemo };
}
