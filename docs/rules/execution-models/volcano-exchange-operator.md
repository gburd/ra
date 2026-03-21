# Rule: Volcano Iterator Model - Exchange Operator

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-exchange-operator.rra`

## Metadata

- **ID:** `volcano-exchange-operator`
- **Version:** 1.0.0
- **Databases:** postgresql, oracle, mssql, duckdb
- **Tags:** execution, iterator, volcano, exchange, parallelism, partitioning
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Exchange Operator

## Description

The Exchange operator is the Volcano model's mechanism for introducing
parallelism into an otherwise single-threaded iterator tree. Exchange
encapsulates all parallelism concerns (partitioning, buffering, thread
synchronization) into a single operator, allowing all other operators
to remain single-threaded and unaware of parallel execution.

**When to apply:** When a query plan can benefit from intra-query
parallelism -- typically for large scans, hash joins on big tables,
and parallel aggregation. The exchange operator is inserted at
parallelism boundaries in the plan tree.

**Why it works:** By isolating parallelism in one operator, the
Volcano model avoids the complexity of making every operator
thread-safe. Each parallel worker runs a complete sub-tree of
single-threaded iterators. The exchange operator handles data
redistribution between producer and consumer threads using queues.

**Exchange variants:**
- **Gather (N:1)**: Multiple producers, single consumer. Used at
  the top of parallel regions to collect results.
- **Scatter/Repartition (N:M)**: Redistribute data by hash, range,
  or round-robin. Used between parallel pipeline stages that need
  different partitioning (e.g., hash join → hash aggregate on
  different keys).
- **Broadcast (1:N)**: One producer sends to all consumers. Used
  for small dimension tables in parallel joins.

## Relational Algebra

```
Exchange operator types:

Gather(N → 1):
  Producer_1 ──┐
  Producer_2 ──┼──→ Consumer
  Producer_N ──┘
  Uses: merge queue, round-robin, or ordered merge

Scatter(N → M, partition_fn):
  Producer_i → partition_fn(tuple) → Queue_j → Consumer_j
  partition_fn: hash(key) mod M, range, round-robin

Broadcast(1 → N):
  Producer → copy to all → Consumer_1, ..., Consumer_N

Parallel query plan structure:

  -- Sequential plan:
  Sort(Agg(Join(Scan(A), Scan(B))))

  -- Parallel plan with exchanges:
  Gather(
    Sort(
      Exchange_repartition(hash(group_key),
        Agg_partial(
          Exchange_repartition(hash(join_key),
            Join(
              Scan_parallel(A),  -- each worker scans a portion
              Broadcast(Scan(B)) -- small table broadcast
            )
          )
        )
      )
    )
  )

Parallelism degree selection:
  DOP = min(
    available_cores,
    ceil(table_pages / min_pages_per_worker),
    max_parallel_workers_per_query
  )
```

## Implementation

```rust
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

/// Partitioning strategy for exchange operator.
#[derive(Debug, Clone)]
pub enum PartitionStrategy {
    /// Hash partitioning on specified columns.
    Hash { columns: Vec<usize>, num_partitions: usize },
    /// Round-robin distribution.
    RoundRobin { num_partitions: usize },
    /// Range partitioning with boundary values.
    Range { column: usize, boundaries: Vec<Value> },
    /// Broadcast to all consumers.
    Broadcast { num_consumers: usize },
    /// Gather from all producers to single consumer.
    Gather,
}

/// A bounded, thread-safe queue for tuple exchange.
pub struct ExchangeQueue {
    sender: mpsc::SyncSender<Option<Tuple>>,
    receiver: mpsc::Receiver<Option<Tuple>>,
}

impl ExchangeQueue {
    pub fn new(buffer_size: usize) -> (
        mpsc::SyncSender<Option<Tuple>>,
        mpsc::Receiver<Option<Tuple>>,
    ) {
        mpsc::sync_channel(buffer_size)
    }
}

/// Gather exchange: collects results from N parallel workers
/// into a single output stream.
pub struct GatherExchange {
    /// Plan fragment each worker executes.
    child_plan: Arc<RelExpr>,
    /// Degree of parallelism.
    num_workers: usize,
    /// Receivers for worker output.
    receivers: Vec<mpsc::Receiver<Option<Tuple>>>,
    /// Worker thread handles.
    workers: Vec<thread::JoinHandle<Result<()>>>,
    /// Current receiver index (round-robin).
    current_receiver: usize,
    /// Number of exhausted workers.
    exhausted_count: usize,
}

impl VolcanoIterator for GatherExchange {
    fn open(&mut self) -> Result<()> {
        // Spawn N worker threads, each executing the
        // child plan independently
        for worker_id in 0..self.num_workers {
            let plan = Arc::clone(&self.child_plan);
            let (sender, receiver) =
                ExchangeQueue::new(1024);
            self.receivers.push(receiver);

            let handle = thread::spawn(move || {
                let mut iter =
                    build_iterator_tree_parallel(
                        &plan, worker_id,
                    );
                iter.open()?;
                loop {
                    match iter.next_tuple()? {
                        Some(tuple) => {
                            if sender
                                .send(Some(tuple))
                                .is_err()
                            {
                                break; // Consumer gone
                            }
                        }
                        None => {
                            let _ = sender.send(None);
                            break;
                        }
                    }
                }
                iter.close()?;
                Ok(())
            });
            self.workers.push(handle);
        }
        Ok(())
    }

    fn next_tuple(&mut self) -> Result<Option<Tuple>> {
        // Round-robin across workers, skip exhausted ones
        loop {
            if self.exhausted_count >= self.num_workers {
                return Ok(None);
            }

            let idx = self.current_receiver
                % self.receivers.len();
            self.current_receiver += 1;

            match self.receivers[idx].recv() {
                Ok(Some(tuple)) => return Ok(Some(tuple)),
                Ok(None) => {
                    self.exhausted_count += 1;
                    continue;
                }
                Err(_) => {
                    self.exhausted_count += 1;
                    continue;
                }
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        // Drop receivers to signal workers to stop
        self.receivers.clear();
        // Wait for all workers to finish
        for handle in self.workers.drain(..) {
            handle.join().map_err(|_| {
                anyhow::anyhow!("worker thread panicked")
            })??;
        }
        Ok(())
    }

    fn schema(&self) -> &Schema {
        // Same schema as child plan
        &self.child_schema
    }

    fn estimated_cardinality(&self) -> f64 {
        self.child_cardinality
    }
}

/// Hash-partitioning exchange: repartitions data from N
/// producers to M consumers based on hash of key columns.
pub struct RepartitionExchange {
    child_plan: Arc<RelExpr>,
    hash_columns: Vec<usize>,
    num_producers: usize,
    num_consumers: usize,
    /// One sender per (producer, consumer) pair.
    senders: Vec<Vec<mpsc::SyncSender<Option<Tuple>>>>,
    /// One receiver per consumer, merged from all producers.
    consumer_receivers: Vec<mpsc::Receiver<Option<Tuple>>>,
}

impl RepartitionExchange {
    /// Compute target partition for a tuple.
    fn partition_for(
        &self,
        tuple: &Tuple,
    ) -> usize {
        let mut hasher = DefaultHasher::new();
        for &col in &self.hash_columns {
            tuple.value(col).hash(&mut hasher);
        }
        (hasher.finish() as usize) % self.num_consumers
    }
}

/// Decide parallelism degree for a plan node.
pub fn choose_parallelism(
    plan: &RelExpr,
    available_cores: usize,
    max_workers: usize,
) -> usize {
    let table_pages = plan.estimated_pages();
    let min_pages_per_worker = 1000;

    let data_parallelism =
        (table_pages / min_pages_per_worker).max(1);

    data_parallelism.min(available_cores).min(max_workers)
}
```

## Preconditions

- Multiple CPU cores available
- Table large enough to justify parallel overhead
  (typically > 1000 pages)
- Operators in the parallel sub-tree are not inherently sequential
- Sufficient memory for per-worker buffers and exchange queues
- No correlated subqueries crossing exchange boundaries

## Cost Model

**Exchange overhead:**
- Queue enqueue/dequeue: ~100-200 ns per tuple (synchronization)
- Thread creation: ~10-50 us per worker (one-time)
- Memory: buffer_size x tuple_width per queue
- For 1M tuples, 1024-slot queue: ~200 ms queue overhead

**Speedup model:**
- Ideal: `speedup = DOP` (linear scaling)
- Practical: `speedup = DOP / (1 + overhead_fraction)`
- Typical overhead: 5-20% for exchange coordination
- Amdahl's law: serial fraction limits max speedup

**Partitioning costs:**
- Hash: O(1) per tuple (hash computation + modulo)
- Range: O(log P) per tuple (binary search on boundaries)
- Round-robin: O(1) per tuple (counter increment)
- Broadcast: O(N) per tuple (copied N times)

**When exchange is profitable:**
- Table > 10,000 pages (enough work per worker)
- DOP <= available cores (no over-subscription)
- Exchange overhead < (1 - 1/DOP) x sequential cost
- No skew in partitioning (balanced work distribution)

**Skew handling:**
- Hash skew: popular keys overload one partition
- Mitigation: work stealing, adaptive repartitioning
- Detection: monitor queue depths during execution

## Test Cases

```sql
-- Test 1: Parallel sequential scan with gather
SET max_parallel_workers_per_gather = 4;
SELECT COUNT(*) FROM large_table;
-- Expected: 4 workers scan portions, gather merges partial counts
-- Verify: EXPLAIN shows "Gather" with "Workers Planned: 4"
-- Speedup: ~3-4x over single-threaded

-- Test 2: Parallel hash join
SELECT * FROM orders o
JOIN customers c ON o.cust_id = c.id;
-- Expected: parallel scan of orders, each worker builds
--   partial hash table from broadcast customers
-- Verify: "Parallel Hash Join" in EXPLAIN

-- Test 3: Repartition for aggregation
SELECT region, COUNT(*)
FROM orders
GROUP BY region;
-- Expected: parallel scan → partial agg per worker
--   → repartition by region → final agg
-- Verify: "Partial HashAggregate" + "Finalize HashAggregate"

-- Test 4: Gather merge (preserving order)
SET enable_sort = on;
SELECT * FROM orders ORDER BY created_at LIMIT 100;
-- Expected: each worker produces sorted portion
--   Gather Merge combines sorted streams
-- Verify: "Gather Merge" in EXPLAIN (not plain Gather)

-- Test 5: Small table - no parallelism
SELECT * FROM config WHERE key = 'setting';
-- Expected: sequential scan, no exchange overhead
-- Verify: no parallel workers in plan

-- Negative test: skewed partitioning
SELECT customer_id, COUNT(*)
FROM orders
GROUP BY customer_id;
-- If one customer has 90% of orders:
--   one worker gets 90% of work
-- Expected: near-sequential performance despite DOP > 1
-- Mitigation: detect skew, use work stealing
```

## References

1. **Graefe, Goetz**. "Encapsulation of Parallelism in the Volcano
   Query Processing System." SIGMOD 1990.
   - Original exchange operator paper
   - Defines gather, scatter, broadcast variants

2. **Graefe, Goetz**. "Volcano: An Extensible and Parallel Query
   Evaluation System." IEEE TKDE 6(1), 1994.
   - Exchange operator integration with iterator model

3. **Graefe, Goetz**. "Sorting & Hashing Revisited: Building
   Efficient and Compact Data Processing Pipelines." IEEE Data
   Engineering Bulletin 38(1), 2015.
   - Modern perspective on parallel sort and hash with exchange

4. **PostgreSQL Source**: `src/backend/executor/nodeGather.c`
   - Gather and Gather Merge implementations
   - Worker management and tuple queue

5. **PostgreSQL Source**: `src/backend/access/transam/parallel.c`
   - Parallel worker infrastructure
   - Shared memory setup for exchange

6. **Leis, Viktor et al**. "Morsel-Driven Parallelism: A NUMA-Aware
   Query Evaluation Framework." SIGMOD 2014.
   - Contrasts exchange-based parallelism with morsel model
   - Shows exchange limitations for NUMA architectures
