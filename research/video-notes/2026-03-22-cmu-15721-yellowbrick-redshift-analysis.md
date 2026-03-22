# CMU 15-721 Lectures 21-22: Yellowbrick and Amazon Redshift System Analysis

**Source:** CMU 15-721 Spring 2024, Lectures 21-22
**Date:** 2024-04-22, 2024-04-24
**Topic:** Production data warehouse optimizer techniques
**Key Papers:** "Yellowbrick: An Elastic Data Warehouse on Kubernetes" (VLDB 2024),
"Amazon Redshift Re-Invented" (SIGMOD 2022)

## Key Points

These lectures analyze two production data warehouses with distinct optimizer
approaches. Both provide lessons for Ra's optimizer design.

### Yellowbrick

**Architecture:** Disaggregated storage + compute on Kubernetes, NVMe-optimized.

**Optimizer techniques:**

1. **Cost-Based Operator Selection with NVMe Awareness:**
   - Cost model accounts for NVMe SSD characteristics (high random IOPS)
   - Random access penalty much lower than HDD-era cost models assume
   - This changes break-even point for index scan vs sequential scan
   - More queries benefit from index access paths

2. **Workload-Aware Materialization:**
   - Track which intermediate results are reused across queries
   - Automatically create/maintain frequently-needed materializations
   - Optimizer checks materialization cache before generating full plan

3. **Kubernetes-Aware Resource Planning:**
   - Available memory/CPU varies as pods scale up/down
   - Optimizer generates plans with resource annotations
   - Plans degrade gracefully when resources are reduced
   - Important for elastic cloud deployment

4. **Zone Map Optimization:**
   - Extensive use of zone maps (min/max per block) for pruning
   - Zone maps on sorted columns provide strong pruning
   - Optimizer estimates zone map effectiveness based on data ordering

### Amazon Redshift

**Architecture:** Disaggregated storage (Redshift Managed Storage on S3),
distributed MPP execution.

**Optimizer techniques:**

1. **Distribution Key Selection:**
   - Data distributed across nodes by hash of distribution key
   - Optimizer must know distribution to avoid redistributions
   - Co-located joins (same distribution key) avoid network shuffle
   - ALL distribution: small tables replicated to all nodes

   **Optimization rule:** distribution-aware-join-planning - choose join order
   and method based on table distribution keys to minimize shuffles.

2. **Sort Key Selection and Zone Map Interaction:**
   - Sort keys determine physical data ordering
   - Zone maps on sort keys provide optimal pruning (no overlap between blocks)
   - Compound sort keys: useful for prefix predicates
   - Interleaved sort keys: useful for any-column predicates (less efficient)

   **Optimization rule:** sort-key-aware-scan-planning - adjust scan strategy
   based on sort key alignment with query predicates.

3. **Compilation Cache:**
   - Compiled query plans cached by query template + parameter types
   - Cache hit avoids compilation overhead (significant for short queries)
   - Optimizer produces compilation-friendly plans (avoid dynamic dispatch)

4. **Result Caching:**
   - Cache query results for repeated identical queries
   - Automatic invalidation when underlying data changes
   - Optimizer checks result cache before planning

5. **Automatic Table Optimization (ATO):**
   - Automatically choose sort keys and distribution keys
   - Based on workload analysis (most common join keys, filter predicates)
   - Background process re-sorts and re-distributes data

   **Optimization rule:** automatic-physical-design - recommend sort keys and
   distribution keys based on workload analysis.

6. **Concurrency Scaling:**
   - When query queue backs up, automatically add compute clusters
   - Read-only queries can run on scaling clusters with stale data
   - Optimizer must produce plans that work with potentially stale cached data

7. **AQUA (Advanced Query Accelerator):**
   - Hardware-accelerated processing at the storage layer
   - Pushes filtering and aggregation to custom FPGA hardware near storage
   - Optimizer decides what to push to AQUA vs execute on compute nodes

   **Optimization rule:** storage-compute-pushdown-decision - decide which
   operators to push to storage-layer processing (AQUA, Parquet pushdown)
   vs compute-layer execution.

8. **Federated Query Optimization:**
   - Query across Redshift, S3, Aurora, RDS
   - Optimizer generates plans that minimize cross-system data movement
   - Push predicates and aggregations to source systems

### Common Patterns Across Both Systems

1. **Zone map / min-max pruning** - Both systems rely heavily on this
2. **Distribution-aware planning** - Avoid unnecessary data movement
3. **Automatic physical design** - Reduce DBA manual tuning
4. **Result caching** - Short-circuit repeated queries
5. **Storage-compute co-optimization** - Push work to where data lives

## Optimization Rules for Ra

### New Rules Identified

1. **distribution-key-aware-join-ordering** - Order joins to maximize co-located
   joins (tables with same distribution key joined first)
2. **sort-key-zone-map-synergy** - When table is sorted on predicate column,
   estimate zone map pruning effectiveness (potentially skip 90%+ of blocks)
3. **compilation-cache-friendly-plans** - Generate plan templates that maximize
   compilation cache hit rate (avoid unnecessary plan variation)
4. **result-cache-check-insertion** - Before full plan execution, check if
   identical query result exists in cache
5. **automatic-physical-design-advisor** - Based on workload history, recommend
   optimal sort keys, distribution keys, and materializations
6. **storage-compute-pushdown-decision** - Decide which operators to push to
   storage layer based on selectivity and storage layer capabilities
7. **elastic-resource-aware-parallelism** - Adjust plan parallelism based on
   available resources (pods, slots, workers)
8. **nvme-aware-cost-model** - Adjust random I/O cost for NVMe SSDs (much
   lower penalty than HDD)

### Ra Gap Analysis

Ra currently has:
- `rules/physical/distributed/` - Distributed execution rules
- `rules/cost-models/storage-tiering-cost-model.rra` - Storage tier costs
- `rules/hardware/` - Hardware-aware rules
- `crates/ra-advisor/` - Physical design advisor
- No distribution-key-aware optimization
- No result caching rules
- No compilation cache awareness
- No NVMe-specific cost model adjustments

**Missing capabilities:**
- Data distribution metadata (which nodes hold which partitions)
- Distribution-key-aware join ordering
- Sort key alignment with query predicates for zone map effectiveness estimation
- Result cache integration
- Elastic resource-aware plan generation
- NVMe-adjusted I/O cost model

## Relevance to Ra

**Priority:** Medium for most items, High for distribution-aware join ordering
(critical for any distributed execution) and NVMe-aware cost model (affects all
modern hardware).

**Proposed RFCs:**
1. Distribution-Aware Join Ordering - factor data distribution into join order decisions
2. NVMe/SSD Cost Model Adjustment - update I/O cost model for modern storage
3. Automatic Physical Design Advisor Enhancement - add distribution key recommendation
